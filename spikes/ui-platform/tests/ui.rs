use gpui_kit::component::input::{Input, InputState};
use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext, Bounds, Context, Entity, EntityInputHandler, IntoElement, Render,
    TestAppContext, Window, WindowBounds, WindowOptions, div, point, prelude::*, px, size,
};
use std::collections::HashSet;
use ui_platform_spike::view::{SlideGrid, bind_keys};

fn options() -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            Default::default(),
            size(px(1000.), px(700.)),
        ))),
        ..Default::default()
    }
}

fn open(cx: &mut TestAppContext, cards: u32) -> (AnyWindowHandle, Entity<SlideGrid>) {
    cx.update(gpui_kit::init);
    cx.update(bind_keys);
    let (handle, grid) = cx.update(|cx| {
        gpui_kit::open_window(options(), cx, |window, cx| {
            let view = cx.new(|cx| SlideGrid::new(cards, true, cx));
            view.read(cx).focus_handle().clone().focus(window, cx);
            view
        })
        .expect("window")
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    (handle, grid)
}

#[gpui_kit::test]
fn renders_only_the_visible_cards(cx: &mut TestAppContext) {
    let (_, grid) = open(cx, 5000);
    cx.update(|cx| {
        let grid = grid.read(cx);
        assert_eq!(grid.layout().columns, 4);
        assert!(
            grid.thumbnail_count() > 0 && grid.thumbnail_count() < 60,
            "{}",
            grid.thumbnail_count()
        );
    });
}

#[gpui_kit::test]
fn keyboard_moves_zooms_and_sends_live(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx, 5000);
    cx.update_window(handle, |_, window, cx| {
        window.press("right", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(grid.read(cx).cursor(), 5);
        assert_eq!(grid.read(cx).live(), Some(5));
    });
    cx.update_window(handle, |_, window, cx| {
        window.press("cmd-=", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(grid.read(cx).min_width(), 280.0);
        assert_eq!(grid.read(cx).layout().columns, 3);
    });
}

#[gpui_kit::test]
fn marquee_from_a_gap_selects_cards(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx, 5000);
    cx.update_window(handle, |_, window, cx| {
        window.drag(point(px(247.), px(10.)), point(px(600.), px(400.)), cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            grid.read(cx).selected(),
            &HashSet::from([1, 2, 5, 6, 9, 10])
        )
    });
}

#[gpui_kit::test]
fn dragging_a_card_reorders_it(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx, 20);
    cx.update_window(handle, |_, window, cx| {
        window.drag(point(px(120.), px(60.)), point(px(630.), px(60.)), cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(&grid.read(cx).order()[..4], [1, 0, 2, 3]));
}

#[gpui_kit::test]
fn empty_grid_is_safe(cx: &mut TestAppContext) {
    let (handle, grid) = open(cx, 0);
    cx.update_window(handle, |_, window, cx| {
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(grid.read(cx).live(), None));
}

struct Field(Entity<InputState>);

impl Render for Field {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(Input::new(&self.0).id("note"))
    }
}

/// Drives the same handler macOS calls for marked text. A real Yoruba keyboard
/// layout or IME is a separate, manual check.
#[gpui_kit::test]
fn yoruba_composition_commit_and_undo(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (handle, field) = cx.update(|cx| {
        gpui_kit::open_window(options(), cx, |window, cx| {
            let state = cx.new(|cx| InputState::new(window, cx));
            cx.new(|_| Field(state))
        })
        .expect("window")
    });
    let state = cx.update(|cx| field.read(cx).0.clone());
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("note", cx);
        state.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "e", None, window, cx);
            assert!(input.marked_text_range(window, cx).is_some());
            input.replace_and_mark_text_in_range(None, "ẹ", None, window, cx);
            input.replace_and_mark_text_in_range(None, "ẹ\u{301}", None, window, cx);
            assert_eq!(input.value(), "ẹ\u{301}");
            input.replace_text_in_range(None, "ẹ\u{301}", window, cx);
            assert!(input.marked_text_range(window, cx).is_none());
            input.replace_text_in_range(None, "kọ\u{300}", window, cx);
        });
        assert_eq!(state.read(cx).value(), "ẹ\u{301}kọ\u{300}");
        println!(
            "after commit {:?}",
            state
                .read(cx)
                .value()
                .chars()
                .map(|c| format!("{:04X}", c as u32))
                .collect::<Vec<_>>()
        );
        window.press("backspace", cx);
        println!(
            "after backspace {:?}",
            state
                .read(cx)
                .value()
                .chars()
                .map(|c| format!("{:04X}", c as u32))
                .collect::<Vec<_>>()
        );
        window.press("secondary-z", cx);
        println!(
            "after undo {:?}",
            state
                .read(cx)
                .value()
                .chars()
                .map(|c| format!("{:04X}", c as u32))
                .collect::<Vec<_>>()
        );
    })
    .unwrap();
    cx.update(|cx| assert_eq!(state.read(cx).value(), "ẹ\u{301}kọ\u{300}"));
}
