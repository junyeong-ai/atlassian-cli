//! Persistent storage for OAuth 3LO tokens.
//!
//! Strategy: prefer OS keychain (macOS Keychain / Linux Secret Service /
//! Windows Credential Manager) via `keyring-core`. Fall back to a 0600-mode
//! JSON file at `~/.config/atlassian-cli/credentials.json` for environments
//! without a working keychain (CI, headless servers).
//!
//! On every `save` we clear the same key from the other backend so reads
//! are unambiguous.

use anyhow::{Context, Result};
use keyring_core::{Entry, Error as KeyringError, get_default_store, set_default_store};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const KEYRING_SERVICE: &str = "atlassian-cli";

/// `ATLASSIAN_NO_KEYCHAIN` bypasses the OS keychain entirely so token storage
/// uses the 0600 file. On a desktop OS the keychain prompts with a GUI dialog
/// that blocks indefinitely in a headless or AI-agent session; this explicit
/// opt-out (no heuristic auto-detection) lets those callers skip it. Blank /
/// `0` / `false` count as unset, consistent with the project's blank-value
/// policy for the rest of the env surface.
fn keychain_disabled() -> bool {
    keychain_disabled_from(std::env::var("ATLASSIAN_NO_KEYCHAIN").ok().as_deref())
}

/// Pure parsing of the opt-out value, split out so it is testable without
/// mutating process-global env (which would race the parallel test runner).
fn keychain_disabled_from(value: Option<&str>) -> bool {
    value
        .map(|v| {
            let v = v.trim();
            !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false")
        })
        .unwrap_or(false)
}

/// Where the persisted tokens for the active profile currently live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStorageBackend {
    Keyring,
    File,
}

impl std::fmt::Display for TokenStorageBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenStorageBackend::Keyring => f.write_str("OS keychain"),
            TokenStorageBackend::File => f.write_str("file"),
        }
    }
}

/// Result of a successful `TokenStore::load`. Carries both the credential
/// material and the backend it was read from so callers don't have to
/// re-query storage to display provenance.
#[derive(Debug, Clone)]
pub struct LoadedTokens {
    pub tokens: TokenSet,
    pub backend: TokenStorageBackend,
}

/// Tokens held in memory. Secrets wrapped in `SecretString` to prevent
/// accidental leaks via `Debug`/`Display`.
#[derive(Clone)]
pub struct TokenSet {
    pub access_token: SecretString,
    pub refresh_token: Option<SecretString>,
    /// Absolute expiry as Unix seconds. Use `is_expired_with_buffer` for checks.
    pub expires_at_unix: i64,
    pub scopes: Vec<String>,
    pub cloud_id: Option<String>,
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &"<redacted>")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "<redacted>"),
            )
            .field("expires_at_unix", &self.expires_at_unix)
            .field("scopes", &self.scopes)
            .field("cloud_id", &self.cloud_id)
            .finish()
    }
}

impl TokenSet {
    /// Returns true if `expires_at` is within `buffer_secs` of now.
    /// Defensive: tokens within the buffer should be refreshed proactively
    /// rather than failing mid-pagination.
    pub fn is_expired_with_buffer(&self, buffer_secs: i64) -> bool {
        let now = now_unix();
        self.expires_at_unix.saturating_sub(buffer_secs) <= now
    }

    /// Seconds until the token's official expiry (negative if already past).
    pub fn seconds_until_expiry(&self) -> i64 {
        self.expires_at_unix - now_unix()
    }
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Serialize, Deserialize)]
struct OnDisk {
    access_token: String,
    refresh_token: Option<String>,
    expires_at_unix: i64,
    scopes: Vec<String>,
    cloud_id: Option<String>,
}

impl From<&TokenSet> for OnDisk {
    fn from(t: &TokenSet) -> Self {
        Self {
            access_token: t.access_token.expose_secret().to_string(),
            refresh_token: t
                .refresh_token
                .as_ref()
                .map(|s| s.expose_secret().to_string()),
            expires_at_unix: t.expires_at_unix,
            scopes: t.scopes.clone(),
            cloud_id: t.cloud_id.clone(),
        }
    }
}

