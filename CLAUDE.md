# atlassian-cli

Rust 2024 edition, single binary. CLI for Atlassian Cloud (Jira + Confluence).

## Build / test / lint

```bash
cargo +1.95.0 build --release   # production binary at target/release/atlassian-cli
cargo test                      # unit tests (env-var tests serialize via an internal Mutex)
cargo clippy                    # lint; CI requires zero warnings
cargo fmt                       # format; CI enforces rustfmt
cargo audit                     # transitive-CVE check; CI runs this (rustls-webpki CVEs have hit us)
```

## Auth model (non-obvious)

Three auth methods, selected **explicitly** via `ATLASSIAN_AUTH_METHOD=basic|service_account|oauth` or the `method` field inside `[default.auth]`. No heuristic detection.

| Method | Principal | Base URL | Required fields | Token storage |
|---|---|---|---|---|
| `basic` | user (token owner) | `https://{domain}/rest/...` | `domain`, `email`, `token` | config.toml |
| `service_account` | non-human SA | `https://api.atlassian.com/ex/{jira,confluence}/{cloud_id}/rest/...` | `client_id`, `client_secret`; `cloud_id` auto-discovered if omitted | in-memory only |
| `oauth` | user (interactive) | `https://api.atlassian.com/ex/{jira,confluence}/{cloud_id}/rest/...` | `client_id`, `client_secret`, `redirect_port` (default 8976), `scopes` | OS keychain → 0600 file fallback |

Runtime dispatch is via `trait auth::AuthStrategy` — each method lives in its own module (`auth::basic`, `auth::service_account`, `auth::oauth`). `ApiClient` holds an `Arc<dyn AuthStrategy>` and never matches on the variant. Adding a new method = new module + one `AuthConfig` enum variant.

- `AuthMethod` (public enum) is the typed method tag returned by `AuthStrategy::method()`. Use it instead of comparing strings — `as_str()` produces the canonical TOML/env form.
- Identity probing lives on the trait: `AuthStrategy::probe_identity(&ApiClient) -> Result<Option<Identity>>`. `Some` = a `/myself` lookup succeeded (basic, oauth); `None` = the principal doesn't expose a human identity (service_account). `config validate` calls this once and renders accordingly — no `if method == ...` branching in main.rs.
- The shared `/myself` probe lives at `auth::strategy::probe_myself` (crate-internal) so neither `basic` nor `oauth` reaches into the other's module to share it.
- All defaults and token-lifecycle parameters are **single-sourced** in `auth.rs`: `DEFAULT_OAUTH_REDIRECT_PORT`, `default_oauth_scopes`, `TOKEN_REFRESH_BUFFER_SECS`, `DEFAULT_TOKEN_LIFETIME_SECS`. Both serde defaults and `config::AuthResolver` read those constants — change a value once.
- The base URL divergence is the reason `ApiClient` exists — API functions take `&ApiClient` and service-relative paths, never absolute URLs.
- Service account tokens and OAuth access tokens refresh automatically with the same buffer (`TOKEN_REFRESH_BUFFER_SECS`, 5 min).
- OAuth refresh tokens **rotate** on each use (Atlassian); the strategy serializes refresh attempts via a `Mutex<TokenSet>` so concurrent callers issue at most one token-endpoint round trip, and the persistent store is overwritten atomically with the new pair.
- OAuth tokens persist via `auth::oauth::store::TokenStore`: tries OS keychain (macOS / Linux Secret Service / Windows Credential Manager) and falls back to `~/.config/atlassian-cli/credentials.json` (0600, atomic via `tempfile`). Per-profile keying. `TokenStorageBackend` (`Keyring`/`File`) is surfaced by `auth status`.
- OAuth redirect URI is `http://127.0.0.1:{redirect_port}/callback` — port MUST match what is registered at developer.atlassian.com. RFC 8252 (loopback IP for native apps).
- `OAuthStrategy::login` returns just `LoginOutcome` (no leftover strategy handle); `resume` loads a strategy from persisted tokens. The runtime `profile` (used as token-storage key) is passed as a separate argument — `OAuthParams` holds pure config data only.
- `AuthConfig::oauth_params(profile)` is the single extraction point for OAuth flow inputs (used by every `auth` subcommand); it produces consistent error messages when the profile isn't OAuth-configured.
- `AuthResolver::resolve` dispatches to one `resolve_*` helper per variant. Adding a fourth method adds a sibling helper, not another inline match arm.
- Confluence pagination returns absolute URLs from the API; `ApiClient::rewrite_url` rewrites them to the proxy host under service_account / oauth auth.

