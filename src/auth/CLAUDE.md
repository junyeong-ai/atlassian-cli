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
    fn cloud_id(&self) -> Option<&str>;
    async fn probe_identity(&self, &ApiClient) -> Result<Option<Identity>>;
    fn identity_label(&self) -> String;
}
```

`probe_identity` returning `Ok(None)` means "this principal has no human
identity to probe" (service_account) — it is **not** a credential failure.
`config validate` renders the label instead of bailing.

Every user-delegated method (basic, scoped_token, oauth) probes and reports
the failure as-is. Do not add a status-based escape hatch for a token scoped
away from Jira: the gateway answers insufficient scope with the same 401 it
uses for a bad credential, so nothing here can tell them apart, and mapping a
status to `Ok(None)` on a guess would hide real credential failures.

The shared `/myself` probe lives in `strategy::probe_myself`, and
`strategy::encode_basic_credential` holds the one copy of the `email:token`
base64 encoding, so no strategy reaches into another module to share either.

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

`scoped_token`, `service_account` and `oauth` all route through
`api.atlassian.com/ex/...`, so the URL builder lives in `client.rs` as
`proxy_url`. Every strategy impl delegates; none inlines the format string.

`build_url` is the single point where a request host is chosen, and each impl
derives it from configuration only (`basic` from the validated domain, the
other three from `ATLASSIAN_PROXY_BASE`). There is deliberately no entry point
that takes a caller-supplied absolute URL — that is what keeps a pagination
link from steering a `Basic email:token` header at an arbitrary host. Both
token methods send a reusable credential, so this matters twice over.

Every impl writes the `/` separator before `path` and trims any leading ones
off the argument, so `path` can only land in the path component. Plain
concatenation is **not** safe here: `https://{domain}` + `@evil/x` parses with
`evil` as the host and the domain as userinfo. `proxy_url` was already immune
(its literal `/ex/{service}/{cloud_id}` terminates the authority first) but
spells the separator the same way so both builders read identically.

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
  The crate's bundled `reqwest` feature is disabled; token-endpoint calls
  bridge into the binary's own reqwest/rustls client through
  `perform_oauth_request` (redirects hard-disabled — a followed redirect
  would hand the client credentials to the redirect target), keeping a
  single reqwest in the dependency tree.
- `callback.rs` is a one-shot loopback HTTP receiver on
  `127.0.0.1:{redirect_port}/callback` (RFC 8252). Pure tokio TCP — no
  HTTP framework dependency.
- `store.rs` persists tokens. `TokenStore::{save,load,delete}` are async.
  Tries OS keychain first (Keychain / Credential Manager / Secret Service —
  Linux uses pure-Rust zbus, no system `libdbus`); falls back to
  `~/.config/atlassian-cli/credentials.json` (0600, atomic via tempfile).
  `load` returns `LoadedTokens { tokens, backend }` so callers report
  provenance without a second store query. Per-profile keyed.
  `ATLASSIAN_NO_KEYCHAIN` (truthy) bypasses the keychain entirely — `keyring_op`
  short-circuits to `NoEntry` so all three ops fall through to the file store.
  This is for headless / AI-agent sessions on a desktop OS where the keychain
  prompts with a blocking GUI dialog. Explicit opt-out only; no auto-detection.
  **It is a per-environment setting, not a per-call toggle.** Because the flag
  forbids touching the keychain, `save`/`delete` cannot clear a token that a
  prior keychain login left behind: toggling the flag off later lets `load`
  resurrect that stale keychain token (keychain is read first), and `auth
  logout` while the flag is set clears only the file store. If you ever logged
  in with the keychain, run `auth logout` **without** the flag once to clear it
  before adopting the opt-out. This is inherent to "never touch the keychain" —
  not a bug to patch by re-introducing the blocking keychain call.
- `strategy.rs` holds the `OAuthStrategy`. `tokens: Mutex<TokenSet>` so
  concurrent callers serialize on refresh — at most one token-endpoint
  round trip when the cache is stale. Refresh tokens **rotate** on every
  use; the merged set replaces the stored one atomically.
- OAuth redirect URI must use `127.0.0.1` literally, never `localhost`.
- `OAuthStrategy::login` returns `LoginOutcome` only. The runtime
  `profile` (storage key) is a separate argument so `OAuthParams` holds
  pure config data.

## Scoped-token specifics

- The credential is `basic`'s, the routing is `service_account`'s. What is
  unique is where `cloud_id` comes from: `_edge/tenant_info` on the site host,
  sent **without** an `Authorization` header. The site host is precisely where
  a scoped token is ignored, so attaching one would spend a credential on a
  request that does not want it.
- `validate_cloud_id` runs at strategy construction, as it does for the other
  proxy methods — an id reaching the path from a config file that skipped
  `Config::validate` is still caught.
- A pinned `cloud_id` short-circuits discovery entirely, which is the
  documented answer for networks where the site host is unreachable.

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
