//! `tests/overlay-dispatch.test.ts`: which outputs take a program push, which
//! are held back as fallbacks, and what each destination is called.

use kairo_domain::overlay::{OutputSource, OverlayOutput, OverlayOutputKind};
use kairo_domain::overlay_defaults::make_overlay_output;
use kairo_domain::overlay_outputs::{
    follows_program, get_dispatch_plan, output_destination_label, surface_outputs,
};

fn out(id: &str, kind: OverlayOutputKind, edit: impl FnOnce(&mut OverlayOutput)) -> OverlayOutput {
    let mut o = make_overlay_output(id, kind);
    o.enabled = true;
    edit(&mut o);
    o
}

fn ids(list: &[&OverlayOutput]) -> Vec<String> {
    list.iter().map(|o| o.id.clone()).collect()
}

#[test]
fn screens_devices_and_every_ndi_feed_receive_independent_program_pushes() {
    let outputs = vec![
        out("projector", OverlayOutputKind::Screen, |_| {}),
        out("card", OverlayOutputKind::Device, |_| {}),
        out("main", OverlayOutputKind::Ndi, |_| {}),
        out("stream", OverlayOutputKind::Ndi, |_| {}),
        out("off", OverlayOutputKind::Screen, |o| o.enabled = false),
    ];
    let all = ["projector", "card", "main", "stream"];
    assert_eq!(ids(&get_dispatch_plan(&outputs, |_| true).rendered), all);
    assert_eq!(ids(&surface_outputs(&outputs)), all);
    let empty = get_dispatch_plan(&[], |_| true);
    assert!(empty.rendered.is_empty() && empty.fallbacks.is_empty());
}

#[test]
fn content_filters_apply_independently_to_normal_and_fallback_outputs() {
    let outputs = vec![
        out("screen", OverlayOutputKind::Screen, |o| {
            o.show.lyrics = false
        }),
        out("feed", OverlayOutputKind::Ndi, |_| {}),
        out("fallback-off", OverlayOutputKind::Device, |o| {
            o.fallback_only = true;
            o.show.lyrics = false;
        }),
        out("fallback", OverlayOutputKind::Ndi, |o| {
            o.fallback_only = true
        }),
    ];
    let plan = get_dispatch_plan(&outputs, |o| o.show.lyrics);
    assert_eq!(ids(&plan.rendered), ["feed"]);
    assert_eq!(ids(&plan.fallbacks), ["fallback"]);
}

#[test]
fn fallbacks_are_held_back_and_ordered_disabled_and_playlist_destinations_are_excluded() {
    let outputs = vec![
        out("late", OverlayOutputKind::Ndi, |o| {
            o.fallback_only = true;
            o.order = 9.;
        }),
        out("early", OverlayOutputKind::Screen, |o| {
            o.fallback_only = true;
            o.order = 1.;
        }),
        out("off", OverlayOutputKind::Ndi, |o| {
            o.enabled = false;
            o.fallback_only = true;
        }),
        out("lobby", OverlayOutputKind::Screen, |o| {
            o.source = OutputSource::Playlist;
            o.fallback_only = true;
        }),
        out("card", OverlayOutputKind::Device, |o| {
            o.source = OutputSource::Playlist
        }),
    ];
    let plan = get_dispatch_plan(&outputs, |_| true);
    assert!(plan.rendered.is_empty());
    assert_eq!(ids(&plan.fallbacks), ["early", "late"]);
    assert_eq!(ids(&surface_outputs(&outputs)), ["late", "early"]);
    assert!(!follows_program(outputs[3].kind, outputs[3].source));
    assert!(!follows_program(outputs[4].kind, outputs[4].source));
}

#[test]
fn destination_labels_describe_standalone_destinations() {
    assert_eq!(
        output_destination_label(OverlayOutputKind::Screen),
        "External display"
    );
    assert_eq!(
        output_destination_label(OverlayOutputKind::Device),
        "Video card"
    );
    assert_eq!(output_destination_label(OverlayOutputKind::Ndi), "NDI feed");
}