impl From<OnDisk> for TokenSet {
    fn from(d: OnDisk) -> Self {
        Self {
            access_token: SecretString::new(d.access_token.into()),
            refresh_token: d.refresh_token.map(|s| SecretString::new(s.into())),
            expires_at_unix: d.expires_at_unix,
            scopes: d.scopes,
            cloud_id: d.cloud_id,
        }
    }
}

/// Per-profile token store. Construction does NOT touch the backend —
/// I/O happens on `save` / `load` / `delete`.
#[derive(Debug)]
pub struct TokenStore {
    profile: String,
    file_path: PathBuf,
}

impl TokenStore {
    pub fn new(profile: impl Into<String>) -> Result<Self> {
        Ok(Self {
            profile: profile.into(),
            file_path: default_file_path()?,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_paths(profile: impl Into<String>, file_path: PathBuf) -> Self {
        Self {
            profile: profile.into(),
            file_path,
        }
    }

    /// Save tokens. Tries keyring first; on any error falls back to the
    /// 0600 file. Always clears the unused backend so reads are unambiguous.
    pub async fn save(&self, tokens: &TokenSet) -> Result<TokenStorageBackend> {
        let on_disk = OnDisk::from(tokens);
        let json = serde_json::to_string(&on_disk).context("Failed to serialize tokens")?;

        let keyring_json = json.clone();
        match self
            .keyring_op(move |e| e.set_password(&keyring_json))
            .await
        {
            Ok(()) => {
                let _ = self.file_delete();
                Ok(TokenStorageBackend::Keyring)
            }
            Err(e) => {
                tracing::debug!("Keyring save failed, falling back to file: {}", e);
                self.file_save(&json)?;
                let _ = self.keyring_op(|e| e.delete_credential()).await;
                Ok(TokenStorageBackend::File)
            }
        }
    }

    /// Load tokens. Checks keyring first, then file. Returns the loaded
    /// tokens tagged with the backend they came from, or `Ok(None)` if not
    /// present in either backend.
    pub async fn load(&self) -> Result<Option<LoadedTokens>> {
        match self.keyring_op(|e| e.get_password()).await {
            Ok(json) => {
                let on_disk: OnDisk = serde_json::from_str(&json)
                    .context("Corrupted token entry in keyring (re-run `auth login`)")?;
                Ok(Some(LoadedTokens {
                    tokens: on_disk.into(),
                    backend: TokenStorageBackend::Keyring,
                }))
            }
            Err(e) => {
                if !matches!(e, KeyringError::NoEntry) {
                    tracing::debug!("Keyring read failed, trying file: {}", e);
                }
                Ok(self.file_load()?.map(|tokens| LoadedTokens {
                    tokens,
                    backend: TokenStorageBackend::File,
                }))
            }
        }
    }

    /// Delete this profile's tokens from both backends.
    ///
    /// A keychain that holds nothing, or that is not in play on this platform
    /// at all, leaves nothing to delete and is success. A keychain that exists
    /// and refused — locked, or a prompt the user denied — is a failure, and
    /// reporting it as success is how `auth logout` and `self uninstall` came
    /// to say a token was gone while it was still there.
    pub async fn delete(&self) -> Result<()> {
        let keyring = self.keyring_op(|e| e.delete_credential()).await;
        // The file is this tool's to remove whatever the keychain did. Skipping
        // it because the keychain failed would leave a token behind on exactly
        // the machines that keep tokens there.
        self.file_delete()?;

        match keyring {
            Ok(())
            | Err(KeyringError::NoEntry)
            | Err(KeyringError::NoDefaultStore)
            | Err(KeyringError::NotSupportedByStore(_)) => Ok(()),
            Err(e) => Err(anyhow::anyhow!(
                "Failed to clear the keychain entry for '{}': {e}",
                self.profile
            )),
        }
    }

    /// Run a keyring operation off the async runtime. Native stores expose
    /// a sync API; the Linux backend internally blocks on async I/O.
    /// Isolating each call on a blocking thread keeps the tokio reactor
    /// free to service the spawned futures.
    async fn keyring_op<T, F>(&self, op: F) -> std::result::Result<T, KeyringError>
    where
        F: FnOnce(&Entry) -> std::result::Result<T, KeyringError> + Send + 'static,
        T: Send + 'static,
    {
        // Explicit opt-out: skip the keychain so save/load/delete fall through
        // to the file store via their existing `NoEntry` handling — no GUI
        // prompt, no blocking, in headless / AI-agent sessions.
        if keychain_disabled() {
            return Err(KeyringError::NoEntry);
        }

        let profile = self.profile.clone();
        tokio::task::spawn_blocking(move || {
            ensure_store_installed()?;
            let entry = Entry::new(KEYRING_SERVICE, &profile)?;
            op(&entry)
        })
        .await
        .unwrap_or_else(|join_err| Err(KeyringError::PlatformFailure(Box::new(join_err))))
    }

    fn file_save(&self, json_for_profile: &str) -> Result<()> {
        let parent = self
            .file_path
            .parent()
            .context("credentials file path has no parent")?;
        fs::create_dir_all(parent).context("Failed to create credentials directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }

        let mut all = self.file_read_all().unwrap_or_default();
        let parsed: OnDisk = serde_json::from_str(json_for_profile)
            .context("Internal: failed to round-trip on-disk token JSON")?;
        all.insert(self.profile.clone(), parsed);

        let mut tmp = tempfile::NamedTempFile::new_in(parent)
            .context("Failed to create credentials tmpfile")?;
        let buf = serde_json::to_vec_pretty(&all).context("Failed to serialize credentials")?;
        tmp.write_all(&buf)
            .context("Failed to write credentials tmpfile")?;
        tmp.as_file().sync_all().ok();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = tmp
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600));
        }

        tmp.persist(&self.file_path)
            .context("Failed to atomically replace credentials file")?;
        Ok(())
    }

    fn file_load(&self) -> Result<Option<TokenSet>> {
        let all = self.file_read_all()?;
        Ok(all.into_iter().find_map(|(p, d)| {
            if p == self.profile {
                Some(d.into())
            } else {
                None
            }
        }))
    }

    fn file_delete(&self) -> Result<()> {
        let mut all = match self.file_read_all() {
            Ok(a) => a,
            Err(_) => return Ok(()),
        };
        if all.remove(&self.profile).is_some() {
            if all.is_empty() {
                let _ = fs::remove_file(&self.file_path);
            } else {
                let parent = self
                    .file_path
                    .parent()
                    .context("credentials file path has no parent")?;
                let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
                let buf = serde_json::to_vec_pretty(&all)?;
                tmp.write_all(&buf)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = tmp
                        .as_file()
                        .set_permissions(fs::Permissions::from_mode(0o600));
                }
                tmp.persist(&self.file_path)?;
            }
        }
        Ok(())
    }

