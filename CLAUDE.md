# atlassian-cli

Rust 2024 edition, single binary. CLI for Atlassian Cloud (Jira + Confluence).

## Build / test / lint

```bash
cargo +1.96.0 build --release   # production binary at target/release/atlassian-cli
cargo test                      # unit tests
cargo clippy                    # lint; CI requires zero warnings
cargo fmt                       # format; CI enforces rustfmt
```

CI also runs `cargo-deny` (advisories/bans/licenses/sources) on `Cargo.toml`/`Cargo.lock` changes via the `Security` workflow — touch a dep and assume those gates apply.

## Auth model (non-obvious)

Three auth methods, selected **explicitly** via `ATLASSIAN_AUTH_METHOD=basic|service_account|oauth` or the `method` field inside `[default.auth]`. No heuristic detection.

| Method | Principal | Base URL | Required fields | Token storage |
|---|---|---|---|---|
| `basic` | user (token owner) | `https://{domain}/rest/...` | `domain`, `email`, `token` | config.toml |
| `service_account` | non-human SA | `https://api.atlassian.com/ex/{jira,confluence}/{cloud_id}/rest/...` | `client_id`, `client_secret`; `cloud_id` auto-discovered if omitted | in-memory only |
| `oauth` | user (interactive) | `https://api.atlassian.com/ex/{jira,confluence}/{cloud_id}/rest/...` | `client_id`, `client_secret`, `redirect_port` (default 8976), `scopes` | OS keychain → 0600 file fallback |

Runtime dispatch is via `trait auth::AuthStrategy` — each method is one module under `src/auth/`. `ApiClient` holds an `Arc<dyn AuthStrategy>` and never matches on the variant. The two URL columns above (direct domain vs proxy) are the reason `ApiClient` exists: API functions take service-relative paths only, never absolute URLs. Confluence pagination returns absolute URLs from the API; `ApiClient::rewrite_url` reroutes them under proxy-based methods.

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

This mix is deliberate — do not "modernize" the Confluence search path.

## Write-side behavior to know

- `jira create`/`update`/`comment`/`link`/`worklog`: plain text args auto-convert to ADF via `jira::adf::process_*_input`. For rich text, pass an ADF JSON document directly.
- `--format markdown` on reads does **not** return pure markdown — it keeps the JSON envelope and converts content fields (description, body) in place.
- `--stream` writes JSONL to stdout; progress/totals go to stderr. The function returns `Value::Null` so `output_json` suppresses any trailing output. Do not re-introduce a trailing summary line — it breaks `| jq`.
- **Destructive-op guard**: whole-resource deletes (`jira delete`, `confluence delete`) require an explicit `--yes` at the CLI layer (the binary is non-interactive/JSON-first, so a prompt would hang pipelines — a required flag is the guard). The API functions (`delete_issue`/`delete_page`) stay pure; the `--yes` check lives in the `main.rs` handler. Targeted sub-resource removals that already require a specific id — Jira `comment delete`, `link remove`, `worklog remove`, `watcher remove`; Confluence `comment delete`, `label remove`, `property delete` — do **not** require `--yes`, because the id/name/key is the specificity guard. Jira issue delete is irreversible (no recycle bin); Confluence page delete goes to trash.

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
3. Implement the async function in `jira/api.rs` or `confluence/api.rs`, taking `client: &ApiClient` and using `client.get/post/put/delete(Service::X, "/service-relative/path")`. Service-relative paths only — never construct absolute URLs.
4. **URL safety**: percent-encode user input in path segments via `http_utils::encode_path_segment`. Use the reqwest `.query(&[(k, v)])` builder for query params containing user input — never `format!` user input into the URL string. Do not encode server-side identifiers (cloud IDs, numeric resource IDs) — the AsciiSet re-encodes `:` and would corrupt those.
5. **Pagination**: Jira/Agile endpoints that follow the `values`/`isLast`/`startAt` contract must use the shared `paginate_values` helper (bails on missing `values`/`isLast` rather than silently truncating). Confluence v2 list endpoints are cursor-paginated — route them through `fetch_all_v2_results` instead (see `src/confluence/CLAUDE.md`). Either way, never return only the first page.
6. **Bulk writes**: Agile bulk endpoints (sprint/backlog/epic moves) cap each POST at `AGILE_BULK_LIMIT = 50` issues. Route them through `post_issue_batches`, which chunks transparently and reports `processed/total` on partial failure.
7. **Query filters**: when matching keyword-prefixed clauses (`project`, `space`) in user-provided JQL/CQL, run the regex against `query_utils::mask_string_literals(input)` so quoted text doesn't false-positive.
8. List endpoints must return `{"items": [...]}` envelope. Write endpoints that create return `{"id": ...}`. Side-effect-only writes return `{}`.
9. Read endpoints must call `filter::apply(&mut data, client.config())` before returning.
10. Error messages must include status code: `anyhow::bail!("Failed to X ({}): {}", status, body)`.
11. Tests must drive the production async function against a `wiremock::MockServer` via `test_utils::mock_client(server.uri())`. Verify method, path, query params, request body, and response envelope — synthetic data-shape assertions do not validate behavior.