### Auth subcommand tree

```
atlassian-cli auth login [--no-browser]    # PKCE flow, persists tokens
atlassian-cli auth logout                  # clears tokens (no-op on non-OAuth profiles, with a message)
atlassian-cli auth status                  # identity, expiry, scopes, storage backend
atlassian-cli auth refresh                 # force refresh (debugging)
```

All four subcommands route through `oauth_params_from_config` (main.rs) which extracts the OAuth params from the active profile and produces a uniform error message when the profile isn't OAuth. The flow uses PKCE S256, `audience=api.atlassian.com`, `prompt=consent`, and the configured `scopes` (default set includes `offline_access` — required for refresh tokens).

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

When `config.jira.projects_filter` is non-empty, bare JQL is wrapped: `status = Open` → `project IN ("P1","P2") AND (status = Open)`. Injection is skipped when the JQL already contains a `project` clause — detection uses a word-boundary regex (not substring) so `projectId = 10` does not count. Confluence's `space` filter follows the same shape.

## Adding a new command

1. Add a variant to `JiraSubcommand`/`ConfluenceSubcommand`/`ConfigSubcommand` in `main.rs` (include doc comment + flag `help` strings — clap surfaces them in `--help`).
2. Add the match arm in `handle_jira`/`handle_confluence`/`handle_config`.
3. Implement the async function in `jira/api.rs` or `confluence/api.rs`, taking `client: &ApiClient` and using `client.get/post/put(Service::X, "/service-relative/path")`. Service-relative paths only — never construct absolute URLs.
4. If it's a read operation and you're adding new tests, extend `test_utils.rs` rather than duplicating fixtures.

## Debugging

- `-v` (info), `-vv` (debug), `-vvv` (trace) — logs go to stderr.
- `config validate` checks the configured credentials against Atlassian auth/API endpoints. For service account auth this means token fetch + accessible-resources lookup; individual Jira/Confluence calls still depend on OAuth scopes and product permissions.
- `--profile <name>` switches between config profiles (e.g. a service account `default` and a Basic `fallback`).

## Security invariants

- Domain validation requires `ends_with(".atlassian.net")` — substring match would let `evil.atlassian.net.attacker.com` through.
- Secrets are `#[serde(skip_serializing)]` on `AuthConfig`, and the `config show` output masks them to first-4 + `***`. Don't print resolved tokens anywhere else.
- Config files at 0600 are recommended; the loader warns (does not bail) on looser permissions.
- OAuth tokens in memory are wrapped in `secrecy::SecretString` — `Debug`/`Display` redact automatically. Use `ExposeSecret` at the smallest scope possible.
- OAuth redirect URI must use `127.0.0.1` (literal IP), not `localhost` — DNS spoofing on `localhost` is conceivable in adversarial network setups; the IP is unambiguous.
- OAuth `state` parameter is generated via CSPRNG (`CsrfToken::new_random`) and validated on the callback. Mismatch → reject + clean error.
- PKCE is **always** used (S256). Atlassian permits public-client OAuth without PKCE but every CLI is a public client, so we enforce it.
- `credentials.json` is 0600; parent directory 0700. Loader warns on looser perms. Atomic writes via `tempfile::persist` prevent partial files on crash.
