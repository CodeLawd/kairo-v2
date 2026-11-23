//! Electron-compatible credential and licensed scripture encryption. Keychain
//! calls are synchronous and can prompt for access. Use a background thread.

mod electron;

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, OsRng, rand_core::RngCore},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use zeroize::Zeroizing;

const KEY_BYTES: usize = 32;
const IV_BYTES: usize = 12;
const TAG_BYTES: usize = 16;
const CACHE_DB: &str = "api-bible-cache.db";

// The single-instance guard excludes other native processes from this profile.
// Serialize key creation and invalidation across services within the process.
static KEY_ACCESS: Mutex<()> = Mutex::new(());

pub const CREDENTIALS_NOT_SAVED_NOTICE: &str =
    "This computer has no keychain, so you’ll need to sign in again next launch.";

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("OS keychain is unavailable")]
    Keychain,
    #[error("encrypted data key is invalid")]
    InvalidKey,
    #[error("encrypted payload is malformed or failed authentication")]
    InvalidPayload,
    #[error("OS random number generation failed")]
    Random,
    #[error("secret storage filesystem operation failed")]
    Io(#[from] io::Error),
}

/// Stores a 32-byte data key in OS-protected storage, keyed by a profile slot.
pub trait KeyProtector: Send + Sync {
    fn load(&self, slot: &str) -> Result<Option<[u8; KEY_BYTES]>, SecretError>;
    fn save(&self, slot: &str, key: &[u8; KEY_BYTES]) -> Result<(), SecretError>;
}

