//! The launch screen and the startup warning bar (\`LoadingScreen.tsx\`,
//! \`AnimatedMark.tsx\` and the warning in \`App.tsx\`).
//!
//! The splash shows on every launch: the lockup builds itself, the tagline
//! and a "Powering up" line settle in, and if loading outlasts the brand
//! beat the line becomes a real progress bar naming the step still loading.
//! Every animation is a function of the time since the splash appeared, so
//! the shell drives it with animation frames and stops them under reduced
//! motion, which shows the finished lockup and plain text.

use crate::bootstrap::bootstrap_percent;
use crate::shell::Shell;
use gpui_kit::base::TestSupportExt;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::{
    AnyElement, App, Context, Hsla, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    Role, SharedString, StatefulInteractiveElement, Styled, Window, div, px, relative,
};
use kairo_domain::status::BootstrapProgress;
use kairo_ui_kit::brand::{PRODUCT_NAME, PRODUCT_TAGLINE};
use kairo_ui_kit::launch_mark::AnimatedLockup;
use kairo_ui_kit::motion::{self, Frame, Keyframes};
use kairo_ui_kit::tokens::{TextStep, text};
use kairo_ui_kit::{KairoIcon, KairoThemeExt};

/// Element ids, for tests.
pub mod ids {
    /// The launch screen.
    pub const SCREEN: &str = "launch-screen";
    /// The progress bar's row.
    pub const PROGRESS: &str = "launch-progress";
    /// The step being loaded.
    pub const STEP: &str = "launch-step";
    /// The "Powering up" line.
    pub const BOOT: &str = "launch-boot";
    /// The startup warning bar.
    pub const WARNING: &str = "startup-warning";
    /// Its Retry button.
    pub const RETRY: &str = "startup-warning-retry";
    /// Its dismiss button.
    pub const DISMISS: &str = "startup-warning-dismiss";
}

/// The line under the lockup before progress shows.
pub const BOOT_TEXT: &str = "Powering up";

/// \`.splash-copy\`'s delay.
const COPY_DELAY_MS: f32 = 120.0;

/// The frame of a one-shot animation \`elapsed\` ms after it was applied.
fn once(keyframes: &Keyframes, elapsed: f32, delay: f32, still: bool) -> Frame {
    if still {
        return keyframes.end();
    }
    keyframes.sample(((elapsed - delay) / keyframes.duration_ms as f32).clamp(0.0, 1.0))
}

fn place<E: Styled>(element: E, frame: Frame) -> E {
    element
        .opacity(frame.opacity.clamp(0.0, 1.0))
        .relative()
        .top(px(frame.y))
        .left(px(frame.x))
}

/// Uppercase text with letter spacing, laid out a character at a time
/// (GPUI has no \`letter-spacing\`). \`color\` gives each character's
/// colour from its centre as a fraction of the line.
fn tracked(
    label: &str,
    size: gpui_kit::Pixels,
    tracking_em: f32,
    color: impl Fn(f32) -> Hsla,
) -> gpui_kit::Div {
    let chars: Vec<char> = label.to_uppercase().chars().collect();
    let count = chars.len().max(1) as f32;
    let gap = px(f32::from(size) * tracking_em);
    h_flex().children(chars.into_iter().enumerate().map(|(i, c)| {
        // A space would collapse on its own.
        let c = if c == ' ' { '\u{a0}' } else { c };
        div()
            .mr(gap)
            .text_color(color((i as f32 + 0.5) / count))
            .child(SharedString::from(c.to_string()))
    }))
}

/// The light band's strength at \`x\` (fraction of the line) when the
/// 250%-wide gradient sits at \`position\` (\`background-position-x\`, 1 → 0).
pub fn boot_band(position: f32, x: f32) -> f32 {
    // The band's peak is the gradient's middle: 1.25 W − 1.5 W · position.
    let centre = 1.25 - 1.5 * position;
    // White at 50%, muted from 40% and 60% of a 2.5 W gradient: ±0.25 W.
    (1.0 - (x - centre).abs() / 0.25).max(0.0)
}

fn mix(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (gpui_kit::Rgba::from(a), gpui_kit::Rgba::from(b));
    let l = |x: f32, y: f32| x + (y - x) * t;
    Hsla::from(gpui_kit::Rgba {
        r: l(a.r, b.r),
        g: l(a.g, b.g),
        b: l(a.b, b.b),
        a: l(a.a, b.a),
    })
}

/// The launch screen as it is \`elapsed_ms\` after it appeared.
#[derive(IntoElement)]
pub struct LaunchScreen {
    progress: BootstrapProgress,
    show_progress: bool,
    elapsed_ms: f32,
    step_ms: f32,
}

