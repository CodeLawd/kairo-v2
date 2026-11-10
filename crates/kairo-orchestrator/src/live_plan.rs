//! The live sermon playlist. From `src/main/services/scripture/live-plan.ts`
//! and the store half of `sermon-plans.ts` (plan building is prompt 042's).
//!
//! Consumers read the index at use time, so selecting or editing the live
//! playlist takes effect without restarting the pipeline. The service
//! outlives the orchestrator: the Operator dropdown works while stopped.

use kairo_domain::scripture::plan::{
    SermonPlanIndex, advance_plan_expectation, build_sermon_plan_index,
};
use kairo_domain::scripture::{LivePlanState, SermonPlan};
use kairo_store::json_store::JsonStore;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub trait PlanSource: Send {
    fn get(&self, id: &str) -> Option<SermonPlan>;
    fn live_plan_id(&self) -> Option<String>;
    fn set_live_plan_id(&mut self, id: Option<&str>);
    /// Newest first.
    fn list(&self) -> Vec<SermonPlan>;
    /// Stamps `updatedAt`, replaces the plan with the same id or puts it first.
    fn save(&mut self, plan: SermonPlan, now_ms: f64) -> SermonPlan;
    /// Removes the plan and clears the live id if it pointed at it.
    fn delete(&mut self, id: &str);
}

use kairo_domain::js::now_ms;

/// `kairo-sermon-plans.json` (`proautomate-sermon-plans` adopted).
pub struct SermonPlanStore {
    store: JsonStore,
}

impl SermonPlanStore {
    pub fn open(user_data: &std::path::Path) -> Self {
        let spec = kairo_store::settings::store_spec("kairo-sermon-plans")
            .expect("sermon plan store spec");
        Self {
            store: kairo_store::settings::open_store(user_data, spec),
        }
    }

    fn raw_plans(&self) -> Vec<Value> {
        self.store
            .get("plans")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    }

    fn plans(&self) -> Vec<SermonPlan> {
        self.raw_plans()
            .into_iter()
            .filter_map(|p| serde_json::from_value(p).ok())
            .collect()
    }
}

impl PlanSource for SermonPlanStore {
    fn get(&self, id: &str) -> Option<SermonPlan> {
        self.plans().into_iter().find(|p| p.id == id)
    }

