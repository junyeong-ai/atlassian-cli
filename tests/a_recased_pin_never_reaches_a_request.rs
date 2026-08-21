//! `resume` is where a stored session becomes a live one, and the cloud id it
//! settles on goes straight into the `/ex/{service}/{cloud_id}` proxy path.
//! The gateway resolves that id byte-for-byte — the correct id in the wrong
//! case is 404, not 403 — so a pin differing from the stored one only in case
//! buys a session whose every request misses. Exercising the rule through
//! `resolve_stored_cloud_id` proves the rule; only driving `resume` proves
//! `resume` applies it.
//!
//! Its own test binary: `keyring_core` takes the default store as
//! process-global startup state, and the unit tests share a mock this one
//! would replace.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use atlassian_cli::auth::{OAuthParams, OAuthStrategy};
use keyring_core::api::{CredentialApi, CredentialStoreApi};
use keyring_core::{Credential, Entry, Result};

const STORED_CLOUD_ID: &str = "00e6196b-8845-46cb-bb2b-85ed696dafcd";

/// A stored session, as `load` reads one: the keychain answers first, so the
/// file store is never reached and nothing on this machine is written.
#[derive(Debug)]
struct StoredSession;

impl CredentialApi for StoredSession {
    fn set_secret(&self, _: &[u8]) -> Result<()> {
        Ok(())
    }

    fn get_secret(&self) -> Result<Vec<u8>> {
        Ok(format!(
            r#"{{"access_token":"access","refresh_token":"refresh",
               "expires_at_unix":4102444800,"scopes":["read:jira-work"],
               "cloud_id":"{STORED_CLOUD_ID}"}}"#
        )
        .into_bytes())
    }

    fn delete_credential(&self) -> Result<()> {
        Ok(())
    }

    fn get_credential(&self) -> Result<Option<Arc<Credential>>> {
        Ok(None)
    }

    /// Nothing here enumerates; `resume` reads one entry by name.
    fn get_specifiers(&self) -> Option<(String, String)> {
        None
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct StoredSessionStore;

impl CredentialStoreApi for StoredSessionStore {
    fn vendor(&self) -> String {
        "stored session test store".to_string()
    }

    fn id(&self) -> String {
        "stored session test store".to_string()
    }

    fn build(&self, _: &str, _: &str, _: Option<&HashMap<&str, &str>>) -> Result<Entry> {
        Ok(Entry::new_with_credential(Arc::new(StoredSession)))
    }

    fn search(&self, _: &HashMap<&str, &str>) -> Result<Vec<Entry>> {
        Ok(Vec::new())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn params(cloud_id: Option<&str>) -> OAuthParams {
    OAuthParams {
        client_id: "cid".into(),
        client_secret: "secret".into(),
        redirect_port: 8976,
        scopes: vec!["read:jira-work".into()],
        cloud_id: cloud_id.map(str::to_string),
    }
}

/// Not `#[tokio::test]`: the environment is settled before a runtime exists,
/// which is the only point at which writing it is sound.
///
/// `ATLASSIAN_NO_KEYCHAIN` in the inherited environment would send `load` past
/// the mock to the file store, and the file store's path is the one this
/// machine's sessions are kept at — a test that reads it is reading the user's
/// credentials to decide its own result. Cleared here, and the home the path is
/// built from is a temporary directory, so the fallback has nothing of anyone's
/// to find either way.
#[test]
fn resume_refuses_a_pin_that_differs_from_the_stored_id_only_in_case() {
    let home = tempfile::tempdir().expect("temporary home");
    unsafe {
        std::env::remove_var("ATLASSIAN_NO_KEYCHAIN");
        std::env::set_var("HOME", home.path());
        std::env::set_var("USERPROFILE", home.path());
    }
    keyring_core::set_default_store(Arc::new(StoredSessionStore));

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("test runtime")
        .block_on(async {
            let err =
                OAuthStrategy::resume(params(Some(&STORED_CLOUD_ID.to_uppercase())), "recased")
                    .await
                    .unwrap_err()
                    .to_string();
            assert!(err.contains("only in case"), "{err}");
            assert!(err.contains(STORED_CLOUD_ID), "{err}");

            // The pin the login stored, and no pin at all, both resume.
            OAuthStrategy::resume(params(Some(STORED_CLOUD_ID)), "pinned")
                .await
                .expect("the stored id, pinned as it was stored");
            OAuthStrategy::resume(params(None), "unpinned")
                .await
                .expect("no pin leaves the stored id to stand");
        });
}
