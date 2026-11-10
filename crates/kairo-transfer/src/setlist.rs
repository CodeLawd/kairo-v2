//! Named song setlists (`kairo-setlist.json`).

use crate::{Clock, Error, Listeners, Result, lock, new_id, open, replace_store};
use kairo_domain::collections::{
    DEFAULT_SETLIST_NAME, SETLIST_MAX_LISTS, SetlistCommand, SetlistState, SongSetlist,
    migrate_setlist_state, normalize_setlist_name, with_song, without_song,
};
use kairo_store::json_store::JsonStore;
use serde_json::Value;
use std::sync::{Arc, Mutex};

struct Inner {
    store: JsonStore,
    listeners: Listeners<SetlistState>,
}

#[derive(Clone)]
pub struct SetlistService {
    inner: Arc<Mutex<Inner>>,
    clock: Clock,
}

impl SetlistService {
    pub fn open(user_data: &std::path::Path, clock: Clock) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                store: open(user_data, "kairo-setlist"),
                listeners: Listeners::default(),
            })),
            clock,
        }
    }

    /// Every read normalises, so a legacy single-list store is never seen raw.
    pub fn snapshot(&self) -> SetlistState {
        let inner = lock(&self.inner);
        Self::read(&inner, (self.clock)())
    }

    fn read(inner: &Inner, now: f64) -> SetlistState {
        migrate_setlist_state(&Value::Object(inner.store.data().clone()), now)
    }

    pub fn on_changed(&self, listener: impl Fn(&SetlistState) + Send + Sync + 'static) {
        lock(&self.inner).listeners.add(listener);
    }

    fn write(inner: &mut Inner, state: SetlistState) -> SetlistState {
        replace_store(&mut inner.store, &state);
        inner.listeners.notify(&state);
        state
    }

    fn edit_list(
        state: SetlistState,
        list_id: Option<&str>,
        now: f64,
        edit: impl FnOnce(&[String]) -> Vec<String>,
    ) -> Result<SetlistState> {
        let target_id = list_id
            .map(str::to_owned)
            .or_else(|| state.active_id.clone());
        let Some(index) = state
            .lists
            .iter()
            .position(|l| Some(&l.id) == target_id.as_ref())
        else {
            return Err(Error::User("Create a playlist first."));
        };
        let mut state = state;
        let list = &mut state.lists[index];
        list.song_ids = edit(&list.song_ids);
        list.updated_at = now;
        Ok(state)
    }

    pub fn apply(&self, command: SetlistCommand) -> Result<SetlistState> {
        let mut inner = lock(&self.inner);
        let now = (self.clock)();
        let mut state = Self::read(&inner, now);
        let next = match command {
            SetlistCommand::List => return Ok(state),
            SetlistCommand::Create { name } => {
                if state.lists.len() >= SETLIST_MAX_LISTS {
                    return Err(Error::User("Delete a playlist before creating another."));
                }
                let created = SongSetlist {
                    id: new_id(),
                    name: normalize_setlist_name(&name, Some(DEFAULT_SETLIST_NAME)),
                    song_ids: Vec::new(),
                    created_at: now,
                    updated_at: now,
                    extra: Default::default(),
                };
                // New lists go to the top and become active.
                state.active_id = Some(created.id.clone());
                state.lists.insert(0, created);
                state
            }
            SetlistCommand::Rename { list_id, name } => {
                for list in state.lists.iter_mut().filter(|l| l.id == list_id) {
                    list.name = normalize_setlist_name(&name, Some(&list.name));
                    list.updated_at = now;
                }
                state
            }
            SetlistCommand::Delete { list_id } => {
                state.lists.retain(|l| l.id != list_id);
                if state.active_id.as_deref() == Some(list_id.as_str()) {
                    state.active_id = state.lists.first().map(|l| l.id.clone());
                }
                state
            }
            SetlistCommand::Select { list_id } => {
                state.active_id = list_id.filter(|id| state.lists.iter().any(|l| &l.id == id));
                state
            }
            SetlistCommand::Add {
                song_id,
                index,
                list_id,
            } => Self::edit_list(state, list_id.as_deref(), now, |ids| {
                with_song(ids, &song_id, index)
            })?,
            SetlistCommand::RemoveSong { song_id, list_id } => {
                Self::edit_list(state, list_id.as_deref(), now, |ids| {
                    without_song(ids, &song_id)
                })?
            }
            SetlistCommand::Clear { list_id } => {
                Self::edit_list(state, list_id.as_deref(), now, |_| Vec::new())?
            }
        };
        Ok(Self::write(&mut inner, next))
    }

    /// A setlist from an imported file, already filled. A taken name gets a
    /// numeric suffix, so nothing is merged into a list the operator made.
    pub fn import_list(&self, name: &str, song_ids: &[String]) -> Result<SongSetlist> {
        let mut inner = lock(&self.inner);
        let now = (self.clock)();
        let mut state = Self::read(&inner, now);
        if state.lists.len() >= SETLIST_MAX_LISTS {
            return Err(Error::User("Delete a playlist before importing another."));
        }
        let base = normalize_setlist_name(name, Some(DEFAULT_SETLIST_NAME));
        let taken: std::collections::HashSet<String> =
            state.lists.iter().map(|l| l.name.to_lowercase()).collect();
        let mut unique = base.clone();
        let mut n = 2;
        while taken.contains(&unique.to_lowercase()) {
            unique = format!("{base} ({n})");
            n += 1;
        }
        let created = SongSetlist {
            id: new_id(),
            name: unique,
            song_ids: song_ids
                .iter()
                .fold(Vec::new(), |ids, id| with_song(&ids, id, None)),
            created_at: now,
            updated_at: now,
            extra: Default::default(),
        };
        state.active_id = Some(created.id.clone());
        state.lists.insert(0, created.clone());
        Self::write(&mut inner, state);
        Ok(created)
    }

    /// Drops a song the library no longer has.
    pub fn forget_song(&self, song_id: &str) {
        let mut inner = lock(&self.inner);
        let now = (self.clock)();
        let mut state = Self::read(&inner, now);
        if !state
            .lists
            .iter()
            .any(|l| l.song_ids.iter().any(|id| id == song_id))
        {
            return;
        }
        for list in state
            .lists
            .iter_mut()
            .filter(|l| l.song_ids.iter().any(|id| id == song_id))
        {
            list.song_ids = without_song(&list.song_ids, song_id);
            list.updated_at = now;
        }
        Self::write(&mut inner, state);
        tracing::info!(song_id, "[Setlist] Removed a song that left the library");
    }

    pub fn flush(&self) -> std::result::Result<(), String> {
        lock(&self.inner).store.flush()
    }
}
