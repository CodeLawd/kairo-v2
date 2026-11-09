//! Electron 33's Chromium OSCrypt wrappers around a base64 data key.

use super::{DataKey, SecretError};
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use super::{KEY_BYTES, STANDARD};
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use base64::Engine as _;
use std::path::Path;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use zeroize::Zeroizing;

pub(super) fn unwrap(profile: &Path, app_name: &str, blob: &[u8]) -> Result<DataKey, SecretError> {
    platform_decrypt(profile, app_name, blob)
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn decode_key(plaintext: &[u8]) -> Result<DataKey, SecretError> {
    let key = Zeroizing::new(
        STANDARD
            .decode(plaintext)
            .map_err(|_| SecretError::InvalidKey)?,
    );
    let key: [u8; KEY_BYTES] = key
        .as_slice()
        .try_into()
        .map_err(|_| SecretError::InvalidKey)?;
    Ok(DataKey(Zeroizing::new(key)))
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn decrypt_cbc(
    ciphertext: &[u8],
    password: &[u8],
    iterations: u32,
) -> Result<DataKey, SecretError> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};

    let mut key = Zeroizing::new([0; 16]);
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password, b"saltysalt", iterations, &mut *key);
    let plaintext = Zeroizing::new(
        cbc::Decryptor::<aes::Aes128>::new((&*key).into(), (&[b' '; 16]).into())
            .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
            .map_err(|_| SecretError::InvalidKey)?,
    );
    decode_key(&plaintext)
}

#[cfg(target_os = "macos")]
fn platform_decrypt(_: &Path, app_name: &str, blob: &[u8]) -> Result<DataKey, SecretError> {
    let ciphertext = blob.strip_prefix(b"v10").ok_or(SecretError::InvalidKey)?;
    let service = format!("{app_name} Safe Storage");
    let password = keyring::Entry::new(&service, app_name)
        .and_then(|entry| entry.get_secret())
        .map(Zeroizing::new)
        .map_err(|_| SecretError::Keychain)?;
    decrypt_cbc(ciphertext, &password, 1003)
}

#[cfg(target_os = "linux")]
fn platform_decrypt(_: &Path, app_name: &str, blob: &[u8]) -> Result<DataKey, SecretError> {
    if let Some(ciphertext) = blob.strip_prefix(b"v10") {
        return decrypt_cbc(ciphertext, b"peanuts", 1).or_else(|_| decrypt_cbc(ciphertext, b"", 1));
    }
    let ciphertext = blob.strip_prefix(b"v11").ok_or(SecretError::InvalidKey)?;
    let mut backend_unlocked = false;
    for lookup in [libsecret_password as fn(&str) -> _, kwallet_password] {
        if let Ok(password) = lookup(app_name) {
            backend_unlocked = true;
            if let Ok(key) = decrypt_cbc(ciphertext, &password, 1) {
                return Ok(key);
            }
        }
    }
    // Chromium only tries the legacy empty password after the configured
    // backend supplied a key. A denied wallet stays denied.
    if backend_unlocked {
        return decrypt_cbc(ciphertext, b"", 1);
    }
    Err(SecretError::Keychain)
}

#[cfg(target_os = "linux")]
fn libsecret_password(app_name: &str) -> Result<Zeroizing<Vec<u8>>, SecretError> {
    use dbus_secret_service::{EncryptionType, SecretService};
    use std::collections::HashMap;

    let service = SecretService::connect(EncryptionType::Dh).map_err(|_| SecretError::Keychain)?;
    let items = service
        .search_items(HashMap::from([
            ("application", app_name),
            ("xdg:schema", "chrome_libsecret_os_crypt_password_v2"),
        ]))
        .map_err(|_| SecretError::Keychain)?;
    let item = if let Some(item) = items.unlocked.first() {
        item
    } else {
        let item = items.locked.first().ok_or(SecretError::InvalidKey)?;
        item.unlock().map_err(|_| SecretError::Keychain)?;
        item
    };
    item.get_secret()
        .map(Zeroizing::new)
        .map_err(|_| SecretError::Keychain)
}

