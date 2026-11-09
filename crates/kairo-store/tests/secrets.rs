use kairo_store::secrets::{
    ApiCacheCrypto, EncryptedValue, KeyProtector, SecretError, SecureStore, retire_provider_keys,
};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    process::{Command, Stdio},
    sync::Mutex,
};

#[derive(Default)]
struct TestProtector(Mutex<HashMap<String, Vec<u8>>>);

impl KeyProtector for TestProtector {
    fn load(&self, slot: &str) -> Result<Option<[u8; 32]>, SecretError> {
        self.0
            .lock()
            .unwrap()
            .get(slot)
            .map(|key| {
                key.as_slice()
                    .try_into()
                    .map_err(|_| SecretError::InvalidKey)
            })
            .transpose()
    }

    fn save(&self, slot: &str, key: &[u8; 32]) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(slot.to_owned(), key.to_vec());
        Ok(())
    }
}

struct Unavailable;

impl KeyProtector for Unavailable {
    fn load(&self, _: &str) -> Result<Option<[u8; 32]>, SecretError> {
        Err(SecretError::Keychain)
    }

    fn save(&self, _: &str, _: &[u8; 32]) -> Result<(), SecretError> {
        Err(SecretError::Keychain)
    }
}

#[test]
fn credentials_survive_reopening_and_tampering_discards_them() {
    let profile = tempfile::tempdir().unwrap();
    let protector = TestProtector::default();
    let store = SecureStore::new(profile.path(), &protector, "Kairo");
    assert!(store.write("refresh-token", "fake-token"));
    let store = SecureStore::new(profile.path(), &protector, "Kairo");
    assert_eq!(store.read("refresh-token").as_deref(), Some("fake-token"));
    let blob = profile.path().join("cloud/refresh-token.enc");
    assert!(!fs::read_to_string(&blob).unwrap().contains("fake-token"));
    fs::write(&blob, b"{}").unwrap();
    assert_eq!(store.read("refresh-token"), None);
    assert!(!blob.exists());
    assert!(store.write("refresh-token", ""));
    assert_eq!(store.read("refresh-token").as_deref(), Some(""));
    store.clear("refresh-token");
    assert_eq!(store.read("refresh-token"), None);
}

#[test]
fn unavailable_keychain_forgets_sign_in_and_disables_cache() {
    let profile = tempfile::tempdir().unwrap();
    let store = SecureStore::new(profile.path(), &Unavailable, "Kairo");
    assert!(!store.available());
    assert!(!store.write("refresh-token", "fake-token"));
    assert_eq!(store.read("refresh-token"), None);
    assert!(store.credentials_notice().is_some());
    assert!(ApiCacheCrypto::open(profile.path(), &Unavailable, "Kairo").is_err());
    assert_eq!(fs::read_dir(profile.path()).unwrap().count(), 0);
}

#[test]
fn unrecoverable_electron_keys_require_sign_in_and_discard_the_cache() {
    let profile = tempfile::tempdir().unwrap();
    let root = profile.path();
    fs::create_dir(root.join("cloud")).unwrap();
    fs::write(root.join("cloud/key.bin"), b"bad wrap").unwrap();
    fs::write(root.join("cloud/refresh-token.enc"), b"old payload").unwrap();
    fs::write(root.join("api-bible-cache.key"), b"bad wrap").unwrap();
    for suffix in ["", "-shm", "-wal", "-journal"] {
        fs::write(
            root.join(format!("api-bible-cache.db{suffix}")),
            b"old cache",
        )
        .unwrap();
    }

    let protector = TestProtector::default();
    let store = SecureStore::new(root, &protector, "Kairo");
    assert_eq!(store.read("refresh-token"), None);
    assert!(store.credentials_notice().is_some());
    assert!(!root.join("cloud/refresh-token.enc").exists());
    assert!(store.write("refresh-token", "replacement"));
    assert_eq!(store.read("refresh-token").as_deref(), Some("replacement"));
    let (cache, rebuilt) = ApiCacheCrypto::open(root, &protector, "Kairo").unwrap();
    assert!(rebuilt);
    for suffix in ["", "-shm", "-wal", "-journal"] {
        assert!(!root.join(format!("api-bible-cache.db{suffix}")).exists());
    }
    let payload = cache.encrypt("verse").unwrap();
    let (cache, rebuilt) = ApiCacheCrypto::open(root, &protector, "Kairo").unwrap();
    assert!(!rebuilt);
    assert_eq!(cache.decrypt(&payload).unwrap(), "verse");
}

#[test]
fn malformed_native_keys_can_be_replaced_without_retaining_old_payloads() {
    let profile = tempfile::tempdir().unwrap();
    let root = profile.path();
    let protector = TestProtector::default();
    let store = SecureStore::new(root, &protector, "Kairo");
    assert!(store.write("refresh-token", "lost"));
    assert!(store.write("license-snapshot", "lost"));
    assert!(ApiCacheCrypto::open(root, &protector, "Kairo").is_ok());
    fs::write(root.join("api-bible-cache.db"), b"lost").unwrap();
    for key in protector.0.lock().unwrap().values_mut() {
        key.truncate(31);
    }
    assert_eq!(store.read("refresh-token"), None);
    assert!(store.credentials_notice().is_some());
    assert!(store.write("refresh-token", "replacement"));
    assert_eq!(store.read("refresh-token").as_deref(), Some("replacement"));
    assert!(!root.join("cloud/license-snapshot.enc").exists());
    let (_, discarded) = ApiCacheCrypto::open(root, &protector, "Kairo").unwrap();
    assert!(discarded);
    assert!(!root.join("api-bible-cache.db").exists());
}

