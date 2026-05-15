# auth module

`ApiClient` holds `Arc<dyn AuthStrategy>`. Each method is one module that
implements the trait. Adding a method is **one variant + one module** — no
caller updates.

## Trait surface (`strategy.rs`)

```rust
trait AuthStrategy {
    fn method(&self) -> AuthMethod;
    async fn authorization(&self, http: &reqwest::Client) -> Result<String>;
    fn build_url(&self, service: Service, path: &str) -> String;
    fn rewrite_url(&self, service: Service, external_url: &str) -> String;
    fn cloud_id(&self) -> Option<&str>;
    async fn probe_identity(&self, &ApiClient) -> Result<Option<Identity>>;
    fn identity_label(&self) -> String;
}
```

`probe_identity` returning `Ok(None)` means "this principal has no human
identity to probe" (service_account) — it is **not** a credential failure.
`config validate` renders the label instead of bailing.

The shared `/myself` probe lives in `strategy::probe_myself` so neither
`basic` nor `oauth` reaches into the other module to share it.

## Single source of truth

Every OAuth default and lifecycle constant is declared in `auth.rs`:

| Constant | Used by |
|---|---|
| `DEFAULT_OAUTH_REDIRECT_PORT` (8976) | serde default on `AuthConfig::OAuth`, `AuthResolver` |
| `DEFAULT_OAUTH_SCOPES` / `default_oauth_scopes()` | same |
| `TOKEN_REFRESH_BUFFER_SECS` (300) | both `service_account` and `oauth` strategies |
| `DEFAULT_TOKEN_LIFETIME_SECS` (3600) | both, fallback when `expires_in` is missing |

Change a value once.

## `AuthConfig::oauth_params(profile)` is the single extraction point

Every `auth` subcommand goes through `Config::oauth_params()` → which calls
`AuthConfig::oauth_params(profile)`. This is the only place that decides
whether the active profile is OAuth-configured and produces the canonical
error message when it isn't.

## URL building is shared

`service_account` and `oauth` both route through `api.atlassian.com/ex/...`,
so the URL builder lives in `client.rs` as `proxy_url` / `rewrite_via_proxy`.
Both strategy impls delegate; neither inlines the format string.

## Secret hygiene

- Every secret-bearing field is wrapped in `secrecy::SecretString`
  (`encoded` in basic, `client_secret` in service_account token manager,
  `access_token` / `refresh_token` in oauth `TokenSet`).
- `AuthConfig`, `OAuthParams`, and `CliOverrides` all have **manual `Debug`
  impls that redact secrets**. Do not change them back to `#[derive(Debug)]`
  — there is a regression test (`debug_never_leaks_secrets`).

## OAuth specifics (`auth/oauth/`)

- `flow.rs` runs the authorize → code → token exchange via the `oauth2`
  crate (PKCE S256, `audience=api.atlassian.com`, `prompt=consent`).
  The crate is built against reqwest 0.12; we use a dedicated 0.12 client
  for OAuth endpoint calls while API traffic stays on the binary's 0.13.
- `callback.rs` is a one-shot loopback HTTP receiver on
  `127.0.0.1:{redirect_port}/callback` (RFC 8252). Pure tokio TCP — no
  HTTP framework dependency.
- `store.rs` persists tokens. Tries OS keychain first; falls back to
  `~/.config/atlassian-cli/credentials.json` (0600, atomic via tempfile).
  Per-profile keyed. `TokenStorageBackend` is surfaced by `auth status`.
- `strategy.rs` holds the `OAuthStrategy`. `tokens: Mutex<TokenSet>` so
  concurrent callers serialize on refresh — at most one token-endpoint
  round trip when the cache is stale. Refresh tokens **rotate** on every
  use; the merged set replaces the stored one atomically.
- OAuth redirect URI must use `127.0.0.1` literally, never `localhost`.
- `OAuthStrategy::login` returns `LoginOutcome` only. The runtime
  `profile` (storage key) is a separate argument so `OAuthParams` holds
  pure config data.

## Service-account specifics

- `ServiceAccountTokenManager` (`pub(crate)`) caches the access token in
  memory only — no persistence. Refresh on next call after the 5-minute
  buffer expires.
- `cloud_id` is auto-discovered through accessible-resources when not
  configured. Multiple accessible sites → bail with the list; user must
  pin one via `cloud_id`.

## Blank-value policy

Empty / whitespace-only env vars and CLI flags are treated as **absent**
(`config::non_blank*`). `export VAR=""` no longer shadows the config file
value — silent override is a worst-class credential bug. The rule applies
to `ATLASSIAN_AUTH_METHOD`, every `--*` flag, `ENV_DOMAIN`, and the list-
style envs (`JIRA_PROJECTS_FILTER`, etc.).