    /// Installs that predate the key read back `undefined`.
    fn live_plan_id(&self) -> Option<String> {
        self.store
            .get("livePlanId")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    fn set_live_plan_id(&mut self, id: Option<&str>) {
        self.store.set(
            "livePlanId",
            id.map_or(Value::Null, |id| Value::String(id.into())),
        );
    }

    fn list(&self) -> Vec<SermonPlan> {
        let mut plans = self.plans();
        plans.sort_by(|a, b| b.updated_at.total_cmp(&a.updated_at));
        plans
    }

    /// Entries this build cannot parse stay in the file untouched.
    fn save(&mut self, plan: SermonPlan, now_ms: f64) -> SermonPlan {
        let saved = SermonPlan {
            updated_at: now_ms,
            ..plan
        };
        let value = serde_json::to_value(&saved).expect("plan serialises");
        let mut plans = self.raw_plans();
        match plans
            .iter()
            .position(|p| p.get("id").and_then(Value::as_str) == Some(saved.id.as_str()))
        {
            Some(index) => plans[index] = value,
            None => plans.insert(0, value),
        }
        self.store.set("plans", Value::Array(plans));
        saved
    }

    fn delete(&mut self, id: &str) {
        let plans: Vec<Value> = self
            .raw_plans()
            .into_iter()
            .filter(|p| p.get("id").and_then(Value::as_str) != Some(id))
            .collect();
        self.store.set("plans", Value::Array(plans));
        if self.live_plan_id().as_deref() == Some(id) {
            self.set_live_plan_id(None);
        }
    }
}

struct Inner {
    source: Box<dyn PlanSource>,
    plan: Option<SermonPlan>,
    index: Option<SermonPlanIndex>,
    listeners: kairo_core::Listeners<LivePlanState>,
}

#[derive(Clone)]
pub struct LivePlanService {
    inner: Arc<Mutex<Inner>>,
}

fn empty_state() -> LivePlanState {
    LivePlanState {
        next_reference: None,
        next_plan_item_id: None,
        plan_id: None,
        title: None,
        item_count: 0.0,
        unavailable_count: 0.0,
    }
}

impl LivePlanService {
    pub fn new(source: Box<dyn PlanSource>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                source,
                plan: None,
                index: None,
                listeners: kairo_core::Listeners::default(),
            })),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Restores the persisted selection at boot.
    pub fn init(&self) {
        let id = self.lock().source.live_plan_id();
        self.load(id, false);
    }

    pub fn set_plan(&self, plan_id: Option<&str>) -> LivePlanState {
        self.lock().source.set_live_plan_id(plan_id);
        self.load(plan_id.map(str::to_owned), true)
    }

    /// One stored plan, live or not (`sermonPlanStore.get`).
    pub fn plan(&self, plan_id: &str) -> Option<SermonPlan> {
        self.lock().source.get(plan_id)
    }

    /// `scripture:listSermonPlans`.
    pub fn list_plans(&self) -> Vec<SermonPlan> {
        self.lock().source.list()
    }

    /// `scripture:saveSermonPlan`: every playlist edit funnels through here,
    /// which is what keeps the live index from going stale.
    pub fn save_plan(&self, plan: SermonPlan) -> SermonPlan {
        let saved = self.lock().source.save(plan, now_ms());
        self.refresh(&saved.id);
        saved
    }

    /// `scripture:deleteSermonPlan`.
    pub fn delete_plan(&self, plan_id: &str) {
        self.lock().source.delete(plan_id);
        self.handle_deleted(plan_id);
    }

    /// Rebuilds after a save; a no-op unless `plan_id` is live.
    pub fn refresh(&self, plan_id: &str) {
        if self.lock().plan.as_ref().is_some_and(|p| p.id == plan_id) {
            self.load(Some(plan_id.into()), true);
        }
    }

    /// Clears the selection if the deleted plan was live.
    pub fn handle_deleted(&self, plan_id: &str) -> bool {
        if !self.lock().plan.as_ref().is_some_and(|p| p.id == plan_id) {
            return false;
        }
        self.load(None, true);
        true
    }

    pub fn index(&self) -> Option<SermonPlanIndex> {
        self.lock().index.clone()
    }

    pub fn with_index<R>(&self, f: impl FnOnce(Option<&SermonPlanIndex>) -> R) -> R {
        f(self.lock().index.as_ref())
    }

    pub fn observe(&self, reference: &str, item_id: Option<&str>) {
        let mut inner = self.lock();
        let Some(index) = inner.index.as_mut() else {
            return;
        };
        if !advance_plan_expectation(index, reference, item_id) {
            return;
        }
        let state = state_of(&inner);
        inner.listeners.notify(&state);
    }

    pub fn state(&self) -> LivePlanState {
        state_of(&self.lock())
    }

    pub fn on_change(&self, listener: impl Fn(&LivePlanState) + Send + Sync + 'static) {
        self.lock().listeners.add(listener);
    }

    fn load(&self, plan_id: Option<String>, notify: bool) -> LivePlanState {
        let mut inner = self.lock();
        // A dangling id resolves to none and heals on the next write.
        inner.plan = plan_id.and_then(|id| inner.source.get(&id));
        inner.index = inner.plan.as_ref().map(build_sermon_plan_index);
        let state = state_of(&inner);
        tracing::info!(plan = ?state.plan_id, "reference playlist updated");
        if notify {
            inner.listeners.notify(&state);
        }
        state
    }
}

