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
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

const KEYRING_SERVICE: &str = "atlassian-cli";

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

    /// Delete tokens from both backends. Best-effort cleanup; never errors
    /// on missing entries.
    pub async fn delete(&self) -> Result<()> {
        match self.keyring_op(|e| e.delete_credential()).await {
            Ok(()) | Err(KeyringError::NoEntry) => {}
            Err(e) => tracing::debug!("Keyring delete returned: {}", e),
        }
        let _ = self.file_delete();
        Ok(())
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
        if !self.file_path.exists() {
            return Ok(HashMap::new());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::metadata(&self.file_path) {
                let mode = meta.permissions().mode();
                if mode & 0o077 != 0 {
                    tracing::warn!(
                        "Credentials file {:?} has too-permissive mode {:o}; recommend chmod 600",
                        self.file_path,
                        mode
                    );
                }
            }
        }
        let raw = fs::read_to_string(&self.file_path)
            .with_context(|| format!("Failed to read credentials file {:?}", self.file_path))?;
        let parsed: HashMap<String, OnDisk> = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse credentials file {:?}", self.file_path))?;
        Ok(parsed)
    }
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
    .map_err(|msg| KeyringError::PlatformFailure(Box::new(CachedInstallError(msg.clone()))))
}

#[derive(Debug)]
struct CachedInstallError(String);

impl std::fmt::Display for CachedInstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CachedInstallError {}

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

fn default_file_path() -> Result<PathBuf> {
    let mut p = dirs::home_dir().context("Failed to determine home directory")?;
    p.push(".config");
    p.push("atlassian-cli");
    p.push("credentials.json");
    Ok(p)
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
