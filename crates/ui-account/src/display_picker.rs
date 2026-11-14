//! Which physical display an output fills (\`DisplayPicker\` in
//! \`components/screens/displays.tsx\`), as setup's screens page uses it: no
//! video-card group. A display another output already fills is listed but
//! disabled; a bound display that is unplugged stays selected.

use gpui_kit::component::select::{Select, SelectEvent, SelectState};
use gpui_kit::component::searchable_list::SearchableListItem;
use gpui_kit::{AppContext, Context, Entity, EventEmitter, IntoElement, ParentElement, Render, SharedString, Styled, Subscription, Window, div, prelude::FluentBuilder};
use gpui_kit::component::v_flex;
use kairo_domain::displays::{DisplayBinding, binding_for_display, describe_display, resolve_display};
use kairo_domain::overlay::DisplayInfo;
use kairo_ui_kit::KairoThemeExt;
use kairo_ui_kit::tokens::{TextStep, text};

/// Something bound to a display (\`DisplayBound\`).
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayBound {
    /// Its name, for "used by …".
    pub name: String,
    /// Its binding.
    pub binding: DisplayBinding,
}

/// A choice in the picker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pick {
    /// None.
    None,
    /// The bound display, not connected.
    Missing,
    /// A display, by id.
    Display(f64),
}

/// One row.
#[derive(Clone, Debug)]
pub struct PickItem {
    title: SharedString,
    value: Pick,
    disabled: bool,
}

impl SearchableListItem for PickItem {
    type Value = Pick;
    fn title(&self) -> SharedString {
        self.title.clone()
    }
    fn value(&self) -> &Pick {
        &self.value
    }
    fn disabled(&self) -> bool {
        self.disabled
    }
}

/// The rows and the selected one for \`target\` among \`displays\`, with
/// \`others\` already taking theirs.
pub fn picker_items(
    target: &DisplayBound,
    displays: &[DisplayInfo],
    others: &[DisplayBound],
) -> (Vec<PickItem>, Pick) {
    let bound = resolve_display(&target.binding, displays, &[]);
    let missing = target.binding.display_id.is_some() && bound.is_none();
    let mut items = vec![PickItem { title: "None".into(), value: Pick::None, disabled: false }];
    if missing {
        let label = if target.binding.display_label.trim().is_empty() {
            "Previous display".to_owned()
        } else {
            target.binding.display_label.clone()
        };
        items.push(PickItem {
            title: format!("{label} (not connected)").into(),
            value: Pick::Missing,
            disabled: true,
        });
    }
    for display in displays {
        let owner = others
            .iter()
            .find(|o| resolve_display(&o.binding, displays, &[]).is_some_and(|d| d.id == display.id));
        let mut tags = Vec::new();
        if display.hosts_main_window {
            tags.push("Kairo’s controls".to_owned());
        }
        if let Some(owner) = owner {
            tags.push(format!("used by {}", owner.name));
        }
        let mut title = describe_display(&display.label, &display.size, display.scale_factor);
        if !tags.is_empty() {
            title = format!("{title} — {}", tags.join(", "));
        }
        items.push(PickItem { title: title.into(), value: Pick::Display(display.id), disabled: owner.is_some() });
    }
    let selected = match (bound, missing) {
        (Some(display), _) => Pick::Display(display.id),
        (None, true) => Pick::Missing,
        _ => Pick::None,
    };
    (items, selected)
}

/// The binding to save for a pick, or \`None\` for a pick that saves nothing.
pub fn binding_for_pick(pick: Pick, displays: &[DisplayInfo]) -> Option<DisplayBinding> {
    match pick {
        Pick::None => Some(DisplayBinding { display_id: None, display_label: String::new(), display_size: None }),
        Pick::Missing => None,
        Pick::Display(id) => displays.iter().find(|d| d.id == id).map(binding_for_display),
    }
}

/// A new binding picked.
#[derive(Clone, Debug, PartialEq)]
pub struct Picked(pub DisplayBinding);

/// The picker.
pub struct DisplayPicker {
    id: SharedString,
    label: SharedString,
    select: Entity<SelectState<Vec<PickItem>>>,
    displays: Vec<DisplayInfo>,
    hosts_controls: bool,
    disabled: bool,
    _subscription: Subscription,
}

impl EventEmitter<Picked> for DisplayPicker {}