    fn file_read_all(&self) -> Result<HashMap<String, OnDisk>> {
        read_all_from(&self.file_path)
    }
}

fn read_all_from(file_path: &std::path::Path) -> Result<HashMap<String, OnDisk>> {
    if !file_path.exists() {
        return Ok(HashMap::new());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(file_path) {
            let mode = meta.permissions().mode();
            if mode & 0o077 != 0 {
                tracing::warn!(
                    "Credentials file {:?} has too-permissive mode {:o}; recommend chmod 600",
                    file_path,
                    mode
                );
            }
        }
    }
    let raw = fs::read_to_string(file_path)
        .with_context(|| format!("Failed to read credentials file {file_path:?}"))?;
    serde_json::from_str(&raw)
        .with_context(|| format!("Failed to parse credentials file {file_path:?}"))
}

/// How completely the keychain could say which profiles it holds tokens for.
///
/// Carried rather than collapsed into the list, because "these are the profiles
/// with tokens" is a claim only one of these variants supports. A caller that
/// reports the list as exhaustive on any other is telling the user their
/// credentials are gone when they may not be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyringEnumeration {
    /// The store listed its entries, so its side of the answer is complete.
    Listed,
    /// `ATLASSIAN_NO_KEYCHAIN` forbids touching the keychain at all.
    Skipped,
    /// This platform's store provides no search, so anything it holds for a
    /// profile nobody names is unreachable from here.
    Unsupported,
    Failed(String),
}

