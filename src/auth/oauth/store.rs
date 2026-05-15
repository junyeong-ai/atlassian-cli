//! Persistent storage for OAuth 3LO tokens.
//!
//! Strategy: prefer OS keychain (macOS Keychain / Linux Secret Service /
//! Windows Credential Manager) via the `keyring` crate. Fall back to a
//! 0600-mode JSON file at `~/.config/atlassian-cli/credentials.json` for
//! environments without a working keychain (CI, headless servers).
//!
//! On every `save` we clear the same key from the other backend so reads
//! are unambiguous.

use anyhow::{Context, Result};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
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
    pub fn save(&self, tokens: &TokenSet) -> Result<TokenStorageBackend> {
        let on_disk = OnDisk::from(tokens);
        let json = serde_json::to_string(&on_disk).context("Failed to serialize tokens")?;

        match self.keyring_entry().and_then(|e| e.set_password(&json)) {
            Ok(()) => {
                let _ = self.file_delete();
                Ok(TokenStorageBackend::Keyring)
            }
            Err(e) => {
                tracing::debug!("Keyring save failed, falling back to file: {}", e);
                self.file_save(&json)?;
                let _ = self.keyring_entry().and_then(|e| e.delete_credential());
                Ok(TokenStorageBackend::File)
            }
        }
    }

    /// Load tokens. Checks keyring first, then file. `Ok(None)` if not present
    /// in either backend.
    pub fn load(&self) -> Result<Option<TokenSet>> {
        if let Ok(entry) = self.keyring_entry() {
            match entry.get_password() {
                Ok(json) => {
                    let on_disk: OnDisk = serde_json::from_str(&json)
                        .context("Corrupted token entry in keyring (re-run `auth login`)")?;
                    return Ok(Some(on_disk.into()));
                }
                Err(keyring::Error::NoEntry) => {}
                Err(e) => tracing::debug!("Keyring read failed, trying file: {}", e),
            }
        }
        self.file_load()
    }

    /// Delete tokens from both backends. Best-effort cleanup; never errors
    /// on missing entries.
    pub fn delete(&self) -> Result<()> {
        if let Ok(entry) = self.keyring_entry() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => {}
                Err(e) => tracing::debug!("Keyring delete returned: {}", e),
            }
        }
        let _ = self.file_delete();
        Ok(())
    }

    /// Which backend currently holds tokens for this profile. Used by
    /// `auth status` for diagnostics.
    pub fn detect_backend(&self) -> Option<TokenStorageBackend> {
        match self.keyring_entry().and_then(|e| e.get_password()) {
            Ok(_) => Some(TokenStorageBackend::Keyring),
            Err(keyring::Error::NoEntry) => {
                if self.file_path.exists()
                    && self
                        .file_read_all()
                        .ok()
                        .is_some_and(|all| all.contains_key(&self.profile))
                {
                    Some(TokenStorageBackend::File)
                } else {
                    None
                }
            }
            Err(_) => {
                if self.file_path.exists()
                    && self
                        .file_read_all()
                        .ok()
                        .is_some_and(|all| all.contains_key(&self.profile))
                {
                    Some(TokenStorageBackend::File)
                } else {
                    None
                }
            }
        }
    }

    fn keyring_entry(&self) -> std::result::Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(KEYRING_SERVICE, &self.profile)
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
