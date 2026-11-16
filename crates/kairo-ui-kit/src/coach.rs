//! Coach-tour anchors (`data-tour` and the selectors `CoachTour.tsx` uses).
//! A control marks itself with [`coach_anchor`] under a stable name; its
//! bounds are recorded each time it paints, and the tour reads them.
//!
//! A position stays good until the control paints somewhere else: cached
//! layers skip repainting unchanged controls, so a time limit would expire
//! targets that are still on screen (the tour flickered on and off). A move
//! asks for one more frame so the tour follows it.

use gpui_kit::{App, Bounds, Global, IntoElement, Pixels, Styled, Window, canvas, px};
use std::collections::HashMap;

/// Anchor names, in place of the source's selectors.
pub mod anchors {
    /// `nav[aria-label="Workspaces"]`: the rail.
    pub const WORKSPACES: &str = "workspaces";
    /// `[data-tour="transcript-start"]` (076).
    pub const TRANSCRIPT_START: &str = "transcript-start";
    /// `[data-tour="detected"]` (076).
    pub const DETECTED: &str = "detected";
    /// `[data-tour="automation"]` (076).
    pub const AUTOMATION: &str = "automation";
    /// `[data-tour="live-preview"]` (076).
    pub const LIVE_PREVIEW: &str = "live-preview";
    /// `.header-live`: Clear.
    pub const HEADER_LIVE: &str = "header-live";
    /// `.header-statuses > :first-child`: Screens.
    pub const HEADER_SCREENS: &str = "header-screens";
    /// `input[name="scripture-query"]` (078).
    pub const SCRIPTURE_QUERY: &str = "scripture-query";
    /// `[data-tour="lyrics-new"]` (080).
    pub const LYRICS_NEW: &str = "lyrics-new";
    /// `[data-tour="documents-import"]` (082).
    pub const DOCUMENTS_IMPORT: &str = "documents-import";
}

/// Where anchored controls last painted.
#[derive(Default)]
pub struct Anchors(HashMap<&'static str, Bounds<Pixels>>);
impl Global for Anchors {}

impl Anchors {
    /// The anchor's bounds, if it has painted with a size.
    pub fn get(&self, name: &str) -> Option<Bounds<Pixels>> {
        self.0
            .get(name)
            .filter(|b| b.size.width > px(0.) && b.size.height > px(0.))
            .copied()
    }

    /// Records `name` at `bounds`. True when that is new or moved, so the
    /// tour needs another frame to follow it.
    pub fn record(&mut self, name: &'static str, bounds: Bounds<Pixels>) -> bool {
        self.0.insert(name, bounds) != Some(bounds)
    }
}

/// Marks its parent as the coach tour's `name` target. Add it as a child of
/// the control's own element, which must be `relative`; it covers the
/// control, takes no input and changes no layout.
pub fn coach_anchor(name: &'static str) -> impl IntoElement {
    canvas(
        move |bounds, window: &mut Window, cx: &mut App| {
            if cx.default_global::<Anchors>().record(name, bounds) {
                // Refreshing is ignored mid-draw; ask once it is done.
                window.on_next_frame(|window, _| window.refresh());
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .inset_0()
}
