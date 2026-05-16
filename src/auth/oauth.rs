//! OAuth 2.0 3LO (Authorization Code + PKCE) — user-delegated auth.
//!
//! Sub-layout (encapsulated; only the items re-exported below are public):
//! - `callback` — one-shot loopback HTTP receiver (RFC 8252).
//! - `flow` — authorize URL + code exchange + refresh against Atlassian.
//! - `store` — persistent token store (keyring + 0600 file fallback).
//! - `strategy` — implements [`OAuthStrategy`].
//!
//! Public surface to the rest of the crate:
//! - [`OAuthStrategy`] — the runtime `AuthStrategy` impl.
//! - [`OAuthParams`] — the inputs shared by `login` and `resume`.
//! - [`LoginOutcome`] + [`SiteInfo`] — `auth login` reporting types.
//! - [`TokenStorageBackend`] — `Keyring` or `File`, surfaced by `auth status`.

mod callback;
mod flow;
mod store;
mod strategy;

pub use flow::{LoginOutcome, SiteInfo};
pub use store::{LoadedTokens, TokenSet, TokenStorageBackend, TokenStore};
pub use strategy::{OAuthParams, OAuthStrategy};
