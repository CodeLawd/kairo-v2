//! What the manager reads and writes outside the engine: settings, the
//! media library (for lobby playlists) and the orchestrator's routing.

use kairo_core::engine::OutputEngine;
use kairo_domain::media::{MediaItem, MediaPlayback};
use kairo_domain::overlay::OverlayOutput;
use kairo_domain::overlay_defaults::normalize_overlay_settings;
use kairo_domain::program::{PresentationSettings, normalize_presentation_settings};
use kairo_media::library::MediaService;
use kairo_media::settings::SettingsPort;
use kairo_orchestrator::engine::EngineSurfaces;
use kairo_store::settings::SettingsStore;
use serde_json::Value;
use std::sync::Mutex;

/// The two settings sections the manager reads (`overlay`, `presentation`),
/// and the one write it makes: a re-matched display id.
pub trait OutputSettings: Send + Sync + 'static {
    /// `normalizeOverlaySettings(store.get('overlay')).outputs`.
    fn outputs(&self) -> Vec<OverlayOutput>;
    /// `normalizePresentationSettings(store.get('presentation'))`.
    fn presentation(&self) -> PresentationSettings;
    /// Writes `new_id` for `output` only while the stored id is still
    /// `old_id`, so a replug cannot undo a newer operator choice. `stage`
    /// names a stage display instead of an output. True when written.
    fn persist_display_rebinding(
        &self,
        output: &str,
        old_id: u32,
        new_id: u32,
        stage: bool,
    ) -> bool;
}

impl OutputSettings for Mutex<SettingsStore> {
    fn outputs(&self) -> Vec<OverlayOutput> {
        let store = self.lock().unwrap_or_else(|e| e.into_inner());
        normalize_overlay_settings(store.data().get("overlay")).outputs
    }

    fn presentation(&self) -> PresentationSettings {
        let store = self.lock().unwrap_or_else(|e| e.into_inner());
        normalize_presentation_settings(store.data().get("presentation").unwrap_or(&Value::Null))
    }

    fn persist_display_rebinding(
        &self,
        output: &str,
        old_id: u32,
        new_id: u32,
        stage: bool,
    ) -> bool {
        self.lock()
            .unwrap_or_else(|e| e.into_inner())
            .persist_display_rebinding(output, old_id, new_id, stage)
    }
}

/// What a lobby playlist reads from the media library (`mediaService`).
pub trait LobbyMedia: Send + Sync + 'static {
    /// The playlist's items in order. Missing files stay in the playlist
    /// (the dock reports them) but are skipped here. Empty for an unknown id.
    fn playlist_items(&self, playlist_id: &str) -> Vec<MediaItem>;
    /// `getPlayback(id)`: the item's saved trim, loop and colour settings.
    fn playback(&self, item_id: &str) -> MediaPlayback;
}

impl<S: SettingsPort + 'static> LobbyMedia for Mutex<MediaService<S>> {
    fn playlist_items(&self, playlist_id: &str) -> Vec<MediaItem> {
        let media = self.lock().unwrap_or_else(|e| e.into_inner());
        let library = media.library();
        let Some(playlist) = library.playlists.iter().find(|p| p.id == playlist_id) else {
            return Vec::new();
        };
        playlist
            .item_ids
            .iter()
            .filter_map(|id| media.item(id).cloned())
            .collect()
    }

    fn playback(&self, item_id: &str) -> MediaPlayback {
        self.lock()
            .unwrap_or_else(|e| e.into_inner())
            .playback(item_id)
    }
}

/// Where the manager tells the orchestrator which surfaces a push or a
/// Clear may reach ([`EngineSurfaces::set_routing`]).
pub trait RoutingSink: Send + Sync + 'static {
    fn set_routing(&self, outputs: Vec<OverlayOutput>, followers: Vec<String>);
}

impl<E: OutputEngine> RoutingSink for EngineSurfaces<E> {
    fn set_routing(&self, outputs: Vec<OverlayOutput>, followers: Vec<String>) {
        EngineSurfaces::set_routing(self, outputs, followers);
    }
}