impl KeyringEnumeration {
    /// Why the keychain could not be listed, where it said.
    pub fn reason(&self) -> Option<&str> {
        match self {
            KeyringEnumeration::Failed(reason) => Some(reason),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            KeyringEnumeration::Listed => "listed",
            KeyringEnumeration::Skipped => "skipped",
            KeyringEnumeration::Unsupported => "unsupported",
            KeyringEnumeration::Failed(_) => "failed",
        }
    }
}

/// The profiles this machine holds OAuth tokens for.
#[derive(Debug)]
pub struct StoredProfiles {
    pub profiles: BTreeSet<String>,
    pub keyring: KeyringEnumeration,
}

/// Ask both backends which profiles they hold tokens for.
///
/// The file store is a profile-keyed map, so its keys are its whole answer. The
/// keychain answers completely only where it implements search, which is why
/// the outcome comes back beside the list instead of folded into it.
pub async fn stored_profiles() -> StoredProfiles {
    let mut profiles = BTreeSet::new();
    if let Ok(path) = default_file_path()
        && let Ok(all) = read_all_from(&path)
    {
        profiles.extend(all.into_keys());
    }

    if keychain_disabled() {
        return StoredProfiles {
            profiles,
            keyring: KeyringEnumeration::Skipped,
        };
    }

    let found = tokio::task::spawn_blocking(|| {
        ensure_store_installed()?;
        Entry::search(&HashMap::from([("service", KEYRING_SERVICE)]))
    })
    .await
    .unwrap_or_else(|join_err| Err(KeyringError::PlatformFailure(Box::new(join_err))));

    let keyring = match found {
        Ok(entries) => {
            profiles.extend(
                entries
                    .iter()
                    .filter_map(|entry| entry.get_specifiers().map(|(_, user)| user)),
            );
            KeyringEnumeration::Listed
        }
        Err(KeyringError::NotSupportedByStore(_)) => KeyringEnumeration::Unsupported,
        Err(e) => KeyringEnumeration::Failed(e.to_string()),
    };

    StoredProfiles { profiles, keyring }
}

/// Install the platform-native credential store as the keyring-core default,
/// unless an embedder or test has already configured one. Idempotent: the
/// first successful call wins; on failure, the same error surfaces on every
/// subsequent call so the file fallback engages deterministically.
fn ensure_store_installed() -> std::result::Result<(), KeyringError> {
    static INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        if get_default_store().is_some() {
            return Ok(());
        }
        install_store().map_err(|e| e.to_string())
    })
    .as_ref()
    .map(|_| ())
    // `NotSupportedByStore`, whatever the underlying reason: a store that could
    // not be installed is one this binary never wrote a token to, so callers
    // that distinguish "the keychain refused" from "there is no keychain here"
    // get the second answer. The reason travels in the message. Caching forces
    // the round trip through a string — `KeyringError` is not `Clone` — so the
    // variant has to be chosen here rather than carried.
    .map_err(|msg| KeyringError::NotSupportedByStore(msg.clone()))
}

#[cfg(target_os = "macos")]
fn install_store() -> std::result::Result<(), KeyringError> {
    set_default_store(apple_native_keyring_store::keychain::Store::new()?);
    Ok(())
}