#[cfg(target_os = "linux")]
fn kwallet_password(app_name: &str) -> Result<Zeroizing<Vec<u8>>, SecretError> {
    use dbus::blocking::Connection;
    use std::time::Duration;

    let bus = Connection::new_session().map_err(|_| SecretError::Keychain)?;
    let dbus = bus.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        Duration::from_secs(5),
    );
    let interface = "org.kde.KWallet";
    for suffix in ["6", "5", ""] {
        let service = format!("org.kde.kwalletd{suffix}");
        let has_owner =
            dbus.method_call("org.freedesktop.DBus", "NameHasOwner", (service.as_str(),));
        if !matches!(has_owner, Ok((true,))) {
            continue;
        }
        let path = format!("/modules/kwalletd{suffix}");
        let wallet = bus.with_proxy(service, path, Duration::from_secs(45));
        let Ok((wallet_name,)) =
            wallet.method_call::<(String,), _, _, _>(interface, "networkWallet", ())
        else {
            continue;
        };
        let Ok((handle,)) = wallet.method_call::<(i32,), _, _, _>(
            interface,
            "open",
            (&wallet_name, 0_i64, app_name),
        ) else {
            continue;
        };
        if handle < 0 {
            continue;
        }
        // Electron supplies the application name for authorization. The old
        // Chromium wallet used its compiled-in folder and entry names.
        let result = wallet.method_call::<(String,), _, _, _>(
            interface,
            "readPassword",
            (handle, "Chromium Keys", "Chromium Safe Storage", app_name),
        );
        let _: Result<(i32,), _> =
            wallet.method_call(interface, "close", (handle, false, app_name));
        if let Ok((password,)) = result
            && !password.is_empty()
        {
            return Ok(Zeroizing::new(password.into_bytes()));
        }
    }
    Err(SecretError::Keychain)
}

#[cfg(target_os = "windows")]
fn platform_decrypt(profile: &Path, _: &str, blob: &[u8]) -> Result<DataKey, SecretError> {
    use aes_gcm::{
        Aes256Gcm, Nonce,
        aead::{Aead, KeyInit},
    };

    let Some(ciphertext) = blob.strip_prefix(b"v10") else {
        return decode_key(&dpapi_unprotect(blob)?);
    };
    if ciphertext.len() < 12 + 16 {
        return Err(SecretError::InvalidKey);
    }
    let local_state: serde_json::Value =
        serde_json::from_slice(&std::fs::read(profile.join("Local State"))?)
            .map_err(|_| SecretError::InvalidKey)?;
    let wrapped = local_state
        .pointer("/os_crypt/encrypted_key")
        .and_then(serde_json::Value::as_str)
        .ok_or(SecretError::InvalidKey)?;
    let wrapped = STANDARD
        .decode(wrapped)
        .map_err(|_| SecretError::InvalidKey)?;
    let key = dpapi_unprotect(
        wrapped
            .strip_prefix(b"DPAPI")
            .ok_or(SecretError::InvalidKey)?,
    )?;
    let cipher = Aes256Gcm::new_from_slice(&key).map_err(|_| SecretError::InvalidKey)?;
    let (nonce, data) = ciphertext.split_at(12);
    let plaintext = Zeroizing::new(
        cipher
            .decrypt(Nonce::from_slice(nonce), data)
            .map_err(|_| SecretError::InvalidKey)?,
    );
    decode_key(&plaintext)
}

#[cfg(target_os = "windows")]
fn dpapi_unprotect(data: &[u8]) -> Result<Zeroizing<Vec<u8>>, SecretError> {
    use std::{ptr, slice};
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{
            CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
        },
    };
    use zeroize::Zeroize;

    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).map_err(|_| SecretError::InvalidKey)?,
        pbData: data.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB::default();
    // DPAPI reads the input and owns the output allocation until LocalFree.
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return Err(SecretError::Keychain);
    }
    // DPAPI's successful result initializes cbData bytes at pbData.
    unsafe {
        let plaintext = slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let key = Zeroizing::new(plaintext.to_vec());
        plaintext.zeroize();
        LocalFree(output.pbData.cast());
        Ok(key)
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn platform_decrypt(_: &Path, _: &str, _: &[u8]) -> Result<DataKey, SecretError> {
    Err(SecretError::Keychain)
}
