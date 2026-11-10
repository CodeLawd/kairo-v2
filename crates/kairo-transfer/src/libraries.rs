//! User-made libraries for songs, documents and saved passages
//! (`kairo-libraries.json`).

use crate::{Clock, Error, Listeners, Result, lock, new_id, open, replace_store};
use kairo_domain::collections::{
    LIBRARY_MAX, LibrariesCommand, LibrariesState, Library, LibraryKind, LibraryState,
    normalize_libraries_state, normalize_library_name, with_assignment, without_item,
    without_library,
};
use kairo_store::json_store::JsonStore;
use serde_json::Value;
use std::sync::{Arc, Mutex};

struct Inner {
    store: JsonStore,
    listeners: Listeners<LibrariesState>,
}

#[derive(Clone)]
pub struct LibrariesService {
    inner: Arc<Mutex<Inner>>,
    clock: Clock,
}

fn kind_mut(state: &mut LibrariesState, kind: LibraryKind) -> &mut LibraryState {
    match kind {
        LibraryKind::Songs => &mut state.songs,
        LibraryKind::Documents => &mut state.documents,
        LibraryKind::Scripture => &mut state.scripture,
    }
}

impl LibrariesService {
    pub fn open(user_data: &std::path::Path, clock: Clock) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                store: open(user_data, "kairo-libraries"),
                listeners: Listeners::default(),
            })),
            clock,
        }
    }

    fn read(inner: &Inner) -> LibrariesState {
        normalize_libraries_state(&Value::Object(inner.store.data().clone()))
    }

    /// Every read normalises, so a hand-edited file is never seen raw.
    pub fn snapshot(&self) -> LibrariesState {
        Self::read(&lock(&self.inner))
    }

    pub fn on_changed(&self, listener: impl Fn(&LibrariesState) + Send + Sync + 'static) {
        lock(&self.inner).listeners.add(listener);
    }

    fn write(inner: &mut Inner, state: LibrariesState) -> LibrariesState {
        replace_store(&mut inner.store, &state);
        inner.listeners.notify(&state);
        state
    }

    pub fn apply(&self, command: LibrariesCommand) -> Result<LibrariesState> {
        let mut inner = lock(&self.inner);
        let mut state = Self::read(&inner);
        let now = (self.clock)();
        match command {
            LibrariesCommand::List => return Ok(state),
            LibrariesCommand::Create { kind, name } => {
                let current = kind_mut(&mut state, kind);
                if current.libraries.len() >= LIBRARY_MAX {
                    return Err(Error::User("Delete a library before creating another."));
                }
                current.libraries.push(Library {
                    id: new_id(),
                    name: normalize_library_name(&name, None),
                    created_at: now,
                    updated_at: now,
                    extra: Default::default(),
                });
            }
            LibrariesCommand::Rename {
                kind,
                library_id,
                name,
            } => {
                for library in kind_mut(&mut state, kind)
                    .libraries
                    .iter_mut()
                    .filter(|l| l.id == library_id)
                {
                    library.name = normalize_library_name(&name, Some(&library.name));
                    library.updated_at = now;
                }
            }
            LibrariesCommand::Delete { kind, library_id } => {
                // Items come home to the default rather than disappearing.
                let current = kind_mut(&mut state, kind);
                *current = without_library(std::mem::take(current), &library_id);
            }
            LibrariesCommand::Assign {
                kind,
                item_ids,
                library_id,
            } => {
                let current = kind_mut(&mut state, kind);
                *current = with_assignment(std::mem::take(current), &item_ids, &library_id);
            }
        }
        Ok(Self::write(&mut inner, state))
    }

    /// Drops an item that no longer exists anywhere in the app. No write
    /// when it had no assignment.
    pub fn forget_item(&self, kind: LibraryKind, item_id: &str) {
        let mut inner = lock(&self.inner);
        let mut state = Self::read(&inner);
        let current = kind_mut(&mut state, kind);
        if !current.assignments.contains_key(item_id) {
            return;
        }
        *current = without_item(std::mem::take(current), item_id);
        Self::write(&mut inner, state);
        tracing::info!(?kind, item_id, "[Libraries] Forgot a deleted item");
    }

    pub fn flush(&self) -> std::result::Result<(), String> {
        lock(&self.inner).store.flush()
    }
}