impl DisplayPicker {
    /// A picker for \`target\`.
    pub fn new(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        target: &DisplayBound,
        displays: &[DisplayInfo],
        others: &[DisplayBound],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (items, selected) = picker_items(target, displays, others);
        let index = items.iter().position(|i| i.value == selected).map(gpui_kit::component::IndexPath::new);
        let select = cx.new(|cx| SelectState::new(items, index, window, cx));
        let subscription = cx.subscribe(&select, |this, _, event: &SelectEvent<Vec<PickItem>>, cx| {
            let SelectEvent::Confirm(Some(pick)) = event else { return };
            if let Some(binding) = binding_for_pick(*pick, &this.displays) {
                cx.emit(Picked(binding));
            }
        });
        let hosts_controls = resolve_display(&target.binding, displays, &[]).is_some_and(|d| d.hosts_main_window);
        Self {
            id: id.into(),
            label: label.into(),
            select,
            displays: displays.to_vec(),
            hosts_controls,
            disabled: false,
            _subscription: subscription,
        }
    }

    /// Refreshes the rows.
    pub fn update(&mut self, target: &DisplayBound, displays: &[DisplayInfo], others: &[DisplayBound], window: &mut Window, cx: &mut Context<Self>) {
        let (items, selected) = picker_items(target, displays, others);
        let index = items.iter().position(|i| i.value == selected);
        self.displays = displays.to_vec();
        self.hosts_controls = resolve_display(&target.binding, displays, &[]).is_some_and(|d| d.hosts_main_window);
        self.select.update(cx, |select, cx| {
            select.set_items(items, window, cx);
            if let Some(index) = index {
                select.set_selected_index(Some(gpui_kit::component::IndexPath::new(index)), window, cx);
            }
        });
        cx.notify();
    }

    /// Disables it while a pick saves.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }
}

impl Render for DisplayPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let k = *cx.kairo();
        v_flex()
            .gap_1p5()
            .child(
                Select::new(&self.select)
                    .id(self.id.clone())
                    .accessibility_label(self.label.clone())
                    .disabled(self.disabled),
            )
            .when(self.hosts_controls, |el| {
                el.child(
                    div()
                        .text_size(text::XS.size)
                        .text_color(k.text(TextStep::T500))
                        .child("Opens as a window — Kairo’s controls are here."),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay::{DisplayBounds, Size};

    fn display(id: f64, label: &str, main: bool) -> DisplayInfo {
        DisplayInfo {
            id,
            label: label.into(),
            bounds: DisplayBounds { x: 0.0, y: 0.0, width: 1920.0, height: 1080.0 },
            size: Size { width: 1920.0, height: 1080.0, extra: Default::default() },
            scale_factor: 1.0,
            primary: main,
            hosts_main_window: main,
        }
    }

    fn bound(name: &str, id: Option<f64>, label: &str) -> DisplayBound {
        DisplayBound {
            name: name.into(),
            binding: DisplayBinding {
                display_id: id,
                display_label: label.into(),
                display_size: id.map(|_| Size { width: 1920.0, height: 1080.0, extra: Default::default() }),
            },
        }
    }

    #[test]
    fn taken_displays_are_disabled_and_named() {
        let displays = [display(1.0, "Built-in", true), display(2.0, "Projector", false)];
        let (items, selected) = picker_items(&bound("Audience screen", None, ""), &displays, &[bound("Stage display", Some(2.0), "Projector")]);
        assert_eq!(selected, Pick::None);
        assert_eq!(items[1].title.as_ref(), "Built-in · 1920×1080 — Kairo’s controls");
        assert_eq!(items[2].title.as_ref(), "Projector · 1920×1080 — used by Stage display");
        assert!(items[2].disabled && !items[1].disabled);
    }

    #[test]
    fn unplugged_binding_stays_selected() {
        let displays = [display(1.0, "Built-in", true)];
        let (items, selected) = picker_items(&bound("Audience screen", Some(9.0), "Old TV"), &displays, &[]);
        assert_eq!(selected, Pick::Missing);
        assert_eq!(items[1].title.as_ref(), "Old TV (not connected)");
        assert!(items[1].disabled);
        assert_eq!(binding_for_pick(Pick::Missing, &displays), None);
        assert_eq!(binding_for_pick(Pick::None, &displays).unwrap().display_id, None);
    }
}