#[cfg(target_os = "windows")]
fn install_store() -> std::result::Result<(), KeyringError> {
    set_default_store(windows_native_keyring_store::Store::new()?);
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn install_store() -> std::result::Result<(), KeyringError> {
    set_default_store(zbus_secret_service_keyring_store::Store::new()?);
    Ok(())
}

#[cfg(not(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd"
)))]
fn install_store() -> std::result::Result<(), KeyringError> {
    Err(KeyringError::NotSupportedByStore(
        "no native keyring store on this platform".into(),
    ))
}

/// The fallback token file, beside the global config it belongs with.
fn default_file_path() -> Result<PathBuf> {
    let dir =
        crate::config::Config::global_config_dir().context("Failed to determine home directory")?;
    Ok(dir.join("credentials.json"))
}

/// Where the fallback token file lives, for a caller that reports on or removes
/// it rather than reading tokens out of it.
pub fn credentials_file() -> Option<PathBuf> {
    default_file_path().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_tokens() -> TokenSet {
        TokenSet {
            access_token: SecretString::new("access-abc".into()),
            refresh_token: Some(SecretString::new("refresh-xyz".into())),
            expires_at_unix: 1_900_000_000,
            scopes: vec!["read:jira-work".into(), "offline_access".into()],
            cloud_id: Some("cloud-1".into()),
        }
    }

    #[test]
    fn debug_redacts_secrets() {
        let t = fixture_tokens();
        let dbg = format!("{:?}", t);
        assert!(!dbg.contains("access-abc"));
        assert!(!dbg.contains("refresh-xyz"));
        assert!(dbg.contains("<redacted>"));
    }

    #[test]
    fn roundtrip_via_on_disk() {
        let t = fixture_tokens();
        let on_disk = OnDisk::from(&t);
        let back: TokenSet = on_disk.into();
        assert_eq!(back.access_token.expose_secret(), "access-abc");
        assert_eq!(back.refresh_token.unwrap().expose_secret(), "refresh-xyz");
        assert_eq!(back.expires_at_unix, 1_900_000_000);
        assert_eq!(back.scopes, vec!["read:jira-work", "offline_access"]);
        assert_eq!(back.cloud_id.as_deref(), Some("cloud-1"));
    }

    #[test]
    fn expiry_detection_with_buffer() {
        let now = now_unix();
        let mut t = fixture_tokens();
        t.expires_at_unix = now + 600;
        assert!(!t.is_expired_with_buffer(300));
        t.expires_at_unix = now + 100;
        assert!(t.is_expired_with_buffer(300));
        t.expires_at_unix = now - 1;
        assert!(t.is_expired_with_buffer(0));
    }

    #[test]
    fn seconds_until_expiry_matches_clock() {
        let now = now_unix();
        let mut t = fixture_tokens();
        t.expires_at_unix = now + 500;
        let delta = t.seconds_until_expiry();
        assert!((498..=500).contains(&delta), "got {}", delta);
    }

    #[test]
    fn backend_display_is_human() {
        assert_eq!(format!("{}", TokenStorageBackend::Keyring), "OS keychain");
        assert_eq!(format!("{}", TokenStorageBackend::File), "file");
    }

    #[test]
    fn keychain_opt_out_parsing() {
        // Truthy → disabled
        assert!(keychain_disabled_from(Some("1")));
        assert!(keychain_disabled_from(Some("true")));
        assert!(keychain_disabled_from(Some("TRUE")));
        assert!(keychain_disabled_from(Some("yes")));
        // Falsy / unset → keychain stays on (blank-value policy)
        assert!(!keychain_disabled_from(None));
        assert!(!keychain_disabled_from(Some("")));
        assert!(!keychain_disabled_from(Some("   ")));
        assert!(!keychain_disabled_from(Some("0")));
        assert!(!keychain_disabled_from(Some("false")));
        assert!(!keychain_disabled_from(Some("False")));
    }

    #[test]
    fn file_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        let store = TokenStore::with_paths("default", path.clone());

        let on_disk = OnDisk::from(&fixture_tokens());
        let json = serde_json::to_string(&on_disk).unwrap();
        store.file_save(&json).unwrap();

        let loaded = store.file_load().unwrap().unwrap();
        assert_eq!(loaded.access_token.expose_secret(), "access-abc");
        assert_eq!(loaded.cloud_id.as_deref(), Some("cloud-1"));
        assert!(path.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn file_delete_clears_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        let store = TokenStore::with_paths("default", path.clone());

        let json = serde_json::to_string(&OnDisk::from(&fixture_tokens())).unwrap();
        store.file_save(&json).unwrap();
        assert!(store.file_load().unwrap().is_some());

        store.file_delete().unwrap();
        assert!(store.file_load().unwrap().is_none());
        assert!(!path.exists());
    }

    /// A keychain that refuses must not take the file store down with it: the
    /// machines that keep tokens in the file are exactly the ones where the
    /// keychain is unreachable, and a logout there was leaving the token behind.
    #[tokio::test]
    async fn a_refusing_keychain_still_leaves_the_file_cleared() {
        set_default_store(keyring_core::mock::Store::new().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        let store = TokenStore::with_paths("refusing-keychain", path.clone());

        store
            .file_save(&serde_json::to_string(&OnDisk::from(&fixture_tokens())).unwrap())
            .unwrap();
        assert!(path.exists());

        let entry = Entry::new(KEYRING_SERVICE, "refusing-keychain").unwrap();
        entry
            .as_any()
            .downcast_ref::<keyring_core::mock::Cred>()
            .expect("the mock store hands out mock credentials")
            .set_error(KeyringError::NoStorageAccess(Box::new(
                std::io::Error::other("the keychain is locked"),
            )));

        let err = store.delete().await.unwrap_err().to_string();
        assert!(err.contains("Failed to clear the keychain entry"), "{err}");
        assert!(
            !path.exists(),
            "the file token survived a failed keychain delete"
        );
    }

    /// End-to-end exercise of the keyring path via `keyring_core::mock`.
    /// Pre-installing the mock also verifies that `ensure_store_installed`
    /// honors a store already configured by an embedder/test.
    #[tokio::test]
    async fn keyring_path_roundtrip_via_mock() {
        set_default_store(keyring_core::mock::Store::new().unwrap());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        let store = TokenStore::with_paths("keyring-roundtrip", path);

        let backend = store.save(&fixture_tokens()).await.unwrap();
        assert_eq!(backend, TokenStorageBackend::Keyring);

        let loaded = store
            .load()
            .await
            .unwrap()
            .expect("tokens must be present after save");
        assert_eq!(loaded.backend, TokenStorageBackend::Keyring);
        assert_eq!(loaded.tokens.access_token.expose_secret(), "access-abc");
        assert_eq!(loaded.tokens.cloud_id.as_deref(), Some("cloud-1"));

        store.delete().await.unwrap();
        assert!(store.load().await.unwrap().is_none());
    }

    #[test]
    fn file_multi_profile_isolation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        let s1 = TokenStore::with_paths("default", path.clone());
        let s2 = TokenStore::with_paths("work", path.clone());

        let t1 = fixture_tokens();
        let mut t2 = fixture_tokens();
        t2.cloud_id = Some("cloud-work".into());

        s1.file_save(&serde_json::to_string(&OnDisk::from(&t1)).unwrap())
            .unwrap();
        s2.file_save(&serde_json::to_string(&OnDisk::from(&t2)).unwrap())
            .unwrap();

        assert_eq!(
            s1.file_load().unwrap().unwrap().cloud_id.as_deref(),
            Some("cloud-1")
        );
        assert_eq!(
            s2.file_load().unwrap().unwrap().cloud_id.as_deref(),
            Some("cloud-work")
        );

        s1.file_delete().unwrap();
        assert!(s1.file_load().unwrap().is_none());
        assert!(
            s2.file_load().unwrap().is_some(),
            "work profile must survive default delete"
        );
    }
}
