# atlassian-cli

Rust 2024 edition, single binary. CLI for Atlassian Cloud (Jira + Confluence).

## Build / test / lint

```bash
cargo +1.97.1 build --release   # production binary at target/release/atlassian-cli
cargo test                      # unit tests
cargo clippy                    # lint; CI requires zero warnings
cargo fmt                       # format; CI enforces rustfmt
```

CI also runs `cargo-deny` (advisories/bans/licenses/sources) on `Cargo.toml`/`Cargo.lock` changes via the `Security` workflow — touch a dep and assume those gates apply.

## Auth model (non-obvious)

Four auth methods, selected **explicitly** via `ATLASSIAN_AUTH_METHOD=basic|scoped_token|service_account|oauth` or the `method` field inside `[default.auth]`. No heuristic detection.

| Method | Principal | Base URL | Required fields | Token storage |
|---|---|---|---|---|
| `basic` | user (token owner) | `https://{domain}/rest/...` | `domain`, `email`, `token` (classic/unscoped only) | config.toml |
| `scoped_token` | user (token owner) | `https://api.atlassian.com/ex/{jira,confluence}/{cloud_id}/rest/...` | `email`, `token` (must carry scopes); `cloud_id`, else `domain` to resolve it from | config.toml |
| `service_account` | non-human SA | `https://api.atlassian.com/ex/{jira,confluence}/{cloud_id}/rest/...` | `client_id`, `client_secret`; `cloud_id` auto-discovered if omitted | in-memory only |
| `oauth` | user (interactive) | `https://api.atlassian.com/ex/{jira,confluence}/{cloud_id}/rest/...` | `client_id`, `client_secret`, `redirect_port` (default 8976), `scopes` | OS keychain → 0600 file fallback |

`basic` and `scoped_token` carry the identical `Basic email:token` credential and differ only in host, because Atlassian accepts each token shape at exactly one of them: the site host ignores a scoped token (anonymous 401), the gateway rejects a classic one. They are two methods rather than one with a routing switch because the host is not a preference — it is a property of the credential the user holds. `execute` attaches a `hint` naming the other method on any 401. Classic tokens are being retired (everything issued before 2024-12-15 expired by 2026-05-12), so `scoped_token` is the forward path.

`scoped_token` resolves an omitted `cloud_id` from the unauthenticated `https://{domain}/_edge/tenant_info`, the first method in Atlassian's own cloud-ID guide; `accessible-resources` is not usable here because it wants a bearer token. That lookup is the one part of this method that touches the site host, so the failure says to pin `cloud_id` instead — which also makes `domain` unnecessary, the gateway host being a constant.

Runtime dispatch is via `trait auth::AuthStrategy` — each method is one module under `src/auth/`. `ApiClient` holds an `Arc<dyn AuthStrategy>` and never matches on the variant. The two URL columns above (direct domain vs proxy) are the reason `ApiClient` exists: API functions take service-relative paths only, never absolute URLs.

`AuthStrategy::build_url` is the **only** place a request host is decided, and it reads that host from local configuration alone. It also writes the `/` before the path itself rather than assuming the argument carries one — otherwise a path like `@host/x` extends the authority instead of the path, and `https://site` + `@host/x` resolves with `@host` as the host.

Pagination links from the API reach `build_url` only through `confluence::api::link_path`, which accepts exactly two shapes — already rooted at the site (`/…`), or absolute, in which case the path is kept and the host discarded — and rejects everything else instead of guessing. Both halves are load-bearing under either token method, where every request carries a reusable `email:token` credential.

Trait surface, secret handling, OAuth specifics, blank-value policy, and the single-source-of-truth constants are documented in `src/auth/CLAUDE.md` and load on demand when Claude reads files in that module.

### Auth subcommand tree

```
atlassian-cli auth login [--no-browser]    # PKCE flow, persists tokens
atlassian-cli auth logout                  # clears tokens (no-op on non-OAuth profiles, with a message)
atlassian-cli auth status                  # identity, expiry, scopes, storage backend
atlassian-cli auth refresh                 # force refresh (debugging)
```