impl LaunchScreen {
    /// The screen. \`step_ms\` is the time since the step last changed.
    pub fn new(
        progress: BootstrapProgress,
        show_progress: bool,
        elapsed_ms: f32,
        step_ms: f32,
    ) -> Self {
        Self {
            progress,
            show_progress,
            elapsed_ms,
            step_ms,
        }
    }
}

impl RenderOnce for LaunchScreen {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let k = *cx.kairo();
        let still = cx.reduced_motion();
        let t = self.elapsed_ms;
        let copy = once(&motion::SPLASH_COPY_IN, t, COPY_DELAY_MS, still);
        let percent = bootstrap_percent(&self.progress);
        let primary = gpui_kit::component::ActiveTheme::theme(cx).primary;

        let tagline = place(div(), copy).child(
            tracked(
                PRODUCT_TAGLINE,
                text::MICRO.size.to_pixels(text::rem()),
                0.16,
                |_| k.text(TextStep::T300),
            )
            .font_weight(gpui_kit::FontWeight::SEMIBOLD)
            .text_size(text::MICRO.size),
        );

        // One fixed-height row: the boot line normally, real progress only
        // when loading outlasts the splash, so the logo never shifts.
        let row = if self.show_progress {
            let step = place(
                div(),
                once(&motion::SPLASH_STEP_IN, self.step_ms, 0.0, still),
            )
            .id(ids::STEP)
            .test_support()
            .min_w_0()
            .flex_1()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_size(px(12.))
            .font_weight(gpui_kit::FontWeight::MEDIUM)
            .text_color(k.text(TextStep::T300))
            .child(SharedString::from(self.progress.step.clone()));
            v_flex()
                .id(ids::PROGRESS)
                .test_support()
                .role(Role::ProgressIndicator)
                .aria_label(SharedString::from(format!("{percent}%")))
                .w_full()
                .child(
                    div()
                        .h(px(4.))
                        .w_full()
                        .overflow_hidden()
                        .bg(k.surface_elevated)
                        .child(
                            div()
                                .h_full()
                                .w(relative(percent as f32 / 100.0))
                                .bg(primary),
                        ),
                )
                .child(
                    h_flex()
                        .mt(px(10.))
                        .items_baseline()
                        .justify_between()
                        .gap_3()
                        .child(step)
                        .child(
                            div()
                                .flex_shrink_0()
                                .font_family(text::MONO_FAMILY)
                                .text_size(px(12.))
                                .text_color(k.text(TextStep::T400))
                                .child(SharedString::from(format!("{percent}%"))),
                        ),
                )
                .into_any_element()
        } else {
            // "Powering up": a light band running through muted text.
            let band = if still {
                None
            } else {
                let cycle = motion::SPLASH_BOOT.duration_ms as f32;
                Some(motion::SPLASH_BOOT.sample((t % cycle) / cycle).background_x)
            };
            let base = k.splash_boot;
            let muted = k.text(TextStep::T500);
            let white = k.white;
            h_flex()
                .id(ids::BOOT)
                .test_support()
                .aria_label(SharedString::new_static(BOOT_TEXT))
                .w_full()
                .justify_center()
                .font_family(text::MONO_FAMILY)
                .text_size(px(11.))
                .child(
                    tracked(BOOT_TEXT, px(11.), 0.3, move |x| match band {
                        Some(position) => mix(base, white, boot_band(position, x)),
                        None => muted,
                    })
                    .text_size(px(11.)),
                )
                .into_any_element()
        };

        div()
            .id(ids::SCREEN)
            .test_support()
            .role(Role::Status)
            .aria_label(SharedString::new_static(PRODUCT_NAME))
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .bg(k.surface)
            .child(
                v_flex()
                    .w(px(340.))
                    .items_center()
                    .gap(px(28.))
                    // The lockup builds itself (mark, then "Kairo") and stays
                    // alive while Kairo loads.
                    .child(AnimatedLockup::new(px(300.), t).still(still))
                    .child(tagline)
                    .child(place(div(), copy).h(px(30.)).w_full().child(row)),
            )
    }
}