## Debugging

- `-v` (info), `-vv` (debug), `-vvv` (trace) — logs go to stderr.
- `config validate` constructs the strategy (which performs each method's own credential check) and then calls `AuthStrategy::probe_identity`. For service account auth `probe_identity` returns `None` — credentials are still verified, but the `/myself` endpoint typically lacks scope.
- `--profile <name>` switches between config profiles. Profiles are independent. When `--profile` is omitted, the profile name resolves to the literal string `default`.

## Security invariants

- Domain validation goes through `config::validate_atlassian_domain` (shared by `Config::validate` and `BasicStrategy::new`). It strips the scheme/trailing slash, rejects any byte outside `[A-Za-z0-9.-]` — which blocks path (`/`), query (`?`), fragment (`#`), userinfo (`@`), and port (`:`) spoofs like `https://evil.com/foo.atlassian.net` — then requires a non-empty label before `.atlassian.net`. A bare suffix check is **not** sufficient: the path-prefixed form would otherwise send Basic credentials to the attacker host.
- A `cloud_id` is validated by `config::validate_cloud_id` (rejects anything outside `[A-Za-z0-9-]`) before it is interpolated into the `/ex/{service}/{cloud_id}` proxy path. Validation runs at **strategy construction** (`ServiceAccountStrategy::connect`, `OAuthStrategy::resume`), not only in `Config::validate` — so a pinned value reaching the proxy via an `auth` subcommand (which uses `load_without_validation`) or a tampered `credentials.json` is still caught. Auto-discovered IDs come from the API and pass trivially.
- Secrets are `#[serde(skip_serializing)]` on `AuthConfig`, and the `config show` output masks them to first-4 + `***`. Don't print resolved tokens anywhere else.
- Config files at 0600 are recommended; the loader warns (does not bail) on looser permissions.
- OAuth tokens in memory are wrapped in `secrecy::SecretString` — `Debug`/`Display` redact automatically. Use `ExposeSecret` at the smallest scope possible.
- OAuth redirect URI must use `127.0.0.1` (literal IP), not `localhost` — DNS spoofing on `localhost` is conceivable in adversarial network setups; the IP is unambiguous.
- OAuth `state` parameter is generated via CSPRNG (`CsrfToken::new_random`) and validated on the callback. Mismatch → reject + clean error.
- PKCE is **always** used (S256). Atlassian permits public-client OAuth without PKCE but every CLI is a public client, so we enforce it.
- `credentials.json` is 0600; parent directory 0700. Loader warns on looser perms. Atomic writes via `tempfile::persist` prevent partial files on crash.
