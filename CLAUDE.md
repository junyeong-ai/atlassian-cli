# atlassian-cli

Rust 2024 edition, single binary. CLI for Atlassian Cloud (Jira + Confluence).

## Build / test / lint

```bash
cargo build --release           # production binary at target/release/atlassian-cli
cargo test                      # unit tests (env-var tests serialize via an internal Mutex)
cargo clippy                    # lint; CI requires zero warnings
cargo fmt                       # format; CI enforces rustfmt
cargo audit                     # transitive-CVE check; CI runs this (rustls-webpki CVEs have hit us)
```

## Auth model (non-obvious)

Two auth methods, selected **explicitly** via `ATLASSIAN_AUTH_METHOD=basic|oauth` or the `method` field inside `[default.auth]`. No heuristic detection.

| Method | Base URL | Required fields |
|--------|----------|-----------------|
| Basic  | `https://{domain}/rest/...`                       | `domain`, `email`, `token` |
| OAuth  | `https://api.atlassian.com/ex/{jira,confluence}/{cloud_id}/rest/...` | `client_id`, `client_secret`; `cloud_id` auto-discovered if omitted |

- The base URL divergence is the reason `ApiClient` exists — API functions take `&ApiClient` and service-relative paths, never absolute URLs.
- OAuth tokens are refreshed automatically (5-minute buffer before expiry) inside `token::TokenManager`.
- Confluence pagination returns absolute URLs from the API; `ApiClient::rewrite_url` rewrites them to the proxy host under OAuth.

## Config resolution

`config::AuthResolver` is the single source of truth for auth fields. Precedence is strict and per-field: **CLI flag > env var > config file**. Method precedence: `ATLASSIAN_AUTH_METHOD` env beats the method in the config file; when the env method differs, file fields for the other method are dropped (not leaked into the new variant).

Config files use `#[serde(deny_unknown_fields)]`. Legacy flat-top `email`/`token` fields will fail to parse — they belong under `[default.auth]`.

Profile search walks: global (`~/.config/atlassian-cli/config.toml`) → project (`.atlassian.toml` upward from cwd) → `--config` path. A profile must exist in at least one file; absence in any single file is not an error.

## API-version mix (intentional)

- **Jira**: all endpoints use `/rest/api/3/`. Search goes through `POST /rest/api/3/search/jql` (the old GET `/search` is deprecated).
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
- `config validate` runs the full auth path end-to-end; for OAuth this means token fetch + accessible-resources lookup, so a successful run means credentials are truly valid (not just well-formed).
- `--profile <name>` switches between config profiles (e.g. an OAuth `default` and a Basic `fallback`).

## Security invariants

- Domain validation requires `ends_with(".atlassian.net")` — substring match would let `evil.atlassian.net.attacker.com` through.
- Secrets are `#[serde(skip_serializing)]` on `AuthConfig`, and the `config show` output masks them to first-4 + `***`. Don't print resolved tokens anywhere else.
- Config files at 0600 are recommended; the loader warns (does not bail) on looser permissions.
