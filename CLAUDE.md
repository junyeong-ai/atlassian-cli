# atlassian-cli

Rust 2024 edition, single binary. CLI for Atlassian Cloud (Jira + Confluence).

## Build / test / lint

```bash
cargo +1.95.0 build --release   # production binary at target/release/atlassian-cli
cargo test                      # unit tests
cargo clippy                    # lint; CI requires zero warnings
cargo fmt                       # format; CI enforces rustfmt
cargo audit                     # transitive-CVE check; CI runs this
```

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
- **Confluence search**: `GET /wiki/rest/api/search` (v1) — v2 has no CQL equivalent yet.
- **Confluence pages/spaces/comments**: `/wiki/api/v2/*`.

This mix is deliberate — do not "modernize" the Confluence search path.

## Write-side behavior to know

- `jira create`/`update`/`comment`: plain text args auto-convert to ADF via `jira::adf::process_*_input`. For rich text, pass an ADF JSON document directly.
- `--format markdown` on reads does **not** return pure markdown — it keeps the JSON envelope and converts content fields (description, body) in place.
- `--stream` writes JSONL to stdout; progress/totals go to stderr. The function returns `Value::Null` so `output_json` suppresses any trailing output. Do not re-introduce a trailing summary line — it breaks `| jq`.

## Auto-injected filters

When `config.jira.projects_filter` is non-empty, bare JQL is wrapped: `status = Open` → `project IN ("P1","P2") AND (status = Open)`. Injection is skipped when the JQL already contains a `project` clause — Jira detection uses a word-boundary regex (`PROJECT_CLAUSE_RE`) so `projectId = 10` does not count.

`config.confluence.spaces_filter` wraps the same way (`space IN (...) AND (...)`), but skip detection is a lowercased substring check for `space `, `space=`, or `space in` — accept the looser detection since CQL has fewer identifier collisions than JQL.

## Adding a new command

1. Add a variant to `JiraSubcommand`/`ConfluenceSubcommand`/`ConfigSubcommand` in `main.rs` (include doc comment + flag `help` strings — clap surfaces them in `--help`).
2. Add the match arm in `handle_jira`/`handle_confluence`/`handle_config`.
3. Implement the async function in `jira/api.rs` or `confluence/api.rs`, taking `client: &ApiClient` and using `client.get/post/put(Service::X, "/service-relative/path")`. Service-relative paths only — never construct absolute URLs.
4. If it's a read operation and you're adding new tests, extend `test_utils.rs` rather than duplicating fixtures.

## Debugging

- `-v` (info), `-vv` (debug), `-vvv` (trace) — logs go to stderr.
- `config validate` constructs the strategy (which performs each method's own credential check) and then calls `AuthStrategy::probe_identity`. For service account auth `probe_identity` returns `None` — credentials are still verified, but the `/myself` endpoint typically lacks scope.
- `--profile <name>` switches between config profiles. Profiles are independent; the default profile in this repo is `oauth`, with `service` and `basic` available as fallbacks.

## Security invariants

- Domain validation requires `ends_with(".atlassian.net")` — substring match would let `evil.atlassian.net.attacker.com` through.
- Secrets are `#[serde(skip_serializing)]` on `AuthConfig`, and the `config show` output masks them to first-4 + `***`. Don't print resolved tokens anywhere else.
- Config files at 0600 are recommended; the loader warns (does not bail) on looser permissions.
- OAuth tokens in memory are wrapped in `secrecy::SecretString` — `Debug`/`Display` redact automatically. Use `ExposeSecret` at the smallest scope possible.
- OAuth redirect URI must use `127.0.0.1` (literal IP), not `localhost` — DNS spoofing on `localhost` is conceivable in adversarial network setups; the IP is unambiguous.
- OAuth `state` parameter is generated via CSPRNG (`CsrfToken::new_random`) and validated on the callback. Mismatch → reject + clean error.
- PKCE is **always** used (S256). Atlassian permits public-client OAuth without PKCE but every CLI is a public client, so we enforce it.
- `credentials.json` is 0600; parent directory 0700. Loader warns on looser perms. Atomic writes via `tempfile::persist` prevent partial files on crash.
