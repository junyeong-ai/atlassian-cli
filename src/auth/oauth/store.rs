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

/// What the keychain did, in the terms this tool reasons about.
///
/// `keyring_core::Error` describes what went wrong; the three places that
/// consult the keychain each ask a different question of it — "should I use the
/// file instead", "did clearing this entry work", "may I conclude nothing is
/// stored". Answering those from the foreign enum meant one shared reading, and
/// narrowing it for one caller kept changing what another was entitled to
/// assume. These variants are the answers instead, produced in one place, so a
/// caller matches its own question and the compiler enumerates the rest.
enum Keychain<T> {
    Done(T),
    /// The store holds nothing for this profile.
    Empty,
    /// Not consulted: `ATLASSIAN_NO_KEYCHAIN` forbids it. A session stored
    /// before the flag was set may still be in there.
    Forbidden,
    /// This build carries no credential store, so nothing was ever written to
    /// one here — the only outcome from which absence may be concluded.
    Absent,
    /// A store exists and did not answer. Says nothing about what it holds.
    Unreachable(String),
}

impl<T> std::fmt::Display for Keychain<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Keychain::Done(_) => f.write_str("done"),
            Keychain::Empty => f.write_str("no entry"),
            Keychain::Forbidden => f.write_str("ATLASSIAN_NO_KEYCHAIN is set"),
            Keychain::Absent => f.write_str("no credential store in this build"),
            Keychain::Unreachable(reason) => f.write_str(reason),
        }
    }
}

