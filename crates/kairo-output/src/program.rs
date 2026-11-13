//! The program service (`program-service.ts`).
//!
//! What is happening on the program now, beyond the slide: the audience
//! message, the stage-only message, props, logo, camera, web page, timers,
//! and the current and next slide the stage displays mirror. Never
//! persisted: a restart comes up clean. Every change is published as the
//! `ProgramState` topic (`program:state`) and handed to the surface manager.

use crate::ports::OutputSettings;
use kairo_core::Publisher;
use kairo_domain::audience::PollView;
use kairo_domain::program::{
    ProgramQuestion, ProgramSlideInfo, ProgramState, ProgramTimerDef, ProgramTimerState,
    ProgramWeb, TimerKind, idle_timer, next_time_of_day, pause_timer, reset_timer,
    screen_timer_def, start_timer_for,
};
use kairo_domain::web::web_address;
use kairo_orchestrator::clients::{ProgramClient, ScriptureSlideMeta};
use kairo_orchestrator::voice::LiveScriptureContext;
use std::sync::{Arc, Mutex};

/// Where each new state goes: the surface manager, in order.
pub type ProgramSink = Box<dyn Fn(ProgramState) + Send + Sync>;

/// Which program layer `setOnScreens` turns on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnScreens {
    Countdown,
    Clock,
}

struct Inner {
    state: ProgramState,
    scripture: Option<LiveScriptureContext>,
    scripture_revision: i64,
    /// When the live verse went up, so spoken commands can tell it from a newer detection.
    scripture_at: f64,
}

pub struct ProgramService {
    inner: Mutex<Inner>,
    settings: Arc<dyn OutputSettings>,
    topics: Publisher,
    sink: ProgramSink,
    now_ms: fn() -> f64,
}

fn local_next(target: &str, now: f64) -> f64 {
    next_time_of_day(target, now, &chrono::Local)
}

impl ProgramService {
    pub fn new(settings: Arc<dyn OutputSettings>, topics: Publisher, sink: ProgramSink) -> Self {
        topics.publish(ProgramState::default());
        Self {
            inner: Mutex::new(Inner {
                state: ProgramState::default(),
                scripture: None,
                scripture_revision: 0,
                scripture_at: 0.,
            }),
            settings,
            topics,
            sink,
            now_ms: kairo_domain::js::now_ms,
        }
    }

    pub fn with_clock(mut self, now_ms: fn() -> f64) -> Self {
        self.now_ms = now_ms;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `program:getState`.
    pub fn state(&self) -> ProgramState {
        self.lock().state.clone()
    }

    fn set(&self, patch: impl FnOnce(&mut ProgramState)) -> ProgramState {
        let state = {
            let mut inner = self.lock();
            patch(&mut inner.state);
            inner.state.clone()
        };
        self.topics.publish(state.clone());
        (self.sink)(state.clone());
        state
    }

    // ─── Messages ───────────────────────────────────────────────────────────

    pub fn show_message(&self, text: &str) -> ProgramState {
        let message: String = text.trim().chars().take(1000).collect();
        self.set(|s| s.message = (!message.is_empty()).then_some(message))
    }

    pub fn clear_message(&self) -> ProgramState {
        self.set(|s| s.message = None)
    }

    pub fn show_poll(&self, poll: PollView) -> ProgramState {
        self.set(|s| s.poll = Some(poll))
    }

    pub fn clear_poll(&self) -> ProgramState {
        self.set(|s| s.poll = None)
    }

    pub fn show_question(&self, question: ProgramQuestion) -> ProgramState {
        self.set(|s| s.question = Some(question))
    }

    pub fn clear_question(&self) -> ProgramState {
        self.set(|s| s.question = None)
    }

    pub fn set_stage_message(&self, text: Option<&str>) -> ProgramState {
        let message: String = text.unwrap_or("").trim().chars().take(500).collect();
        self.set(|s| s.stage_message = (!message.is_empty()).then_some(message))
    }

    // ─── Logo, props, camera, web ───────────────────────────────────────────

    pub fn set_prop(&self, id: &str, on: bool) -> Result<ProgramState, String> {
        let known = self
            .settings
            .presentation()
            .props
            .iter()
            .any(|p| p.id == id);
        if on && !known {
            return Err("That prop no longer exists.".into());
        }
        Ok(self.set(|s| {
            s.active_prop_ids.retain(|p| p != id);
            if on {
                s.active_prop_ids.push(id.into());
            }
        }))
    }

    pub fn clear_props(&self) -> ProgramState {
        self.set(|s| s.active_prop_ids.clear())
    }

    /// Props removed from settings stop being active too.
    pub fn prune_props(&self) {
        let ids: Vec<String> = self
            .settings
            .presentation()
            .props
            .into_iter()
            .map(|p| p.id)
            .collect();
        let state = self.state();
        if state.active_prop_ids.iter().any(|id| !ids.contains(id)) {
            self.set(|s| s.active_prop_ids.retain(|id| ids.contains(id)));
        }
    }

    pub fn set_logo(&self, on: bool) -> ProgramState {
        self.set(|s| s.logo = on)
    }

    /// `audio_label`: the audio input played with the camera; `None` keeps it silent.
    pub fn set_camera(&self, label: Option<&str>, audio_label: Option<&str>) -> ProgramState {
        let camera = label
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_owned);
        let audio = camera.as_ref().and(
            audio_label
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(str::to_owned),
        );
        self.set(|s| {
            s.camera = camera;
            s.camera_audio = audio;
        })
    }