Every `auth` subcommand routes through `Config::oauth_params(&self)`, which validates that the active profile is OAuth-configured and returns `OAuthParams`. The flow uses PKCE S256, `audience=api.atlassian.com`, `prompt=consent`, and the configured `scopes` (the default set includes `offline_access` — required for refresh tokens).

## Config resolution

`config::AuthResolver` is the single source of truth for auth fields. Precedence is strict and per-field: **CLI flag > env var > config file**. Method precedence: `ATLASSIAN_AUTH_METHOD` env beats the method in the config file; when the env method differs, file fields for the other method are dropped (not leaked into the new variant).

Config files use `#[serde(deny_unknown_fields)]`. Auth fields belong under `[default.auth]`.

Profile search walks: global (`~/.config/atlassian-cli/config.toml`) → project (`.atlassian.toml` or `.atlassian/config.toml` upward from cwd) → `--config` path. A profile must exist in at least one file; absence in any single file is not an error.

## API-version mix (intentional)

- **Jira**: all endpoints use `/rest/api/3/`. Search goes through `POST /rest/api/3/search/jql`.
- **Jira Agile**: board, sprint, epic endpoints use `/rest/agile/1.0/`. These route through the same Jira proxy (`/ex/jira/{cloud_id}/...`), so they use `Service::Jira` — no separate Service variant needed.
- **Confluence search**: `GET /wiki/rest/api/search` (v1) — v2 has no CQL equivalent yet.
- **Confluence pages, comments, labels, properties, spaces, attachments**: `/wiki/api/v2/*` for reads; label and attachment *writes* fall back to v1 (`/wiki/rest/api/content/...`) — see `src/confluence/CLAUDE.md`.
- **Confluence comments** are two v2 families (footer, inline) with an identical path algebra, and a page endpoint returns only ROOT comments. Reads walk every level; see `src/confluence/CLAUDE.md`.

This mix is deliberate — do not "modernize" the Confluence search path.

## Write-side behavior to know

- `jira create`/`update`/`comment`/`link`/`worklog`: plain text args auto-convert to ADF via `jira::adf::process_*_input`. For rich text, pass an ADF JSON document directly.
- `--format markdown` on reads does **not** return pure markdown — it keeps the JSON envelope and converts content fields (description, body) in place.
- `--stream` writes JSONL to stdout; progress/totals go to stderr. The function returns `Value::Null` so `output_json` suppresses any trailing output. Do not re-introduce a trailing summary line — it breaks `| jq`.
- **Error contract**: a failed run prints a single-line JSON object to stderr — `{"error":{"message",...}}`, plus `status`/`operation`/`hint` fields when the failure is a typed `ApiError` — and exits with a stable code: 1 generic, 2 CLI usage (clap), 3 auth (401/403), 4 not found (404), 5 rate limited (429), 6 server error (5xx). Stdout carries results only.
- **429 handling**: every API call routes through `ApiClient::execute`, which retries 429 up to 3 times (server `Retry-After` honored and capped at 60s, else exponential backoff from 500ms). Only 429 is retried — 5xx may have committed a write, so retrying it could duplicate the operation. Each retry re-derives the `Authorization` header (a backoff wait must never replay a token that expired during it). Multipart requests (attachment upload) cannot be cloned and are sent exactly once.
- **Destructive-op guard**: whole-resource deletes (`jira delete`, `confluence delete`, `self uninstall`, `self skill remove`) require an explicit `--yes` at the CLI layer (the binary is non-interactive/JSON-first, so a prompt would hang pipelines — a required flag is the guard). The API functions (`delete_issue`/`delete_page`) stay pure; the `--yes` check lives in the `main.rs` handler. Targeted sub-resource removals that already require a specific id — Jira `comment delete`, `link remove`, `worklog remove`, `watcher remove`; Confluence `comment delete`, `label remove`, `property delete` — do **not** require `--yes`, because the id/name/key is the specificity guard. Jira issue delete is irreversible (no recycle bin); Confluence page delete goes to trash.

## Installation lifecycle (`self`)

```
atlassian-cli self status                    # version, paths, skill byte-state, stored profiles — no network
atlassian-cli self update [--version V] [--force] [--verify-attestations]
atlassian-cli self skill install             # writes the skill this binary carries
atlassian-cli self skill remove --yes
atlassian-cli self uninstall --yes [--keep-skill] [--keep-credentials] [--purge-config]
```

