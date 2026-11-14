//! Application settings handlers and their ordered cross-service effects.
//!
//! The store owns normalization and persistence; this layer consumes every
//! effect it returns. Implementations of [`SettingsServices`] belong in the
//! composition root, above service crates that themselves depend on core.

use kairo_domain::cloud::SecretsConfigured;
use kairo_store::settings::{
    LaunchContext, SettingsEffect, SettingsSetError, SettingsStore, now_ms, random_uuid,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// Service calls made by `settings:set` in Electron's main IPC layer.
///
/// Reconcile and church synchronization enqueue their work and return without
/// waiting for output or network I/O. The other methods apply their change
/// synchronously (or enqueue it in order on the owning actor). No method is
/// invoked while the settings store is locked, so callbacks may read it.
/// Scripture values retain their JSON shape at this dynamic API boundary.
pub trait SettingsServices: Send + Sync {
    fn secrets_configured(&self) -> SecretsConfigured;
    fn reconcile_outputs(&self, outputs: Option<Value>);
    fn prune_props(&self);
    fn sync_church_profile(&self, profile: Value);
    fn broadcast_settings_changed(&self, settings: Value);
    fn scripture_default_translation(&self, translation: Value);
    fn scripture_auto_mode(&self, enabled: Value);
    fn scripture_confidence(&self, threshold: Value);
    fn orchestrator_translation(&self, translation: Value);
    fn orchestrator_auto_mode(&self, enabled: Value);
    fn orchestrator_confidence(&self, threshold: Value);
}

/// Serialized settings handler, intended to be owned by the application actor.
/// Mutation needs exclusive access so effects from two settings writes cannot
/// interleave; reads may be called from service callbacks.
pub struct SettingsWiring {
    store: Arc<Mutex<SettingsStore>>,
    services: Arc<dyn SettingsServices>,
}

impl SettingsWiring {
    pub fn new(store: Arc<Mutex<SettingsStore>>, services: Arc<dyn SettingsServices>) -> Self {
        Self { store, services }
    }

    /// The existing shared store, for service composition and launch migrations.
    pub fn store(&self) -> &Arc<Mutex<SettingsStore>> {
        &self.store
    }

    /// `settings.get`: secret-bearing sections never expose provider keys.
    pub fn get(&self, key: &str) -> Value {
        let mut id = random_uuid;
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_for_renderer(
                key,
                &mut LaunchContext {
                    now_ms: now_ms(),
                    new_id: &mut id,
                },
            )
    }

    /// `settings.getAll` and the payload used for `settings.changed`.
    pub fn get_all(&self) -> Value {
        let configured = self.services.secrets_configured();
        let mut id = random_uuid;
        self.store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .for_renderer(
                &configured,
                &mut LaunchContext {
                    now_ms: now_ms(),
                    new_id: &mut id,
                },
            )
    }

    /// Persist, release the store lock, then dispatch effects in source order.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), SettingsSetError> {
        let mut id = random_uuid;
        let effects = self
            .store
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .apply_set(
                key,
                value,
                &mut LaunchContext {
                    now_ms: now_ms(),
                    new_id: &mut id,
                },
            )?;
        for effect in effects {
            match effect {
                SettingsEffect::ReconcileOutputs(outputs) => {
                    self.services.reconcile_outputs(outputs)
                }
                SettingsEffect::PruneProps => self.services.prune_props(),
                SettingsEffect::SyncChurchProfile(profile) => {
                    self.services.sync_church_profile(profile)
                }
                SettingsEffect::BroadcastSettingsChanged => self.broadcast_changed(),
                SettingsEffect::ApplyScripture(settings) => {
                    let translation = settings
                        .get("defaultTranslation")
                        .cloned()
                        .unwrap_or(Value::Null);
                    let auto = settings.get("autoMode").cloned().unwrap_or(Value::Null);
                    let confidence = settings
                        .get("confidenceThreshold")
                        .cloned()
                        .unwrap_or(Value::Null);
                    self.services
                        .scripture_default_translation(translation.clone());
                    self.services.scripture_auto_mode(auto.clone());
                    self.services.scripture_confidence(confidence.clone());
                    self.services.orchestrator_translation(translation);
                    self.services.orchestrator_auto_mode(auto);
                    self.services.orchestrator_confidence(confidence);
                }
            }
        }
        Ok(())
    }

    /// Used after licence/provider changes, organization-secret pulls and
    /// persisted display rebindings, as well as the two secret-section writes.
    pub fn broadcast_changed(&self) {
        self.services.broadcast_settings_changed(self.get_all());
    }

    pub fn test_api_key() -> Value {
        json!({"ok": false, "message": "Provider credentials are managed in your church account."})
    }
}
