//! Passages the operator kept, with their verse text (`kairo-passages.json`).

use crate::{Clock, Error, LibrariesService, Listeners, Result, lock, new_id, open};
use kairo_domain::collections::{LibrariesCommand, LibraryKind, with_passage};
use kairo_domain::scripture::{PassagesCommand, SavedPassage};
use kairo_store::json_store::JsonStore;
use serde_json::Value;
use std::sync::{Arc, Mutex};

struct Inner {
    store: JsonStore,
    listeners: Listeners<Vec<SavedPassage>>,
}

#[derive(Clone)]
pub struct PassagesService {
    inner: Arc<Mutex<Inner>>,
    libraries: LibrariesService,
    clock: Clock,
}

use kairo_domain::js::slice_utf16;

impl PassagesService {
    pub fn open(user_data: &std::path::Path, libraries: LibrariesService, clock: Clock) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                store: open(user_data, "kairo-passages"),
                listeners: Vec::new(),
            })),
            libraries,
            clock,
        }
    }

    /// Entries without a string id or a verses array are dropped.
    fn read(inner: &Inner) -> Vec<SavedPassage> {
        inner
            .store
            .get("passages")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter(|p| p["id"].is_string() && p["verses"].is_array())
                    .filter_map(|p| serde_json::from_value(p.clone()).ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn snapshot(&self) -> Vec<SavedPassage> {
        Self::read(&lock(&self.inner))
    }

    pub fn on_changed(&self, listener: impl Fn(&Vec<SavedPassage>) + Send + 'static) {
        lock(&self.inner).listeners.push(Box::new(listener));
    }

    fn write(inner: &mut Inner, passages: Vec<SavedPassage>) -> Vec<SavedPassage> {
        if let Ok(value) = kairo_domain::json::to_js_value(&passages) {
            inner.store.set("passages", value);
        }
        for listener in &inner.listeners {
            listener(&passages);
        }
        passages
    }

    pub fn apply(&self, command: PassagesCommand) -> Result<Vec<SavedPassage>> {
        let mut inner = lock(&self.inner);
        let current = Self::read(&inner);
        match command {
            PassagesCommand::List => Ok(current),
            PassagesCommand::Save { result, library_id } => {
                if result.reference.is_empty() || result.verses.is_empty() {
                    return Err(Error::User("Look up a passage before saving it."));
                }
                let passage = SavedPassage {
                    id: new_id(),
                    reference: slice_utf16(&result.reference, 200),
                    translation: result.translation,
                    verses: result.verses,
                    saved_at: (self.clock)(),
                    extra: Default::default(),
                };
                let (reference, translation) =
                    (passage.reference.clone(), passage.translation.clone());
                let next = with_passage(current, passage);
                let saved = next
                    .iter()
                    .find(|p| p.reference == reference && p.translation == translation);
                if let (Some(library_id), Some(saved)) =
                    (library_id.filter(|l| !l.is_empty()), saved)
                {
                    self.libraries.apply(LibrariesCommand::Assign {
                        kind: LibraryKind::Scripture,
                        item_ids: vec![saved.id.clone()],
                        library_id,
                    })?;
                }
                Ok(Self::write(&mut inner, next))
            }
            PassagesCommand::Remove { passage_id } => {
                self.libraries
                    .forget_item(LibraryKind::Scripture, &passage_id);
                let next = current.into_iter().filter(|p| p.id != passage_id).collect();
                Ok(Self::write(&mut inner, next))
            }
        }
    }

    pub fn flush(&self) -> std::result::Result<(), String> {
        lock(&self.inner).store.flush()
    }
}
