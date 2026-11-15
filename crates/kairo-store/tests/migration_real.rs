//! Opt-in: reads a real Electron profile's localStorage (from a temporary
//! copy) and prints only the key names. `KAIRO_REAL_PROFILE=<userData>`.

#[test]
#[ignore = "needs a real Electron profile"]
fn real_profile_local_storage_keys() {
    let Some(dir) = std::env::var_os("KAIRO_REAL_PROFILE") else {
        return;
    };
    let keys = kairo_store::migration::local_storage::read_local_storage(
        std::path::Path::new(&dir),
        kairo_store::migration::local_storage::FILE_ORIGIN,
    )
    .unwrap();
    for (key, value) in &keys {
        eprintln!("key {key} ({} chars)", value.chars().count());
    }
    assert!(!keys.is_empty());
}

/// Opt-in: the whole migration on a *copy* of a real profile
/// (`KAIRO_REAL_PROFILE_COPY=<copied userData>`), with real audio inputs
/// passed as `KAIRO_REAL_INPUTS` lines `id<TAB>label`, and keys kept in
/// memory so the keychain is not touched. Prints the report.
#[test]
#[ignore = "needs a copied real profile"]
fn real_profile_copy_migrates() {
    let Some(dir) = std::env::var_os("KAIRO_REAL_PROFILE_COPY") else {
        return;
    };
    struct Memory;
    impl kairo_store::secrets::KeyProtector for Memory {
        fn load(&self, _: &str) -> Result<Option<[u8; 32]>, kairo_store::secrets::SecretError> {
            Ok(None)
        }
        fn save(&self, _: &str, _: &[u8; 32]) -> Result<(), kairo_store::secrets::SecretError> {
            Ok(())
        }
    }
    let inputs = std::env::var("KAIRO_REAL_INPUTS").unwrap_or_default();
    let devices = move || {
        inputs
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(id, label)| kairo_store::migration::devices::InputDevice {
                id: id.into(),
                label: label.into(),
            })
            .collect()
    };
    let user_data = std::path::PathBuf::from(dir);
    let report = kairo_store::migration::run(&kairo_store::migration::MigrationInput {
        user_data: &user_data,
        default_workspace: dirs_next_documents(),
        protector: &Memory,
        electron_app: "Kairo",
        devices: &devices,
        now: chrono::Local::now(),
    });
    eprintln!(
        "elapsed {:.1} ms, imported {}",
        report.elapsed_ms, report.imported_keys
    );
    for note in &report.notes {
        eprintln!("note attention={} {}", note.attention, note.text);
    }
}

fn dirs_next_documents() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Documents/Kairo")
}