#[derive(Default)]
pub struct KeyringProtector;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl KeyringProtector {
    fn entry(slot: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new("Kairo Native Storage", slot).map_err(|_| SecretError::Keychain)
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
impl KeyProtector for KeyringProtector {
    fn load(&self, slot: &str) -> Result<Option<[u8; KEY_BYTES]>, SecretError> {
        match Self::entry(slot)?.get_secret() {
            Ok(key) => Zeroizing::new(key)
                .as_slice()
                .try_into()
                .map(Some)
                .map_err(|_| SecretError::InvalidKey),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => {
                tracing::warn!(slot, %error, "[Secrets] keychain load failed");
                Err(SecretError::Keychain)
            }
        }
    }

    fn save(&self, slot: &str, key: &[u8; KEY_BYTES]) -> Result<(), SecretError> {
        Self::entry(slot)?.set_secret(key).map_err(|error| {
            tracing::warn!(slot, %error, "[Secrets] keychain save failed");
            SecretError::Keychain
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
impl KeyProtector for KeyringProtector {
    fn load(&self, _: &str) -> Result<Option<[u8; KEY_BYTES]>, SecretError> {
        Err(SecretError::Keychain)
    }

    fn save(&self, _: &str, _: &[u8; KEY_BYTES]) -> Result<(), SecretError> {
        Err(SecretError::Keychain)
    }
}

/// JSON shape used for both `cloud/*.enc` and the API.Bible cache.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EncryptedValue {
    pub ciphertext: String,
    pub iv: String,
    pub auth_tag: String,
}

struct DataKey(Zeroizing<[u8; KEY_BYTES]>);

impl DataKey {
    fn generate() -> Result<Self, SecretError> {
        let mut key = Zeroizing::new([0; KEY_BYTES]);
        OsRng
            .try_fill_bytes(&mut *key)
            .map_err(|_| SecretError::Random)?;
        Ok(Self(key))
    }

    fn encrypt(&self, plaintext: &str) -> Result<EncryptedValue, SecretError> {
        let mut iv = [0; IV_BYTES];
        OsRng
            .try_fill_bytes(&mut iv)
            .map_err(|_| SecretError::Random)?;
        let cipher = Aes256Gcm::new_from_slice(&*self.0).map_err(|_| SecretError::InvalidKey)?;
        let bytes = cipher
            .encrypt(Nonce::from_slice(&iv), plaintext.as_bytes())
            .map_err(|_| SecretError::InvalidPayload)?;
        let (ciphertext, tag) = bytes.split_at(bytes.len() - TAG_BYTES);
        Ok(EncryptedValue {
            ciphertext: STANDARD.encode(ciphertext),
            iv: STANDARD.encode(iv),
            auth_tag: STANDARD.encode(tag),
        })
    }

    fn decrypt(&self, blob: &EncryptedValue) -> Result<String, SecretError> {
        let iv = decode(&blob.iv)?;
        let tag = decode(&blob.auth_tag)?;
        if iv.len() != IV_BYTES || tag.len() != TAG_BYTES {
            return Err(SecretError::InvalidPayload);
        }
        let mut bytes = decode(&blob.ciphertext)?;
        bytes.extend_from_slice(&tag);
        let cipher = Aes256Gcm::new_from_slice(&*self.0).map_err(|_| SecretError::InvalidKey)?;
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&iv), bytes.as_ref())
            .map_err(|_| SecretError::InvalidPayload)?;
        Ok(String::from_utf8_lossy(&plaintext).into_owned())
    }
}

fn decode(value: &str) -> Result<Vec<u8>, SecretError> {
    STANDARD
        .decode(value)
        .map_err(|_| SecretError::InvalidPayload)
}

struct KeySlot<'a> {
    profile: &'a Path,
    protector: &'a dyn KeyProtector,
    electron_app: &'a str,
    purpose: &'static str,
    electron_key: &'static str,
}

impl KeySlot<'_> {
    fn id(&self) -> String {
        let hash = Sha256::digest(self.profile.as_os_str().as_encoded_bytes());
        format!("{}-{hash:x}", self.purpose)
    }

    fn load(&self) -> Result<Option<DataKey>, SecretError> {
        self.protector
            .load(&self.id())
            .map(|key| key.map(|key| DataKey(Zeroizing::new(key))))
    }

    fn save(&self, key: &DataKey) -> Result<(), SecretError> {
        self.protector.save(&self.id(), &key.0)
    }

    fn migrate(&self) -> Result<Option<DataKey>, SecretError> {
        match fs::read(self.profile.join(self.electron_key)) {
            Ok(wrapped) => electron::unwrap(self.profile, self.electron_app, &wrapped).map(Some),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }
}

/// Optional, fail-open sign-in storage. Construct it with the resolved userData
/// path and the Electron product name (normally `Kairo`, even for dev profiles).
pub struct SecureStore<'a> {
    slot: KeySlot<'a>,
    notice: AtomicBool,
}

impl<'a> SecureStore<'a> {
    pub fn new(profile: &'a Path, protector: &'a dyn KeyProtector, electron_app: &'a str) -> Self {
        Self {
            slot: KeySlot {
                profile,
                protector,
                electron_app,
                purpose: "cloud",
                electron_key: "cloud/key.bin",
            },
            notice: AtomicBool::new(false),
        }
    }

    pub fn available(&self) -> bool {
        let available = self.slot.load().is_ok();
        if !available {
            self.notice.store(true, Ordering::Relaxed);
        }
        available
    }

    pub fn credentials_notice(&self) -> Option<&'static str> {
        (self.notice.load(Ordering::Relaxed) || !self.available())
            .then_some(CREDENTIALS_NOT_SAVED_NOTICE)
    }

