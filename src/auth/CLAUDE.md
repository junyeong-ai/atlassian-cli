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
away from Jira: the gateway answers insufficient scope with the same 401
*status* it uses for a bad credential — only the body differs (`Unauthorized;
scope does not match` against a bare `Unauthorized`) — so a status cannot tell
them apart, and mapping one to `Ok(None)` would hide real credential failures.
Reading the body instead would hang the decision on server prose, which is the
trade this codebase refuses everywhere else.

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

`auth login` and `auth refresh` go through `Config::oauth_params()` → which
calls `AuthConfig::oauth_params(profile)`. This is the only place that decides
whether the active profile is OAuth-configured and produces the canonical
error message when it isn't.

`logout` and `status` are about what is stored, not about what is configured,
so they do not ask: a profile moved off OAuth keeps its persisted session, and
gating on the method would leave that credential unreachable.

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

Every impl writes the `/` separator before `path` and consumes one leading
separator off the argument, so `path` can only land in the path component. Plain
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
  `ATLASSIAN_NO_KEYCHAIN` (truthy) bypasses the keychain entirely — `keychain`
  answers `Forbidden` without touching it, so reads and writes use the file
  store and a delete reports the file half alone.
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
- **One classification, `Keychain<T>`, produced only in `store.rs`'s `keychain`.**
  `keyring_core::Error` says what went wrong; the three consumers each ask a
  different question of it — should I use the file instead, did clearing this
  entry work, may I conclude nothing is stored. Reading the foreign enum at each
  site meant one shared meaning, and narrowing it to suit one consumer kept
  changing what another was entitled to assume. The variants are the answers
  instead: `Done`, `Empty`, `Forbidden` (the opt-out), `Absent` (this build
  carries no store — the only outcome absence may be concluded from), and
  `Unreachable`. Add a consumer and the compiler names every case.
- `save` commits to one backend and then clears the other so a later read
  cannot find an older session first. That clear is **reported, never raised**:
  the save has already happened, and returning an error would tell an operator
  their login failed while they are logged in — and would fail every token
  refresh on that machine besides. The two branches are not equally harmless,
  and the warnings say which is which. `load` reads the keychain first, so a
  file entry left behind by a keychain save is shadowed; a keychain entry left
  behind by a file save is the one a later read will find, and after rotation
  its refresh token is the revoked one. Neither can be resolved from here — the
  keychain that would not answer is the same one holding it — so the operator
  is told, and `auth logout` once it answers is the remedy.
- An unparseable `credentials.json` is never written over, because it still
  holds whatever sessions it names. That leaves no in-tool repair, so the parse
  error names the one there is: remove the file.
- `TokenStore::delete` clears both backends independently and reports a keychain
  that would not answer as a failure. The file goes first, so nothing is lost
  when the keychain is unreachable — but `auth logout` does exit non-zero there,
  and that is deliberate: every released target has a keychain compiled in, so
  "could not reach it" cannot be read as "there was nothing in it", and a token
  saved from a desktop session is exactly what would be left behind by a quiet
  success. Cleared means the entry went, was not there, this build carries no
  store, or the opt-out forbade the look — every outcome but "it would not
  answer". It is a statement about this call, not about the keychain: under the
  opt-out a session stored before the flag stays where it is, which is why that
  case is spelled out above. `auth logout` therefore reads only to name the
  backend it found, never to decide whether to clear — a read falls back to the
  file, so finding nothing there says nothing about the keychain.
- **The search spec is the store's, not the service's.** `stored_profiles` asks
  the keychain which profiles it holds, and each store defines the vocabulary
  that question is asked in: Apple and Secret Service match attributes
  (`service`), the Windows Credential Manager has none and matches target names
  by regex (`pattern`), rejecting any other key outright. So `search_spec` sits
  beside the stores rather than at the call site, and it only narrows — the
  service each returned entry names is what decides whether it is one of ours,
  and an entry that will not name itself leaves the listing incomplete rather
  than shortened.
- The fallback file is rewritten and removed whole, so `owned_credentials_file`
  requires it to be a regular file — on the write as well as the removal. The
  rewrite is a rename, so through a symlink it replaces the link, and the unlink
  removes the link; either way every token stays readable at the far end. It
  reads absence from `NotFound` alone, as `read_all_from` does: a path that
  cannot be stat'd is not an empty one, and taking it for one is what let an
  uninstall report a profile cleared without having read it.
- **Nothing store-shaped leaves `keychain`.** A `keyring_core::Entry` looks like
  data and is not: on Linux, reading one blocks on the session bus through
  zbus's private tokio runtime, and `Runtime::block_on` panics on a thread that
  is already driving futures. `stored_profiles` therefore reads each entry's
  specifiers inside the closure and returns the profile names, not the entries.
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
