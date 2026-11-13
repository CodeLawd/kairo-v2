//! `SegmentedControl` (`components/shared/SegmentedControl.tsx`): a darker
//! strip with the chosen option lifted. The one pill style for choices and
//! tabs across Theme, Screens and the booth.

use crate::theme::KairoThemeExt;
use crate::tokens::{TextStep, text};
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::h_flex;
use gpui_kit::{
    App, ElementId, FocusHandle, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    RenderOnce, Role, SharedString, StatefulInteractiveElement, Styled, Toggled, Window, div,
    prelude::FluentBuilder, relative,
};
use std::rc::Rc;

/// What the options do: set a value, or switch panels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SegmentRole {
    /// `radiogroup`: the options set a value.
    #[default]
    RadioGroup,
    /// `tablist`: the options switch panels.
    TabList,
}

/// One option: a stable key and its label.
#[derive(Clone, Debug, PartialEq)]
pub struct SegmentOption {
    key: SharedString,
    label: SharedString,
}

impl SegmentOption {
    /// An option with a key and label.
    pub fn new(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
        }
    }

    /// The key passed to `on_change`.
    pub fn key(&self) -> &SharedString {
        &self.key
    }
}

type ChangeHandler = Rc<dyn Fn(&SharedString, &mut Window, &mut App)>;

/// Pick one of a few options.
#[derive(IntoElement)]
pub struct SegmentedControl {
    id: ElementId,
    label: SharedString,
    options: Vec<SegmentOption>,
    selected: Option<SharedString>,
    role: SegmentRole,
    fit: bool,
    compact: bool,
    on_change: Option<ChangeHandler>,
}