    pub fn read(&self, name: &str) -> Option<String> {
        let path = self.path(name)?;
        let _guard = KEY_ACCESS.lock().ok()?;
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return None,
            Err(_) => {
                let _ = remove(&path);
                return None;
            }
        };
        let key = match self.slot.load() {
            Ok(Some(key)) => key,
            Ok(None) => match self.slot.migrate() {
                Ok(Some(key)) => {
                    if let Err(error) = self.slot.save(&key) {
                        tracing::warn!(name, %error, "[Secrets] could not save migrated key");
                        self.notice.store(true, Ordering::Relaxed);
                        return None;
                    }
                    key
                }
                Ok(None) | Err(_) => {
                    tracing::warn!(
                        name,
                        "[Secrets] data key missing from keychain; clearing unreadable secret"
                    );
                    self.notice.store(true, Ordering::Relaxed);
                    let _ = remove(&path);
                    return None;
                }
            },
            Err(SecretError::InvalidKey) => {
                tracing::warn!(
                    name,
                    "[Secrets] data key in keychain is invalid; clearing unreadable secret"
                );
                self.notice.store(true, Ordering::Relaxed);
                let _ = remove(&path);
                return None;
            }
            Err(error) => {
                tracing::warn!(name, %error, "[Secrets] keychain unavailable while reading secret");
                self.notice.store(true, Ordering::Relaxed);
                return None;
            }
        };
        let result = serde_json::from_slice(&data)
            .ok()
            .and_then(|blob| key.decrypt(&blob).ok());
        if result.is_none() {
            tracing::warn!(name, "[Secrets] secret failed to decrypt; clearing file");
            let _ = remove(&path);
        }
        result
    }

    pub fn write(&self, name: &str, value: &str) -> bool {
        let Some(path) = self.path(name) else {
            return false;
        };
        let result = self.write_inner(&path, value);
        if let Err(error) = &result {
            tracing::warn!(name, %error, "[Secrets] could not save secret");
            self.notice.store(true, Ordering::Relaxed);
        }
        result.is_ok()
    }

    fn write_inner(&self, path: &Path, value: &str) -> Result<(), SecretError> {
        let _guard = KEY_ACCESS.lock().map_err(|_| SecretError::Keychain)?;
        let dir = self.slot.profile.join("cloud");
        let key = match self.slot.load() {
            Ok(Some(key)) => key,
            Ok(None) => match self.slot.migrate() {
                Ok(Some(key)) => {
                    self.slot.save(&key)?;
                    key
                }
                Ok(None) | Err(_) => self.replace_key(&dir)?,
            },
            Err(SecretError::InvalidKey) => self.replace_key(&dir)?,
            Err(error) => return Err(error),
        };
        let blob = key.encrypt(value)?;
        fs::create_dir_all(&dir)?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer(file.as_file_mut(), &blob).map_err(io::Error::other)?;
        file.flush()?;
        file.as_file().sync_all()?;
        file.persist(path).map_err(|err| err.error)?;
        Ok(())
    }

    fn replace_key(&self, dir: &Path) -> Result<DataKey, SecretError> {
        match fs::read_dir(dir) {
            Ok(entries) => {
                for entry in entries {
                    let path = entry?.path();
                    if path.extension().is_some_and(|extension| extension == "enc") {
                        self.notice.store(true, Ordering::Relaxed);
                        remove(&path)?;
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let key = DataKey::generate()?;
        self.slot.save(&key)?;
        Ok(key)
    }

    pub fn clear(&self, name: &str) {
        if let (Some(path), Ok(_guard)) = (self.path(name), KEY_ACCESS.lock()) {
            let _ = remove(&path);
        }
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        (!name.is_empty()
            && name
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_')))
        .then(|| self.slot.profile.join("cloud").join(format!("{name}.enc")))
    }
}

/// What first-run migration did with one Electron data key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyMigration {
    /// The native keyring already holds this profile's key.
    AlreadyNative,
    /// Electron's wrapped key was unwrapped and saved in the native keyring.
    Rewrapped,
    /// Electron never wrote this key (nothing saved under it).
    NoElectronKey,
    /// The key could not be read or saved; the message says why.
    Failed(String),
}

/// Rewraps Electron's data keys (`cloud/key.bin`, `api-bible-cache.key`)
/// into native keyring entries, without reading or deleting anything they
/// protect. Electron's wrapped files stay for rollback.
pub fn migrate_electron_keys(
    profile: &Path,
    protector: &dyn KeyProtector,
    electron_app: &str,
) -> Vec<(&'static str, KeyMigration)> {
    let Ok(_guard) = KEY_ACCESS.lock() else {
        return vec![("cloud", KeyMigration::Failed("key lock poisoned".into()))];
    };
    [
        ("cloud", "cloud/key.bin"),
        ("api-bible-cache", "api-bible-cache.key"),
    ]
    .into_iter()
    .map(|(purpose, electron_key)| {
        let slot = KeySlot {
            profile,
            protector,
            electron_app,
            purpose,
            electron_key,
        };
        let result = match slot.load() {
            Ok(Some(_)) => KeyMigration::AlreadyNative,
            Ok(None) | Err(SecretError::InvalidKey) => match slot.migrate() {
                Ok(Some(key)) => match slot.save(&key) {
                    Ok(()) => KeyMigration::Rewrapped,
                    Err(e) => KeyMigration::Failed(e.to_string()),
                },
                Ok(None) => KeyMigration::NoElectronKey,
                Err(e) => KeyMigration::Failed(e.to_string()),
            },
            Err(e) => KeyMigration::Failed(e.to_string()),
        };
        (purpose, result)
    })
    .collect()
}

/// Fail-closed crypto for licensed text. Call once at startup while holding
/// exclusive use of the profile, before opening any cache database connection.
/// Losing the key may require deleting the cache database and its side files.
pub struct ApiCacheCrypto(DataKey);

impl ApiCacheCrypto {
    /// Returns whether cache data had to be discarded after a lost key.
    pub fn open(
        profile: &Path,
        protector: &dyn KeyProtector,
        electron_app: &str,
    ) -> Result<(Self, bool), SecretError> {
        let _guard = KEY_ACCESS.lock().map_err(|_| SecretError::Keychain)?;
        let slot = KeySlot {
            profile,
            protector,
            electron_app,
            purpose: "api-bible-cache",
            electron_key: "api-bible-cache.key",
        };
        match slot.load() {
            Ok(Some(key)) => return Ok((Self(key), false)),
            Ok(None) => {
                if let Ok(Some(key)) = slot.migrate() {
                    slot.save(&key)?;
                    return Ok((Self(key), false));
                }
            }
            Err(SecretError::InvalidKey) => {}
            Err(error) => return Err(error),
        }

        // Do not commit a replacement key until all of the old database files
        // are gone. If cleanup fails, the next startup can retry safely.
        let mut discarded = false;
        for suffix in ["-wal", "-shm", "-journal", ""] {
            discarded |= remove(&profile.join(format!("{CACHE_DB}{suffix}")))?;
        }
        let key = DataKey::generate()?;
        slot.save(&key)?;
        Ok((Self(key), discarded))
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<EncryptedValue, SecretError> {
        self.0.encrypt(plaintext)
    }

    pub fn decrypt(&self, value: &EncryptedValue) -> Result<String, SecretError> {
        self.0.decrypt(value)
    }
}

fn remove(path: &Path) -> io::Result<bool> {
    match fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

/// Apply after loading settings and after merging a settings patch. Returns
/// whether the settings need to be written back.
pub fn retire_provider_keys(settings: &mut Map<String, Value>) -> bool {
    let mut changed = false;
    for (section, names) in [
        (
            "stt",
            &["apiKey", "anthropicApiKey", "deepseekApiKey", "bibleApiKey"][..],
        ),
        ("lyrics", &["braveApiKey", "googleTranslateApiKey"][..]),
    ] {
        if let Some(section) = settings.get_mut(section).and_then(Value::as_object_mut) {
            for &name in names {
                if section.get(name).and_then(Value::as_str) != Some("") {
                    section.insert(name.to_owned(), Value::String(String::new()));
                    changed = true;
                }
            }
        }
    }
    changed
}
