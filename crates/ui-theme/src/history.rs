//! Undo and redo for the theme open in the editor (`useThemeHistory.ts`).
//!
//! `record` is called with the theme as it was before each edit; `undo` and
//! `redo` hand back the theme to show. Edits closer together than
//! [`COALESCE_MS`] are one step, so a drag, a scrub or a slider is one undo.

use kairo_domain::overlay::OverlayTheme;

/// Edits closer together than this are one undo step.
pub const COALESCE_MS: u64 = 600;
/// Steps kept.
pub const MAX_STEPS: usize = 100;

#[derive(Clone, Debug, Default)]
pub struct ThemeHistory {
    past: Vec<OverlayTheme>,
    future: Vec<OverlayTheme>,
    /// When the last edit was recorded. `None` after an undo, a redo or a
    /// clear, so the next edit starts a fresh step.
    last_edit: Option<u64>,
}

impl ThemeHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Notes an edit made at `now_ms`; `before` is the theme it replaced.
    /// The window slides: each coalesced edit extends it.
    pub fn record(&mut self, before: &OverlayTheme, now_ms: u64) {
        let fresh = match self.last_edit {
            None => true,
            Some(last) => now_ms.saturating_sub(last) > COALESCE_MS,
        };
        if fresh {
            self.past.push(before.clone());
            if self.past.len() > MAX_STEPS {
                self.past.remove(0);
            }
        }
        self.last_edit = Some(now_ms);
        self.future.clear();
    }

    pub fn undo(&mut self, current: &OverlayTheme) -> Option<OverlayTheme> {
        let target = self.past.pop()?;
        self.future.push(current.clone());
        self.last_edit = None;
        Some(target)
    }

    pub fn redo(&mut self, current: &OverlayTheme) -> Option<OverlayTheme> {
        let target = self.future.pop()?;
        self.past.push(current.clone());
        self.last_edit = None;
        Some(target)
    }

    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
        self.last_edit = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay_defaults::DEFAULT_OVERLAY_THEME;

    fn theme(size: f64) -> OverlayTheme {
        let mut t = DEFAULT_OVERLAY_THEME.clone();
        t.verse.font_size_px = size;
        t
    }

    #[test]
    fn edits_inside_the_window_are_one_step_and_the_window_slides() {
        let mut h = ThemeHistory::new();
        h.record(&theme(1.0), 1_000);
        h.record(&theme(2.0), 1_500);
        h.record(&theme(3.0), 2_000);
        // 2_000 - 1_000 > 600, but each edit extends the window.
        assert_eq!(h.undo(&theme(4.0)).unwrap().verse.font_size_px, 1.0);
        assert!(!h.can_undo());
        assert!(h.can_redo());
    }

    #[test]
    fn a_pause_starts_a_new_step() {
        let mut h = ThemeHistory::new();
        h.record(&theme(1.0), 1_000);
        h.record(&theme(2.0), 1_601);
        assert_eq!(h.undo(&theme(3.0)).unwrap().verse.font_size_px, 2.0);
        assert_eq!(h.undo(&theme(2.0)).unwrap().verse.font_size_px, 1.0);
    }

    #[test]
    fn exactly_the_window_still_coalesces() {
        let mut h = ThemeHistory::new();
        h.record(&theme(1.0), 1_000);
        h.record(&theme(2.0), 1_600);
        assert_eq!(h.undo(&theme(3.0)).unwrap().verse.font_size_px, 1.0);
        assert!(!h.can_undo());
    }

    #[test]
    fn undo_then_edit_starts_a_fresh_step_and_drops_redo() {
        let mut h = ThemeHistory::new();
        h.record(&theme(1.0), 1_000);
        h.undo(&theme(2.0));
        assert!(h.can_redo());
        h.record(&theme(1.0), 1_100);
        assert!(!h.can_redo());
        assert!(h.can_undo());
    }

    #[test]
    fn redo_returns_what_undo_replaced() {
        let mut h = ThemeHistory::new();
        h.record(&theme(1.0), 0);
        let back = h.undo(&theme(2.0)).unwrap();
        assert_eq!(back.verse.font_size_px, 1.0);
        assert_eq!(h.redo(&back).unwrap().verse.font_size_px, 2.0);
        assert!(h.can_undo());
        assert!(!h.can_redo());
        assert!(h.redo(&theme(9.0)).is_none());
    }

    #[test]
    fn keeps_a_hundred_steps() {
        let mut h = ThemeHistory::new();
        for i in 0..150 {
            h.record(&theme(i as f64), i * 1_000);
        }
        let mut count = 0;
        let mut current = theme(150.0);
        while let Some(prev) = h.undo(&current) {
            current = prev;
            count += 1;
        }
        assert_eq!(count, MAX_STEPS);
        assert_eq!(current.verse.font_size_px, 50.0);
    }

    #[test]
    fn clear_forgets_everything() {
        let mut h = ThemeHistory::new();
        h.record(&theme(1.0), 0);
        h.clear();
        assert!(!h.can_undo() && !h.can_redo());
        assert!(h.undo(&theme(2.0)).is_none());
    }
}