`src/dist/` owns this and does not touch `ApiClient` — it talks to GitHub. The skill is compiled into the binary (`include_str!`), so binary and skill cannot be different versions and a deployed copy is checked byte-for-byte. `self update` proves the downloaded binary runs and reports the expected version **before** replacing anything, so there is no half-installed state to roll back from. `scripts/install.sh` and `scripts/uninstall.sh` delegate here rather than restating what an installation consists of. See `src/dist/CLAUDE.md`.

`self status` deliberately makes no network call; "is there a newer one" is answered by `self update`, which changes nothing when the running binary is already current.

## Auto-injected filters

Both `config.jira.projects_filter` and `config.confluence.spaces_filter` route through the **single** `query_utils::inject_filter(query, clause_re, injected_clause)` helper. Jira passes `PROJECT_CLAUSE_RE` + `project IN (...)`, Confluence passes `SPACE_CLAUSE_RE` + `space IN (...)` — the only per-language differences. The helper:

- masks quoted literals (`query_utils::mask_string_literals`, both `"` and `'`) before any detection, so `summary ~ "project = foo"` and `project = 'X'` are handled correctly;
- skips injection when the masked query already matches the clause regex (`projectId = 10` does not match — word boundary);
- preserves a trailing `ORDER BY` (appended after the injected clause, never wrapped inside the condition group);
- collapses an empty/whitespace condition body to just the injected clause (no dangling `AND ()`).

Do not reintroduce a second copy of this logic — the two languages diverged in earlier revisions (CQL produced invalid `AND (order by …)`); the shared helper exists to prevent that drift.

## Adding a new command

Multi-operation domains (`comment`, `transition`, `link`, `worklog`, `watcher`, `board`, `sprint`, `epic`) use nested subcommands via an `Action` enum (e.g. `CommentAction`, `LinkAction`). The Confluence side mirrors this: `comment`, `label`, `property`, `space`, `attachment` route through `Confluence*Action` enums. Global discovery (`list types/priorities/statuses/labels`) uses the dedicated `ListAction` group. Single-operation commands (`get`, `create`, `update`) remain flat. `board` currently has one operation but is nested so future additions don't break the CLI surface.

1. For a new domain with multiple operations: add an `XAction` enum with variants (`Add`, `List`, `Remove`, etc.), then a `JiraSubcommand::X { action: XAction }` variant in `main.rs`.
2. Add the match arm in `handle_jira`/`handle_confluence`/`handle_config`.
3. Implement the async function in `jira/api.rs` or `confluence/api.rs`, taking `client: &ApiClient`. Build the request with `client.get/post/put/delete(Service::X, "/service-relative/path")` and send it through `client.execute(operation, request)` — never call `.send()` directly. `execute` retries 429 (honoring `Retry-After`, safe for non-idempotent writes because 429 guarantees the request was not processed) and converts any non-2xx into a typed `ApiError`. Service-relative paths only — never construct absolute URLs.
4. **URL safety**: percent-encode user input in path segments via `http_utils::encode_path_segment`. Use the reqwest `.query(&[(k, v)])` builder for query params containing user input — never `format!` user input into the URL string. Do not encode server-side identifiers (cloud IDs, numeric resource IDs) — the AsciiSet re-encodes `:` and would corrupt those.
5. **Pagination**: `startAt`/`maxResults` Jira endpoints must use the shared `paginate` helper, naming the `PageContract` they follow — `AGILE_PAGE` (`values` + `isLast`) or `COMMENT_PAGE` (`comments` + `total`). Both bail on a page missing its items or its end signal rather than silently truncating; a new contract is a new constant, never a second loop. Confluence v2 list endpoints are cursor-paginated — route them through `fetch_all_v2_results` instead (see `src/confluence/CLAUDE.md`). Either way, never return only the first page.
6. **Bulk writes**: Agile bulk endpoints (sprint/backlog/epic moves) cap each POST at `AGILE_BULK_LIMIT = 50` issues. Route them through `post_issue_batches`, which chunks transparently and reports `processed/total` on partial failure.
7. **Query filters**: when matching keyword-prefixed clauses (`project`, `space`) in user-provided JQL/CQL, run the regex against `query_utils::mask_string_literals(input)` so quoted text doesn't false-positive.
8. List endpoints must return `{"items": [...]}` envelope. Write endpoints that create return `{"id": ...}`. Side-effect-only writes return `{}`.
9. Read endpoints must call `filter::apply(&mut data, client.config())` before returning.
10. API failures come from `client.execute` as `client::ApiError` (Display: `Failed to {operation} ({status}): {body}`). Do not hand-roll status checks or `bail!` with ad-hoc error strings — the typed error is what maps to exit codes and the structured stderr object in `main.rs::render_error`.
11. Tests must drive the production async function against a `wiremock::MockServer` via `test_utils::mock_client(server.uri())`. Verify method, path, query params, request body, and response envelope — synthetic data-shape assertions do not validate behavior.

