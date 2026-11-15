use crate::{ScreensWindow, model::binding};
use gpui_kit::component::Selectable;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{Disableable, Sizable};
use gpui_kit::*;
use kairo_domain::displays::{describe_display, resolve_display};
use kairo_domain::overlay::OverlayOutputKind;
use kairo_ui_kit::{
    KairoThemeExt,
    tokens::{TextStep, radius, text},
};

/// OS display bounds scaled uniformly into the arrangement band. Negative
/// origins and vertically stacked displays preserve their actual relationship.
pub fn tiles(
    displays: &[kairo_domain::overlay::DisplayInfo],
    width: f32,
    height: f32,
) -> Vec<(f32, f32, f32, f32)> {
    if displays.is_empty() {
        return vec![];
    }
    let left = displays
        .iter()
        .map(|d| d.bounds.x)
        .fold(f64::INFINITY, f64::min);
    let top = displays
        .iter()
        .map(|d| d.bounds.y)
        .fold(f64::INFINITY, f64::min);
    let right = displays
        .iter()
        .map(|d| d.bounds.x + d.bounds.width)
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = displays
        .iter()
        .map(|d| d.bounds.y + d.bounds.height)
        .fold(f64::NEG_INFINITY, f64::max);
    let scale = (width as f64 / (right - left).max(1.)).min(height as f64 / (bottom - top).max(1.));
    let x = (width as f64 - (right - left) * scale) / 2.;
    let y = (height as f64 - (bottom - top) * scale) / 2.;
    displays
        .iter()
        .map(|d| {
            (
                (x + (d.bounds.x - left) * scale) as f32,
                (y + (d.bounds.y - top) * scale) as f32,
                (d.bounds.width * scale - 6.).max(1.) as f32,
                (d.bounds.height * scale - 6.).max(1.) as f32,
            )
        })
        .collect()
}
impl ScreensWindow {
    pub(crate) fn arrangement(&self, cx: &mut Context<Self>) -> AnyElement {
        let s = self.snapshot.as_ref().unwrap();
        let k = *cx.kairo();
        if s.displays.is_empty() {
            return div()
                .h(px(170.))
                .flex()
                .items_center()
                .justify_center()
                .bg(k.surface_stage)
                .text_color(k.text(TextStep::T500))
                .child("Looking for displays…")
                .into_any_element();
        }
        let can_bind = self.selected.as_ref().is_some_and(|key| {
            key.starts_with("stage:")
                || s.settings
                    .overlay
                    .outputs
                    .iter()
                    .any(|o| o.id == *key && o.kind == OverlayOutputKind::Screen)
        });
        let positions = tiles(&s.displays, 480., 145.);
        let mut band = div().relative().w(px(480.)).h(px(145.));
        for (ix, (display, (x, y, w, h))) in s.displays.iter().zip(positions).enumerate() {
            let mut names = Vec::new();
            let mut selected = false;
            for output in &s.settings.overlay.outputs {
                if output.kind == OverlayOutputKind::Screen
                    && resolve_display(
                        &binding(
                            output.display_id,
                            &output.display_label,
                            &output.display_size,
                        ),
                        &s.displays,
                        &[],
                    )
                    .is_some_and(|d| d.id == display.id)
                {
                    selected |= self.selected.as_deref() == Some(&output.id);
                    names.push(format!(
                        "{}{}",
                        output.name,
                        if output.enabled { "" } else { " (off)" }
                    ));
                }
            }
            for stage in &s.settings.presentation.stage_displays {
                if resolve_display(
                    &binding(stage.display_id, &stage.display_label, &stage.display_size),
                    &s.displays,
                    &[],
                )
                .is_some_and(|d| d.id == display.id)
                {
                    selected |= self.selected.as_deref() == Some(&format!("stage:{}", stage.id));
                    names.push(format!(
                        "Stage · {}{}",
                        stage.name,
                        if stage.enabled { "" } else { " (off)" }
                    ));
                }
            }
            let display = display.clone();
            let label = format!(
                "{}\n{}{}\n{}",
                ix + 1,
                describe_display(&display.label, &display.size, display.scale_factor),
                if display.hosts_main_window {
                    " · Kairo controls"
                } else {
                    ""
                },
                names.join(" · ")
            );
            band = band.child(
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(w))
                    .h(px(h))
                    .rounded(radius::md())
                    .bg(k.surface_display)
                    .child(
                        Button::new(SharedString::from(format!(
                            "screens-display-{}",
                            display.id
                        )))
                        .ghost()
                        .size_full()
                        .small()
                        .label(label)
                        .selected(selected)
                        .disabled(!can_bind)
                        .on_click(cx.listener(
                            move |t, _, window, cx| t.bind(Some(display.clone()), window, cx),
                        )),
                    ),
            );
        }
        div()
            .w_full()
            .h(px(185.))
            .flex()
            .items_center()
            .justify_center()
            .bg(k.surface_stage)
            .text_size(text::XS.size)
            .child(band)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::tiles;
    use kairo_domain::overlay::{DisplayBounds, DisplayInfo, Size};
    #[test]
    fn negative_origins_and_vertical_layout_keep_one_uniform_scale() {
        let display = |id, x, y, w, h| DisplayInfo {
            id,
            label: String::new(),
            bounds: DisplayBounds {
                x,
                y,
                width: w,
                height: h,
            },
            size: Size {
                width: w,
                height: h,
                extra: Default::default(),
            },
            scale_factor: 1.,
            primary: false,
            hosts_main_window: false,
        };
        let displays = [
            display(1., -1920., -1080., 1920., 1080.),
            display(2., 0., 0., 1920., 1080.),
        ];
        let rects = tiles(&displays, 480., 270.);
        assert!(rects[0].0 < rects[1].0 && rects[0].1 < rects[1].1);
        assert_eq!(rects[0].2, rects[1].2);
        assert_eq!(rects[0].3, rects[1].3);
        assert!(tiles(&[], 480., 270.).is_empty());
    }
}