/// Run a keychain operation and classify what came back.
///
/// The one place `keyring_core::Error` is read. Operations that address a
/// single entry go through [`TokenStore::keyring_op`]; a search addresses the
/// service instead, so it comes here directly rather than through an entry it
/// has no profile for.
async fn keychain<T, F>(op: F) -> Keychain<T>
where
    F: FnOnce() -> std::result::Result<T, KeyringError> + Send + 'static,
    T: Send + 'static,
{
    if keychain_disabled() {
        return Keychain::Forbidden;
    }
    tokio::task::spawn_blocking(move || {
        if let Err(reason) = ensure_store_installed() {
            return if HAS_NATIVE_STORE {
                Keychain::Unreachable(reason)
            } else {
                Keychain::Absent
            };
        }
        match op() {
            Ok(value) => Keychain::Done(value),
            Err(KeyringError::NoEntry) => Keychain::Empty,
            Err(e) => Keychain::Unreachable(e.to_string()),
        }
    })
    .await
    .unwrap_or_else(|join_err| Keychain::Unreachable(join_err.to_string()))
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

    /// A store whose fallback file is named rather than derived, so a caller
    /// holding an installation's paths uses those instead of the machine's.
    pub fn at(profile: impl Into<String>, file_path: PathBuf) -> Self {
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
            Keychain::Done(()) => {
                let _ = self.file_delete();
                Ok(TokenStorageBackend::Keyring)
            }
            outcome => {
                tracing::debug!("Keyring save unavailable ({outcome}), using the file store");
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
            Keychain::Done(json) => {
                let on_disk: OnDisk = serde_json::from_str(&json)
                    .context("Corrupted token entry in keyring (re-run `auth login`)")?;
                Ok(Some(LoadedTokens {
                    tokens: on_disk.into(),
                    backend: TokenStorageBackend::Keyring,
                }))
            }
            outcome => {
                if !matches!(outcome, Keychain::Empty) {
                    tracing::debug!("Keyring read unavailable ({outcome}), trying the file");
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
    /// A keychain that holds nothing, or that this build has none of, leaves
    /// nothing to delete and is success. A keychain that exists and would not
    /// answer — locked, a prompt denied, a session bus out of reach — is a
    /// failure, and reporting it as success is how `auth logout` and
    /// `self uninstall` came to say a token was gone while it was still there.
    pub async fn delete(&self) -> Result<()> {
        let keyring = self.keyring_op(|e| e.delete_credential()).await;
        // The file is this tool's to remove whatever the keychain did. Skipping
        // it because the keychain failed would leave a token behind on exactly
        // the machines that keep tokens there.
        self.file_delete()?;

        match keyring {
            Keychain::Done(()) | Keychain::Empty | Keychain::Forbidden | Keychain::Absent => Ok(()),
            Keychain::Unreachable(reason) => Err(anyhow::anyhow!(
                "Failed to clear the keychain entry for '{}': {reason}",
                self.profile
            )),
        }
    }

    /// Run a keyring operation off the async runtime. Native stores expose
    /// a sync API; the Linux backend internally blocks on async I/O.
    /// Isolating each call on a blocking thread keeps the tokio reactor
    /// free to service the spawned futures.
    /// Run one operation against this profile's entry.
    async fn keyring_op<T, F>(&self, op: F) -> Keychain<T>
    where
        F: FnOnce(&Entry) -> std::result::Result<T, KeyringError> + Send + 'static,
        T: Send + 'static,
    {
        let profile = self.profile.clone();
        keychain(move || Entry::new(KEYRING_SERVICE, &profile).and_then(|entry| op(&entry))).await
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
        // A file that cannot be read still holds whatever is in it, so calling
        // that a cleared session is the same lie as swallowing a keychain that
        // refused.
        let mut all = self.file_read_all()?;
        if all.remove(&self.profile).is_some() {
            if all.is_empty() {
                fs::remove_file(&self.file_path).with_context(|| {
                    format!("Failed to remove credentials file {:?}", self.file_path)
                })?;
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
/// credentials are gone when they may not be — and only `Unsupported` lets a
/// caller conclude there was nothing there to begin with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyringEnumeration {
    /// The store listed its entries, so its side of the answer is complete.
    Listed,
    /// `ATLASSIAN_NO_KEYCHAIN` forbids touching the keychain at all.
    Skipped,
    /// This build carries no credential store, so nothing was ever written to
    /// one here.
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
    /// Why the fallback file could not be read, where it could not. Whatever it
    /// holds is still there and none of it is named in `profiles`.
    pub file_error: Option<String>,
}

/// Ask both backends which profiles they hold tokens for.
///
/// The file store is a profile-keyed map, so its keys are its whole answer when
/// it can be read at all. The keychain answers through a search whose spec each
/// store defines for itself, so what comes back is narrowed rather than
/// trusted: only entries that name this service are kept, and one that will not
/// name itself makes the listing incomplete. Both halves report how complete
/// they are beside the list rather than folded into it.
pub async fn stored_profiles(credentials_file: &std::path::Path) -> StoredProfiles {
    let mut profiles = BTreeSet::new();
    let file_error = match read_all_from(credentials_file) {
        Ok(all) => {
            profiles.extend(all.into_keys());
            None
        }
        Err(e) => Some(format!("{e:#}")),
    };

    let keyring = match keychain(|| {
        let (key, value) = search_spec();
        Entry::search(&HashMap::from([(key, value.as_str())]))
    })
    .await
    {
        Keychain::Done(entries) => {
            let mut named = BTreeSet::new();
            let mut unnamed = 0usize;
            for entry in &entries {
                match entry.get_specifiers() {
                    Some((service, user)) if service == KEYRING_SERVICE => {
                        named.insert(user);
                    }
                    // Another service's entry, which a store that searches by
                    // name rather than by attribute returns alongside ours.
                    Some(_) => {}
                    None => unnamed += 1,
                }
            }
            profiles.extend(named);
            if unnamed == 0 {
                KeyringEnumeration::Listed
            } else {
                KeyringEnumeration::Failed(format!(
                    "{unnamed} keychain entries would not say which profile they belong to"
                ))
            }
        }
        Keychain::Empty => KeyringEnumeration::Listed,
        Keychain::Forbidden => KeyringEnumeration::Skipped,
        Keychain::Absent => KeyringEnumeration::Unsupported,
        Keychain::Unreachable(reason) => KeyringEnumeration::Failed(reason),
    };

    StoredProfiles {
        profiles,
        keyring,
        file_error,
    }
}

/// Install the platform-native credential store as the keyring-core default,
/// unless an embedder or test has already configured one. Idempotent: the
/// first successful call wins; on failure, the same error surfaces on every
/// subsequent call so the file fallback engages deterministically.
fn ensure_store_installed() -> std::result::Result<(), String> {
    static INIT: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        if get_default_store().is_some() {
            return Ok(());
        }
        install_store().map_err(|e| e.to_string())
    })
    .clone()
}

/// Whether a credential store is compiled into this build. A fact of the
/// target, so where it is false nothing was ever written to a keychain here.
const HAS_NATIVE_STORE: bool = cfg!(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd"
));

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

/// How this tool's entries are addressed to the store's search.
///
/// Each store defines its own search vocabulary and rejects a key it does not
/// know, so the spec is declared here beside the stores rather than at the one
/// call site, where it would be one platform's spelling standing in for all of
/// them. It only narrows what comes back: `stored_profiles` keeps the entries
/// whose own specifiers name this service, so a store that matches more
/// loosely, or not at all, still yields the same answer.
#[cfg(target_os = "windows")]
fn search_spec() -> (&'static str, String) {
    // The Credential Manager has no attributes to match on, so the store
    // searches target names by regex and composes them as `{user}.{service}`.
    ("pattern", format!(r"\.{KEYRING_SERVICE}$"))
}

#[cfg(not(target_os = "windows"))]
fn search_spec() -> (&'static str, String) {
    ("service", KEYRING_SERVICE.to_string())
}

/// The name of the fallback token file inside the global config directory.
pub const CREDENTIALS_FILE: &str = "credentials.json";

/// Where the fallback token file lives, for a caller that reports on or removes
/// it rather than reading tokens out of it.
pub fn credentials_file() -> Option<PathBuf> {
    default_file_path().ok()
}

/// The fallback token file, beside the global config it belongs with.
fn default_file_path() -> Result<PathBuf> {
    let dir =
        crate::config::Config::global_config_dir().context("Failed to determine home directory")?;
    Ok(dir.join(CREDENTIALS_FILE))
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
        let store = TokenStore::at("default", path.clone());

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
        let store = TokenStore::at("default", path.clone());

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
        let store = TokenStore::at("refusing-keychain", path.clone());

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

    /// The Windows spec interpolates the service name into a regex, so the name
    /// has to mean itself there. A gate rather than a note: the spec is
    /// compiled out on every other target, and a metacharacter would silently
    /// widen what an uninstall thinks it has to clear.
    #[test]
    fn the_service_name_carries_no_regex_meaning() {
        assert!(
            KEYRING_SERVICE
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "{KEYRING_SERVICE} would not be a literal in the Windows search pattern"
        );
    }

    /// A store searches by its own spelling, so what comes back can be wider
    /// than what was asked for. The service each entry names is what decides.
    #[tokio::test]
    async fn an_entry_belonging_to_another_service_is_not_one_of_these_profiles() {
        set_default_store(keyring_core::mock::Store::new().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");

        TokenStore::at("ours", path.clone())
            .save(&fixture_tokens())
            .await
            .unwrap();
        Entry::new(&format!("{KEYRING_SERVICE}-extra"), "theirs")
            .unwrap()
            .set_password("x")
            .unwrap();

        let stored = stored_profiles(&path).await;
        assert_eq!(stored.keyring, KeyringEnumeration::Listed);
        assert!(stored.profiles.contains("ours"), "{:?}", stored.profiles);
        assert!(!stored.profiles.contains("theirs"), "{:?}", stored.profiles);
    }

    /// A file that will not parse still holds whatever is in it. Reporting no
    /// profiles is the answer that makes an uninstall believe there is nothing
    /// left to name.
    #[tokio::test]
    async fn a_credentials_file_that_cannot_be_read_says_so() {
        set_default_store(keyring_core::mock::Store::new().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("credentials.json");
        fs::write(&path, "{ not json").unwrap();

        let stored = stored_profiles(&path).await;
        assert!(
            stored
                .file_error
                .is_some_and(|reason| reason.contains("credentials")),
            "an unreadable file passed for an empty one"
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
        let store = TokenStore::at("keyring-roundtrip", path);

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
        let s1 = TokenStore::at("default", path.clone());
        let s2 = TokenStore::at("work", path.clone());

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