## Debugging

- `-v` (info), `-vv` (debug), `-vvv` (trace) — logs go to stderr.
- `config validate` constructs the strategy (which performs each method's own credential check) and then calls `AuthStrategy::probe_identity`. For service account auth `probe_identity` returns `None` — credentials are still verified, but the `/myself` endpoint typically lacks scope.
- `--profile <name>` switches between config profiles. Profiles are independent. When `--profile` is omitted, the profile name resolves to the literal string `default`.

## Security invariants

- Domain validation goes through `config::validate_atlassian_domain` (shared by `Config::validate` and `BasicStrategy::new`). It strips the scheme/trailing slash, rejects any byte outside `[A-Za-z0-9.-]` — which blocks path (`/`), query (`?`), fragment (`#`), userinfo (`@`), and port (`:`) spoofs like `https://evil.com/foo.atlassian.net` — then requires a non-empty label before `.atlassian.net`. A bare suffix check is **not** sufficient: the path-prefixed form would otherwise send Basic credentials to the attacker host.
- A `cloud_id` is validated by `config::validate_cloud_id` (rejects anything outside `[A-Za-z0-9-]`) before it is interpolated into the `/ex/{service}/{cloud_id}` proxy path. Validation runs at **strategy construction** (`ScopedTokenStrategy::connect`, `ServiceAccountStrategy::connect`, `OAuthStrategy::resume`), not only in `Config::validate` — so a pinned value reaching the proxy via an `auth` subcommand (which uses `load_without_validation`) or a tampered `credentials.json` is still caught. A discovered id is validated on the same path rather than trusted for having come from an API: `scoped_token` reads it from the site host, so an unchecked one would let that response steer the proxy path.
- Secrets are `#[serde(skip_serializing)]` on `AuthConfig`, and the `config show` output masks them to first-4 + `***`. Don't print resolved tokens anywhere else.
- Config files at 0600 are recommended; the loader warns (does not bail) on looser permissions.
- OAuth tokens in memory are wrapped in `secrecy::SecretString` — `Debug`/`Display` redact automatically. Use `ExposeSecret` at the smallest scope possible.
- OAuth redirect URI must use `127.0.0.1` (literal IP), not `localhost` — DNS spoofing on `localhost` is conceivable in adversarial network setups; the IP is unambiguous.
- OAuth `state` parameter is generated via CSPRNG (`CsrfToken::new_random`) and validated on the callback. Mismatch → reject + clean error.
- PKCE is **always** used (S256). Atlassian permits public-client OAuth without PKCE but every CLI is a public client, so we enforce it.
- `credentials.json` is 0600; parent directory 0700. Loader warns on looser perms. Atomic writes via `tempfile::persist` prevent partial files on crash.
- `self update` verifies the published SHA-256 before the archive is used for anything, and never installs bytes that fail it. The archive is staged in a directory `tempfile` creates exclusively and this code narrows to 0700 — the system temp directory is world-writable and an archive's name is fully predictable, so a known path there could be pre-created as a symlink or swapped between the write and `gh attestation verify`. Nothing is unpacked: `read_from_tar_gz` returns one named member's bytes, so no path chosen by the archive reaches the filesystem.