    /// A web page on screens. Only http(s): a screen never loads a local file.
    pub fn set_web(&self, url: Option<&str>) -> Result<ProgramState, String> {
        let Some(url) = url else {
            return Ok(self.set(|s| s.web = None));
        };
        let address = web_address(url, false).ok_or("That isn\u{2019}t a web address.")?;
        if self.state().web.is_some_and(|w| w.url == address) {
            return Ok(self.state());
        }
        Ok(self.set(|s| {
            s.web = Some(ProgramWeb {
                url: address,
                reload: 0.,
            })
        }))
    }

    pub fn reload_web(&self) -> ProgramState {
        if self.state().web.is_none() {
            return self.state();
        }
        self.set(|s| {
            if let Some(web) = &mut s.web {
                web.reload += 1.;
            }
        })
    }

    // ─── Timers ─────────────────────────────────────────────────────────────

    fn timer_defs(&self) -> Vec<ProgramTimerDef> {
        self.settings.presentation().timers
    }

    /// The saved timer `id`, or, with no id or one no longer saved, the one on screens.
    fn timer_def(&self, id: Option<&str>) -> ProgramTimerDef {
        let defs = self.timer_defs();
        let state = self.state();
        defs.iter()
            .find(|t| Some(t.id.as_str()) == id)
            .cloned()
            .unwrap_or_else(|| screen_timer_def(&defs, state.screen_timer_id.as_deref()).clone())
    }

    fn timer_run(&self, def: &ProgramTimerDef) -> ProgramTimerState {
        self.state()
            .timers
            .get(&def.id)
            .cloned()
            .unwrap_or_else(|| idle_timer(def, (self.now_ms)(), local_next))
    }

    /// Stores one timer's run, and keeps `timer` a copy of the one on screens.
    fn set_timer_run(&self, def: &ProgramTimerDef, run: ProgramTimerState) -> ProgramState {
        let defs = self.timer_defs();
        self.set(|s| {
            s.timers.insert(def.id.clone(), run);
            let on_screens = screen_timer_def(&defs, s.screen_timer_id.as_deref());
            if let Some(run) = s.timers.get(&on_screens.id) {
                s.timer = run.clone();
            }
        })
    }

    pub fn set_timer(&self, duration_sec: f64, id: Option<&str>) -> ProgramState {
        let def = self.timer_def(id);
        let run = if def.kind == TimerKind::CountdownTo {
            idle_timer(&def, (self.now_ms)(), local_next)
        } else {
            reset_timer(&self.timer_run(&def), Some(duration_sec))
        };
        self.set_timer_run(&def, run)
    }

