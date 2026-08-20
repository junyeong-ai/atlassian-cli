//! A keychain entry is not data, and reading one may block.
//!
//! On Linux the Secret Service store answers `get_specifiers` over the session
//! bus, through a private tokio runtime whose `block_on` panics on a thread
//! that is already driving futures. `stored_profiles` therefore has to read
//! every entry inside the blocking closure rather than carrying entries back
//! out of it. `keyring_core::mock` cannot show that — its credentials are plain
//! fields — so this store blocks the way the real one does.
//!
//! Its own test binary: the store is process-global, and the unit tests share a
//! mock that this one would replace.

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use keyring_core::api::{CredentialApi, CredentialStoreApi};
use keyring_core::{Credential, Entry, Result};

const SERVICE: &str = "atlassian-cli";

#[derive(Debug)]
struct BlockingCred {
    user: String,
}

impl CredentialApi for BlockingCred {
    fn set_secret(&self, _: &[u8]) -> Result<()> {
        Ok(())
    }

    fn get_secret(&self) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }

    fn delete_credential(&self) -> Result<()> {
        Ok(())
    }

    fn get_credential(&self) -> Result<Option<Arc<Credential>>> {
        Ok(None)
    }

    /// What the Secret Service wrapper does: answer by way of a blocking call
    /// into another runtime.
    fn get_specifiers(&self) -> Option<(String, String)> {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("test runtime")
            .block_on(async {});
        Some((SERVICE.to_string(), self.user.clone()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct BlockingStore;

impl CredentialStoreApi for BlockingStore {
    fn vendor(&self) -> String {
        "blocking test store".to_string()
    }

    fn id(&self) -> String {
        "blocking test store".to_string()
    }

    fn build(&self, _: &str, user: &str, _: Option<&HashMap<&str, &str>>) -> Result<Entry> {
        Ok(Entry::new_with_credential(Arc::new(BlockingCred {
            user: user.to_string(),
        })))
    }

    fn search(&self, _: &HashMap<&str, &str>) -> Result<Vec<Entry>> {
        Ok(vec![Entry::new_with_credential(Arc::new(BlockingCred {
            user: "stored".to_string(),
        }))])
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn enumerating_never_reads_an_entry_on_the_reactor() {
    keyring_core::set_default_store(Arc::new(BlockingStore));
    let dir = tempfile::tempdir().unwrap();

    let stored = atlassian_cli::auth::stored_profiles(&dir.path().join("credentials.json")).await;

    assert!(stored.profiles.contains("stored"), "{:?}", stored.profiles);
}