fn state_of(inner: &Inner) -> LivePlanState {
    let Some(plan) = &inner.plan else {
        return empty_state();
    };
    let next = inner
        .index
        .as_ref()
        .and_then(|i| i.ordered.get(i.expected_index));
    LivePlanState {
        next_reference: Some(next.map(|e| e.reference.clone())),
        next_plan_item_id: Some(next.map(|e| e.plan_item_id.clone())),
        plan_id: Some(plan.id.clone()),
        title: Some(plan.title.clone()),
        item_count: plan.items.len() as f64,
        unavailable_count: plan
            .items
            .iter()
            .filter(|i| !i.available || i.verses.is_empty())
            .count() as f64,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[derive(Default)]
    pub struct MemoryPlans {
        pub plans: Vec<SermonPlan>,
        pub live: Option<String>,
    }

    impl PlanSource for MemoryPlans {
        fn get(&self, id: &str) -> Option<SermonPlan> {
            self.plans.iter().find(|p| p.id == id).cloned()
        }
        fn live_plan_id(&self) -> Option<String> {
            self.live.clone()
        }
        fn set_live_plan_id(&mut self, id: Option<&str>) {
            self.live = id.map(str::to_owned);
        }
        fn list(&self) -> Vec<SermonPlan> {
            self.plans.clone()
        }
        fn save(&mut self, plan: SermonPlan, now_ms: f64) -> SermonPlan {
            let saved = SermonPlan {
                updated_at: now_ms,
                ..plan
            };
            match self.plans.iter().position(|p| p.id == saved.id) {
                Some(i) => self.plans[i] = saved.clone(),
                None => self.plans.insert(0, saved.clone()),
            }
            saved
        }
        fn delete(&mut self, id: &str) {
            self.plans.retain(|p| p.id != id);
            if self.live.as_deref() == Some(id) {
                self.live = None;
            }
        }
    }

    pub fn plan(id: &str, refs: &[(&str, &str, f64, f64)]) -> SermonPlan {
        let items: Vec<_> = refs
            .iter()
            .enumerate()
            .map(|(i, (book, translation, chapter, verse))| {
                serde_json::json!({
                    "id": format!("item-{i}"),
                    "reference": format!("{book} {chapter}:{verse}"),
                    "translation": translation,
                    "verses": [{ "book": book, "chapter": chapter, "verse": verse, "text": "text" }],
                    "available": true,
                })
            })
            .collect();
        serde_json::from_value(serde_json::json!({
            "id": id, "title": "Sunday", "sourceFileName": "notes.docx",
            "items": items, "createdAt": 1, "updatedAt": 1,
        }))
        .unwrap()
    }

    #[test]
    fn selection_persists_and_progress_notifies() {
        let source = MemoryPlans {
            plans: vec![plan(
                "p",
                &[("John", "NKJV", 3.0, 16.0), ("Romans", "NKJV", 8.0, 28.0)],
            )],
            live: Some("p".into()),
        };
        let live = LivePlanService::new(Box::new(source));
        live.init();
        let state = live.state();
        assert_eq!(state.next_reference, Some(Some("John 3:16".into())));
        assert_eq!(state.item_count, 2.0);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        live.on_change(move |st| s.lock().unwrap().push(st.next_reference.clone()));
        live.observe("John 3:16", None);
        assert_eq!(
            *seen.lock().unwrap(),
            [Some(Some("Romans 8:28".to_string()))]
        );
        // Deleting the live plan clears it; another plan's deletion does not.
        assert!(!live.handle_deleted("other"));
        assert!(live.handle_deleted("p"));
        assert_eq!(live.state(), empty_state());
        assert!(live.index().is_none());
    }

    #[test]
    fn a_dangling_id_resolves_to_no_plan() {
        let live = LivePlanService::new(Box::new(MemoryPlans {
            plans: Vec::new(),
            live: Some("gone".into()),
        }));
        live.init();
        assert_eq!(live.state().plan_id, None);
        assert_eq!(live.set_plan(None).plan_id, None);
    }

    #[test]
    fn the_store_reads_and_writes_the_live_id() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SermonPlanStore::open(dir.path());
        assert_eq!(store.live_plan_id(), None);
        store.set_live_plan_id(Some("p"));
        assert_eq!(store.live_plan_id().as_deref(), Some("p"));
        assert!(store.list().is_empty());
    }

    #[test]
    fn the_store_saves_lists_and_deletes_like_electron() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SermonPlanStore::open(dir.path());
        store
            .store
            .set("plans", serde_json::json!([{"id": "broken"}]));
        let a = store.save(plan("a", &[("John", "KJV", 3.0, 16.0)]), 10.0);
        assert_eq!(a.updated_at, 10.0);
        store.save(plan("b", &[]), 20.0);
        let a2 = store.save(
            SermonPlan {
                title: "renamed".into(),
                ..a
            },
            30.0,
        );
        let listed = store.list();
        assert_eq!(
            listed.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(listed[0].title, "renamed");
        assert_eq!(listed[0], a2);
        // New plans go first in the file; unparseable entries are kept.
        let raw = store.raw_plans();
        let ids: Vec<_> = raw.iter().map(|p| p["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["b", "a", "broken"]);

        store.set_live_plan_id(Some("a"));
        store.delete("b");
        assert_eq!(store.live_plan_id().as_deref(), Some("a"));
        store.delete("a");
        assert_eq!(store.live_plan_id(), None);
        store.store.flush().unwrap();
        let reopened = SermonPlanStore::open(dir.path());
        assert!(reopened.list().is_empty());
        assert_eq!(reopened.raw_plans().len(), 1);
    }

    #[test]
    fn saving_and_deleting_through_the_service_refreshes_the_live_plan() {
        let live = LivePlanService::new(Box::new(MemoryPlans::default()));
        let saved = live.save_plan(plan("p", &[("John", "KJV", 3.0, 16.0)]));
        assert!(saved.updated_at > 0.0);
        live.set_plan(Some("p"));
        assert_eq!(live.state().item_count, 1.0);
        live.save_plan(plan(
            "p",
            &[("John", "KJV", 3.0, 16.0), ("John", "KJV", 3.0, 17.0)],
        ));
        assert_eq!(live.state().item_count, 2.0);
        assert_eq!(live.list_plans().len(), 1);
        live.delete_plan("p");
        assert_eq!(live.state().plan_id, None);
        assert!(live.list_plans().is_empty());
    }
}