    pub fn start_timer(&self, id: Option<&str>) -> ProgramState {
        let def = self.timer_def(id);
        let run = start_timer_for(&def, &self.timer_run(&def), (self.now_ms)(), local_next);
        self.set_timer_run(&def, run)
    }

    pub fn pause_timer(&self, id: Option<&str>) -> ProgramState {
        let def = self.timer_def(id);
        let run = pause_timer(&self.timer_run(&def), (self.now_ms)());
        self.set_timer_run(&def, run)
    }

    pub fn reset_timer(&self, id: Option<&str>) -> ProgramState {
        let def = self.timer_def(id);
        let run = idle_timer(&def, (self.now_ms)(), local_next);
        self.set_timer_run(&def, run)
    }

    /// Makes `id` the timer screens and stage displays show.
    pub fn show_timer(&self, id: &str) -> ProgramState {
        let Some(def) = self.timer_defs().into_iter().find(|t| t.id == id) else {
            return self.state();
        };
        let run = self.timer_run(&def);
        self.set(|s| {
            s.screen_timer_id = Some(def.id.clone());
            s.timer = run;
        })
    }

    pub fn set_on_screens(&self, layer: OnScreens, on: bool) -> ProgramState {
        self.set(|s| match layer {
            OnScreens::Countdown => s.countdown_on_screens = on,
            OnScreens::Clock => s.clock_on_screens = on,
        })
    }

    // ─── Current / next ─────────────────────────────────────────────────────

    fn forget_scripture(&self) {
        let mut inner = self.lock();
        inner.scripture = None;
        inner.scripture_revision += 1;
    }

    pub fn set_slide_info(
        &self,
        current: Option<ProgramSlideInfo>,
        next: Option<ProgramSlideInfo>,
    ) {
        self.forget_scripture();
        self.set(|s| {
            s.current = current;
            s.next = next;
        });
    }

    /// "Clear all": the audience message, props, logo, camera and web page
    /// come down with the slide. The stage message and the countdown are
    /// the booth's, not the room's, so they stay.
    pub fn clear_all(&self) {
        self.forget_scripture();
        self.set(|s| {
            s.poll = None;
            s.question = None;
            s.message = None;
            s.active_prop_ids.clear();
            s.logo = false;
            s.camera = None;
            s.camera_audio = None;
            s.web = None;
            s.current = None;
            s.next = None;
        });
    }
}

impl ProgramClient for ProgramService {
    fn scripture(&self) -> Option<(LiveScriptureContext, f64)> {
        let inner = self.lock();
        inner.scripture.clone().map(|c| (c, inner.scripture_at))
    }

    fn set_scripture_slide(&self, reference: String, text: String, meta: ScriptureSlideMeta) {
        {
            let mut inner = self.lock();
            inner.scripture_revision += 1;
            inner.scripture = Some(LiveScriptureContext {
                book: meta.book,
                chapter: meta.chapter,
                verse_start: meta.verse_start,
                verse_end: meta.verse_end,
                translation: meta.translation,
                revision: inner.scripture_revision,
                passage_id: Some(meta.passage_id),
                passage_index: Some(meta.passage_index),
            });
            inner.scripture_at = (self.now_ms)();
        }
        self.set(|s| {
            s.current = Some(ProgramSlideInfo { reference, text });
            s.next = None;
        });
    }

    fn set_slide(&self, reference: String, text: String, next: Option<ProgramSlideInfo>) {
        self.set_slide_info(Some(ProgramSlideInfo { reference, text }), next);
    }

    /// Text came down; the stage keeps showing what is next.
    fn clear_current(&self) {
        self.forget_scripture();
        if self.state().current.is_some() {
            self.set(|s| s.current = None);
        }
    }

    fn clear_program(&self) {
        self.clear_all();
    }
}
