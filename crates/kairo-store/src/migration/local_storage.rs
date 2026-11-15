//! Electron's renderer `localStorage`, read from Chromium's LevelDB
//! (`<userData>/Local Storage/leveldb`).
//!
//! Chromium 130 keys a value as `_` + origin + `\0` + encoded key, and keeps
//! `META:` and `VERSION` records beside them. A key or value is one byte
//! saying how it is encoded, then the text: `\x01` Latin-1, `\x00` UTF-16LE.
//! The packaged renderer loads from `file://`.
//!
//! The database is copied to a temporary folder first and read there, so the
//! Electron profile is never opened for writing (opening LevelDB can compact
//! and rewrite its log).

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use rusty_leveldb::{DB, LdbIterator, Options};

/// The packaged renderer's origin.
pub const FILE_ORIGIN: &str = "file://";

/// Why `localStorage` could not be read.
#[derive(Debug, thiserror::Error)]
pub enum LocalStorageError {
    #[error("could not copy the localStorage database: {0}")]
    Copy(#[from] io::Error),
    #[error("could not read the localStorage database: {0}")]
    Read(String),
}

/// Every `key → value` stored for `origin`. A profile with no localStorage
/// folder gives an empty map.
pub fn read_local_storage(
    user_data: &Path,
    origin: &str,
) -> Result<BTreeMap<String, String>, LocalStorageError> {
    let source = user_data.join("Local Storage").join("leveldb");
    if !source.is_dir() {
        return Ok(BTreeMap::new());
    }
    let copy = tempfile::tempdir()?;
    for entry in fs::read_dir(&source)? {
        let entry = entry?;
        let name = entry.file_name();
        // The lock belongs to a running Electron; the copy needs none.
        if name == "LOCK" || !entry.file_type()?.is_file() {
            continue;
        }
        fs::copy(entry.path(), copy.path().join(&name))?;
    }
    let options = Options {
        create_if_missing: false,
        ..Options::default()
    };
    let mut db =
        DB::open(copy.path(), options).map_err(|e| LocalStorageError::Read(e.to_string()))?;
    let mut iter = db
        .new_iter()
        .map_err(|e| LocalStorageError::Read(e.to_string()))?;
    let prefix = {
        let mut p = b"_".to_vec();
        p.extend_from_slice(origin.as_bytes());
        p.push(0);
        p
    };
    let mut out = BTreeMap::new();
    while let Some((key, value)) = iter.next() {
        let Some(rest) = key.strip_prefix(prefix.as_slice()) else {
            continue;
        };
        if let (Some(key), Some(value)) = (decode(rest), decode(&value)) {
            out.insert(key, value);
        }
    }
    Ok(out)
}

/// One encoded string: `\x01` Latin-1 or `\x00` UTF-16LE.
pub fn decode(bytes: &[u8]) -> Option<String> {
    let (kind, text) = bytes.split_first()?;
    match kind {
        1 => Some(text.iter().map(|&b| char::from(b)).collect()),
        0 if text.len() % 2 == 0 => {
            let units: Vec<u16> = text
                .as_chunks::<2>()
                .0
                .iter()
                .map(|c| u16::from_le_bytes(*c))
                .collect();
            Some(String::from_utf16_lossy(&units))
        }
        _ => None,
    }
}

/// The inverse of [`decode`], as Chromium writes: Latin-1 when every
/// character fits, else UTF-16LE. For test fixtures.
pub fn encode(text: &str) -> Vec<u8> {
    if text.chars().all(|c| (c as u32) < 256) {
        let mut out = vec![1];
        out.extend(text.chars().map(|c| c as u8));
        out
    } else {
        let mut out = vec![0];
        for unit in text.encode_utf16() {
            out.extend_from_slice(&unit.to_le_bytes());
        }
        out
    }
}

/// Writes `entries` for `origin` as Chromium would, for test fixtures.
pub fn write_fixture(
    user_data: &Path,
    origin: &str,
    entries: &[(&str, &str)],
) -> Result<(), LocalStorageError> {
    let dir = user_data.join("Local Storage").join("leveldb");
    fs::create_dir_all(&dir)?;
    let options = Options {
        create_if_missing: true,
        ..Options::default()
    };
    let mut db = DB::open(&dir, options).map_err(|e| LocalStorageError::Read(e.to_string()))?;
    db.put(b"VERSION", b"1")
        .map_err(|e| LocalStorageError::Read(e.to_string()))?;
    let mut meta = b"META:".to_vec();
    meta.extend_from_slice(origin.as_bytes());
    db.put(&meta, b"\x08\x01")
        .map_err(|e| LocalStorageError::Read(e.to_string()))?;
    for (key, value) in entries {
        let mut k = b"_".to_vec();
        k.extend_from_slice(origin.as_bytes());
        k.push(0);
        k.extend(encode(key));
        db.put(&k, &encode(value))
            .map_err(|e| LocalStorageError::Read(e.to_string()))?;
    }
    db.flush()
        .map_err(|e| LocalStorageError::Read(e.to_string()))?;
    Ok(())
}