impl Shell {
    /// The splash, while the app is not ready.
    pub(crate) fn render_launch(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let now = cx.background_executor().now();
        let elapsed = self.launch_epoch.map_or(0.0, |epoch| {
            now.saturating_duration_since(epoch).as_secs_f32() * 1000.0
        });
        let step_ms = now.saturating_duration_since(self.step_since).as_secs_f32() * 1000.0;
        if !kairo_ui_kit::KairoThemeExt::reduced_motion(&**cx) && self.launch_epoch.is_some() {
            // The intro has played out once a frame past it is drawn: the
            // splash may close a beat later (\`onAnimationEnd\`).
            if elapsed >= kairo_ui_kit::launch_mark::LOCKUP_INTRO_MS as f32
                && self.launch.mark_intro_done()
            {
                self.schedule_hold(crate::bootstrap::INTRO_BEAT_MS, cx);
            }
            window.request_animation_frame();
        }
        LaunchScreen::new(
            self.launch.current_progress().clone(),
            self.launch.show_progress(),
            elapsed,
            step_ms,
        )
        .into_any_element()
    }

    /// The startup warning, over the top of the app, while it should show.
    pub(crate) fn render_startup_warning(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let message = self.launch.warning()?;
        let k = *cx.kairo();
        let hover = k.tints.yellow;
        Some(
            h_flex()
                .id(ids::WARNING)
                .test_support()
                .role(Role::Alert)
                .aria_label(SharedString::from(message.clone()))
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .items_start()
                .gap(px(10.))
                .bg(k.surface_elevated)
                .px_4()
                .py(px(10.))
                .text_size(text::XS.size)
                .text_color(k.warning_text)
                .child(
                    Icon::new(KairoIcon::AlertTriangle)
                        .size(px(14.))
                        .mt(px(1.))
                        .flex_shrink_0(),
                )
                .child(div().flex_1().child(SharedString::from(message)))
                .child(
                    h_flex()
                        .id(ids::RETRY)
                        .test_support()
                        .role(Role::Button)
                        .aria_label(SharedString::new_static("Retry"))
                        .flex_shrink_0()
                        .items_center()
                        .gap_1()
                        .rounded(px(4.))
                        .px_2()
                        .py(px(2.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(|this, _, _, cx| this.retry_bootstrap(cx)))
                        .child(Icon::new(KairoIcon::RefreshCw).size(px(11.)))
                        .child("Retry"),
                )
                .child(
                    div()
                        .id(ids::DISMISS)
                        .test_support()
                        .role(Role::Button)
                        .aria_label(SharedString::new_static("Dismiss startup warning"))
                        .flex_shrink_0()
                        .rounded(px(4.))
                        .p(px(2.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(|this, _, _, cx| this.dismiss_startup_warning(cx)))
                        .child(Icon::new(KairoIcon::X).size(px(13.))),
                )
                .into_any_element(),
        )
    }

    /// The app's opacity as it fades in after the splash
    /// (\`animate-fade-in\` on the app frame), and whether it is still fading.
    pub(crate) fn opening_opacity(&mut self, cx: &mut Context<Self>) -> (f32, bool) {
        let now = cx.background_executor().now();
        if self.seen_openings != self.launch.openings() {
            self.seen_openings = self.launch.openings();
            // The first opening of a window without a splash does not fade.
            self.opened_at = self.launch_epoch.map(|_| now);
        }
        let Some(opened) = self.opened_at else {
            return (1.0, false);
        };
        let elapsed = now.saturating_duration_since(opened).as_secs_f32() * 1000.0;
        let frame = once(&motion::FADE_IN, elapsed, 0.0, false);
        let fading = elapsed < motion::FADE_IN.duration_ms as f32;
        if !fading {
            self.opened_at = None;
        }
        (frame.opacity, fading)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_ui_kit::motion::REST;

    #[test]
    fn boot_band_runs_left_to_right() {
        // At the start the band sits left of the text.
        assert_eq!(boot_band(1.0, 0.5), 0.0);
        assert!((boot_band(1.0, 0.0) - 0.0).abs() < 1e-6);
        // Halfway it lights the middle.
        assert!((boot_band(0.5, 0.5) - 1.0).abs() < 1e-6);
        assert!((boot_band(0.5, 0.625) - 0.5).abs() < 1e-6);
        // At the end it has passed the right edge.
        assert_eq!(boot_band(0.0, 1.0), 0.0);
    }

    #[test]
    fn copy_waits_for_its_delay_and_still_is_at_rest() {
        let start = once(&motion::SPLASH_COPY_IN, 0.0, COPY_DELAY_MS, false);
        assert_eq!(start.opacity, 0.0);
        assert_eq!(start.y, 6.0);
        assert_eq!(
            once(&motion::SPLASH_COPY_IN, 0.0, COPY_DELAY_MS, true),
            REST
        );
        assert_eq!(
            once(&motion::SPLASH_COPY_IN, 700.0, COPY_DELAY_MS, false),
            REST
        );
    }
}