#[test]
fn a_failed_cache_reset_keeps_the_native_key_unset_for_retry() {
    let profile = tempfile::tempdir().unwrap();
    let root = profile.path();
    let protector = TestProtector::default();
    let cache = root.join("api-bible-cache.db");
    fs::create_dir(&cache).unwrap();
    assert!(ApiCacheCrypto::open(root, &protector, "Kairo").is_err());
    assert!(protector.0.lock().unwrap().is_empty());
    fs::remove_dir(&cache).unwrap();
    fs::write(&cache, b"lost").unwrap();
    let (_, discarded) = ApiCacheCrypto::open(root, &protector, "Kairo").unwrap();
    assert!(discarded);
    assert!(!cache.exists());
}

#[test]
fn retired_provider_keys_are_blank_and_other_settings_are_kept() {
    let mut settings = serde_json::json!({
        "stt": {"provider": "deepgram", "apiKey": "retired", "anthropicApiKey": "retired",
                "deepseekApiKey": "retired", "bibleApiKey": "retired"},
        "lyrics": {"braveApiKey": "retired", "googleTranslateApiKey": "retired"}
    });
    assert!(retire_provider_keys(settings.as_object_mut().unwrap()));
    assert_eq!(
        settings,
        serde_json::json!({
            "stt": {"provider": "deepgram", "apiKey": "", "anthropicApiKey": "",
                    "deepseekApiKey": "", "bibleApiKey": ""},
            "lyrics": {"braveApiKey": "", "googleTranslateApiKey": ""}
        })
    );
    assert!(!retire_provider_keys(settings.as_object_mut().unwrap()));
}

fn typescript(mode: &str, input: &[u8]) -> Vec<u8> {
    let mut child = Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/secret-interop.ts"
        ))
        .arg(mode)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Node 22.18+ is required for this opt-in TypeScript interoperability test");
    child.stdin.take().unwrap().write_all(input).unwrap();
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    result.stdout
}

#[test]
#[ignore = "requires Node 22.18+ with TypeScript stripping"]
fn typescript_aes_gcm_roundtrips_both_ways() {
    let profile = tempfile::tempdir().unwrap();
    let protector = TestProtector::default();
    let (cache, _) = ApiCacheCrypto::open(profile.path(), &protector, "Kairo").unwrap();
    // Replace the generated cache key with the interoperability fixture key.
    *protector.0.lock().unwrap().values_mut().next().unwrap() = vec![7; 32];
    let (fixture, _) = ApiCacheCrypto::open(profile.path(), &protector, "Kairo").unwrap();
    let phrase = "Synthetic verse — Ἐν ἀρχῇ";
    for value in [phrase, ""] {
        let payload: EncryptedValue =
            serde_json::from_slice(&typescript("encrypt", value.as_bytes())).unwrap();
        assert_eq!(fixture.decrypt(&payload).unwrap(), value);
        assert!(cache.decrypt(&payload).is_err());
        let payload = serde_json::to_vec(&fixture.encrypt(value).unwrap()).unwrap();
        assert_eq!(typescript("decrypt", &payload), value.as_bytes());
    }
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "requires an isolated Electron 33 profile and interactive macOS Keychain"]
fn real_macos_electron_profile_survives_migration() {
    use kairo_store::secrets::KeyringProtector;
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;

    let profile = PathBuf::from(std::env::var_os("KAIRO_SECRET_TEST_PROFILE").unwrap());
    let name = std::env::var("KAIRO_SECRET_TEST_APP").unwrap();
    assert!(name.starts_with("KairoMigrationFixture-"));
    assert!(profile.starts_with(std::env::temp_dir()) || profile.starts_with("/private/tmp"));
    let hash = Sha256::digest(profile.as_os_str().as_encoded_bytes());
    let protector = KeyringProtector;
    let cleanup = || {
        for purpose in ["cloud", "api-bible-cache"] {
            let slot = format!("{purpose}-{hash:x}");
            let _ = keyring::Entry::new("Kairo Native Storage", &slot)
                .and_then(|entry| entry.delete_credential());
        }
        let _ = keyring::Entry::new(&format!("{name} Safe Storage"), &name)
            .and_then(|entry| entry.delete_credential());
    };

    // Clean up the dedicated fixture entries even if an assertion fails.
    let outcome = std::panic::catch_unwind(|| {
        let store = SecureStore::new(&profile, &protector, &name);
        assert_eq!(
            store.read("refresh-token").as_deref(),
            Some("synthetic refresh token")
        );
        let (cache, discarded) = ApiCacheCrypto::open(&profile, &protector, &name).unwrap();
        assert!(!discarded);
        let payload = fs::read(profile.join("api-cache-payload.json")).unwrap();
        let payload = serde_json::from_slice(&payload).unwrap();
        assert_eq!(cache.decrypt(&payload).unwrap(), "synthetic licensed verse");
        let store = SecureStore::new(&profile, &protector, &name);
        assert_eq!(
            store.read("refresh-token").as_deref(),
            Some("synthetic refresh token")
        );
        assert!(profile.join("cloud/key.bin").exists());
        assert!(profile.join("api-bible-cache.key").exists());
    });
    cleanup();
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}
