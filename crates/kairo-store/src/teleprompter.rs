use crate::fs_atomic::write_atomic;
use kairo_domain::teleprompter::Script;
use std::{
    fs, io,
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// Each script is replaced independently, so concurrent saves to different
/// scripts cannot overwrite the rest of the library.
pub struct ScriptLibrary {
    directory: PathBuf,
}

impl ScriptLibrary {
    pub fn new(profile: &Path) -> Self {
        Self {
            directory: profile.join("teleprompter"),
        }
    }

    pub fn list(&self) -> io::Result<Vec<Script>> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut scripts = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "json")
                && let Some(id) = path.file_stem().and_then(|name| name.to_str())
                && Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == id)
            {
                // One unreadable file must not hide the rest of the library.
                match self.load(id) {
                    Ok(script) => scripts.push(script),
                    Err(error) => {
                        tracing::warn!(%error, path = %path.display(), "skipping unreadable script")
                    }
                }
            }
        }
        scripts.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms).then(a.id.cmp(&b.id)));
        Ok(scripts)
    }

    pub fn load(&self, id: &str) -> io::Result<Script> {
        let path = self.path(id)?;
        let script: Script = serde_json::from_slice(&fs::read(path)?)?;
        if script.id != id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Script ID does not match its file.",
            ));
        }
        Ok(script)
    }

    pub fn save(&self, id: Option<&str>, title: String, body: String) -> io::Result<Script> {
        let script = Script {
            id: id
                .map(str::to_owned)
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            title,
            body,
            updated_at_ms: chrono::Utc::now().timestamp_millis(),
        };
        write_atomic(&self.path(&script.id)?, &serde_json::to_vec(&script)?)?;
        Ok(script)
    }

    /// Remove a saved script. Removing one that is already gone succeeds.
    pub fn delete(&self, id: &str) -> io::Result<()> {
        match fs::remove_file(self.path(id)?) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    fn path(&self, id: &str) -> io::Result<PathBuf> {
        let uuid = Uuid::parse_str(id)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        if uuid.to_string() != id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid script ID.",
            ));
        }
        Ok(self.directory.join(format!("{uuid}.json")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_a_script_keeps_its_id_and_other_scripts() {
        let profile = tempfile::tempdir().unwrap();
        let library = ScriptLibrary::new(profile.path());
        assert!(library.list().unwrap().is_empty());
        let script = library
            .save(None, "Welcome".into(), "Original".into())
            .unwrap();
        let other = library
            .save(None, "Closing".into(), "Goodbye".into())
            .unwrap();
        let updated = library
            .save(Some(&script.id), script.title, "Olá\n世界".into())
            .unwrap();
        let reopened = ScriptLibrary::new(profile.path());
        assert_eq!(reopened.list().unwrap().len(), 2);
        assert_eq!(reopened.load(&script.id).unwrap(), updated);
        assert_eq!(reopened.load(&other.id).unwrap(), other);
        assert!(reopened.load("../settings").is_err());
    }

    #[test]
    fn unreadable_scripts_are_skipped_and_deleted_scripts_leave() {
        let profile = tempfile::tempdir().unwrap();
        let library = ScriptLibrary::new(profile.path());
        let kept = library.save(None, "Kept".into(), "Body".into()).unwrap();
        let gone = library.save(None, "Gone".into(), "Body".into()).unwrap();
        let broken = Uuid::new_v4().to_string();
        fs::write(
            library.directory.join(format!("{broken}.json")),
            b"{not json",
        )
        .unwrap();
        assert_eq!(library.list().unwrap().len(), 2);
        library.delete(&gone.id).unwrap();
        library.delete(&gone.id).unwrap();
        assert_eq!(library.list().unwrap(), vec![kept]);
        assert!(library.delete("../settings").is_err());
    }
}