impl SegmentedControl {
    /// A control with an accessible name for the group.
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            options: Vec::new(),
            selected: None,
            role: SegmentRole::RadioGroup,
            fit: false,
            compact: false,
            on_change: None,
        }
    }

    /// Adds options.
    pub fn options(mut self, options: impl IntoIterator<Item = SegmentOption>) -> Self {
        self.options.extend(options);
        self
    }

    /// The chosen key.
    pub fn selected(mut self, key: impl Into<SharedString>) -> Self {
        self.selected = Some(key.into());
        self
    }

    /// `tablist` instead of `radiogroup`.
    pub fn tabs(mut self) -> Self {
        self.role = SegmentRole::TabList;
        self
    }

    /// Never truncate a label: each option is at least as wide as its label
    /// and spare room is shared. The default is equal widths, truncating.
    pub fn fit(mut self) -> Self {
        self.fit = true;
        self
    }

    /// Smaller padding and 11px text, for more than four options
    /// (`LabeledSegmented` in `theme/fields.tsx`).
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Called with the key of the option picked.
    pub fn on_change(
        mut self,
        handler: impl Fn(&SharedString, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

/// The key after `current` when moving by `delta`, wrapping at the ends.
pub fn step_key(
    options: &[SegmentOption],
    current: Option<&str>,
    delta: isize,
) -> Option<SharedString> {
    if options.is_empty() {
        return None;
    }
    let len = options.len() as isize;
    let index = current
        .and_then(|key| options.iter().position(|o| o.key.as_ref() == key))
        .map_or(0, |ix| (ix as isize + delta).rem_euclid(len));
    Some(options[index as usize].key.clone())
}

impl RenderOnce for SegmentedControl {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let item_role = match self.role {
            SegmentRole::RadioGroup => Role::RadioButton,
            SegmentRole::TabList => Role::Tab,
        };
        let group_role = match self.role {
            SegmentRole::RadioGroup => Role::RadioGroup,
            SegmentRole::TabList => Role::TabList,
        };
        let handles: Vec<FocusHandle> = self
            .options
            .iter()
            .map(|option| {
                window
                    .use_keyed_state(
                        ElementId::NamedChild(
                            std::sync::Arc::new(self.id.clone()),
                            option.key.clone(),
                        ),
                        cx,
                        |_, cx| cx.focus_handle(),
                    )
                    .read(cx)
                    .clone()
            })
            .collect();
        let options = Rc::new(self.options);
        let handles = Rc::new(handles);
        let size = if self.compact { text::LABEL } else { text::XS };

        h_flex()
            .id(self.id.clone())
            .test_support()
            .role(group_role)
            .aria_label(self.label.clone())
            .rounded(crate::tokens::radius::md())
            .bg(k.surface)
            .p_0p5()
            .children(options.iter().enumerate().map(|(ix, option)| {
                let selected = self.selected.as_ref() == Some(&option.key);
                let key = option.key.clone();
                let on_change = self.on_change.clone();
                let on_key = self.on_change.clone();
                let key_options = options.clone();
                let key_handles = handles.clone();
                let current = option.key.clone();
                div()
                    .id(ElementId::NamedChild(
                        std::sync::Arc::new(self.id.clone()),
                        option.key.clone(),
                    ))
                    .test_support()
                    .track_focus(&handles[ix])
                    .tab_index(0)
                    .role(item_role)
                    .aria_label(option.label.clone())
                    .map(|this| match item_role {
                        Role::Tab => this.aria_selected(selected),
                        _ => this.aria_toggled(if selected {
                            Toggled::True
                        } else {
                            Toggled::False
                        }),
                    })
                    .map(|this| {
                        if self.fit {
                            this.flex_auto().flex_shrink_0().whitespace_nowrap()
                        } else {
                            this.min_w_0().flex_1().truncate()
                        }
                    })
                    .flex()
                    .justify_center()
                    .rounded(crate::tokens::radius::base())
                    .when(self.compact, |this| this.px_1p5())
                    .when(!self.compact, |this| this.px_2())
                    .py_1()
                    .text_size(size.size)
                    .line_height(relative(1.25))
                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                    .cursor_default()
                    .map(|this| {
                        if selected {
                            this.bg(k.surface_elevated).text_color(k.text_primary)
                        } else {
                            this.text_color(k.text(TextStep::T500))
                                .hover(|s| s.text_color(k.text(TextStep::T300)))
                        }
                    })
                    .focus_visible(|s| s.border_1().border_color(k.focus_ring()))
                    .child(option.label.clone())
                    .on_click(move |_, window, cx| {
                        if let Some(handler) = &on_change {
                            handler(&key, window, cx);
                        }
                    })
                    .on_key_down(move |event: &KeyDownEvent, window, cx| {
                        let delta = match event.keystroke.key.as_str() {
                            "left" | "up" => -1,
                            "right" | "down" => 1,
                            "enter" | "space" => 0,
                            _ => return,
                        };
                        cx.stop_propagation();
                        let Some(next) = step_key(&key_options, Some(&current), delta) else {
                            return;
                        };
                        if let Some(ix) = key_options.iter().position(|o| o.key == next) {
                            key_handles[ix].focus(window, cx);
                        }
                        if let Some(handler) = &on_key {
                            handler(&next, window, cx);
                        }
                    })
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Vec<SegmentOption> {
        ["a", "b", "c"]
            .map(|k| SegmentOption::new(k, k.to_uppercase()))
            .to_vec()
    }

    #[test]
    fn arrows_wrap_at_both_ends() {
        assert_eq!(step_key(&opts(), Some("c"), 1).unwrap(), "a");
        assert_eq!(step_key(&opts(), Some("a"), -1).unwrap(), "c");
        assert_eq!(step_key(&opts(), Some("b"), 0).unwrap(), "b");
    }

    #[test]
    fn unknown_or_empty_selection_starts_at_the_first_option() {
        assert_eq!(step_key(&opts(), Some("zz"), 1).unwrap(), "a");
        assert_eq!(step_key(&opts(), None, 1).unwrap(), "a");
        assert!(step_key(&[], Some("a"), 1).is_none());
    }
}
