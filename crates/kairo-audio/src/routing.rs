//! Which surface makes sound (`surface-manager.ts`), and when the operator
//! window plays it instead (`ProgramAudioFallback.tsx`).

use kairo_domain::overlay::{OutputSource, OverlayOutput, OverlayOutputKind};
use kairo_domain::overlay_outputs::{follows_program, is_rendered_kind, primary_ndi_output_id};
use kairo_domain::program::ProgramAudioSettings;

/// `liveTargets('backgrounds')` filtered as `audioSurface` reads it: an
/// output Kairo renders, enabled, showing backgrounds, following the
/// service (never a lobby), with a surface that can take a push
/// (`has_surface`: a ready screen window, an NDI or Syphon sender, a ready
/// video card).
fn live_targets<'a>(
    outputs: &'a [OverlayOutput],
    has_surface: &impl Fn(&OverlayOutput) -> bool,
) -> impl Iterator<Item = &'a OverlayOutput> {
    outputs.iter().filter(move |o| {
        is_rendered_kind(o.kind)
            && o.enabled
            && o.show.backgrounds
            && follows_program(o.kind, o.source)
            && has_surface(o)
    })
}

/// `audioSurface`: the first live screen, else the primary NDI output.
/// Exactly one, so two outputs never echo each other; `None` is silence.
pub fn audio_surface(
    outputs: &[OverlayOutput],
    has_surface: impl Fn(&OverlayOutput) -> bool,
) -> Option<&str> {
    let primary = primary_ndi_output_id(outputs);
    live_targets(outputs, &has_surface)
        .find(|o| o.kind == OverlayOutputKind::Screen)
        .or_else(|| live_targets(outputs, &has_surface).find(|o| Some(o.id.as_str()) == primary))
        // Else the first live video card (DECISIONS 113), so a video is never
        // silent while it shows. A second NDI sender still never sounds.
        .or_else(|| {
            live_targets(outputs, &has_surface).find(|o| o.kind == OverlayOutputKind::Device)
        })
        .map(|o| o.id.as_str())
}

/// What `applyConfig` gives one surface for sound.
#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceAudio {
    /// `enabled && surface === audioSurface`.
    pub enabled: bool,
    pub volume: f64,
    pub output_label: String,
    /// Camera sound plays here: the audio surface, a camera up, and an
    /// audio input chosen for it.
    pub camera_audible: bool,
    /// This surface feeds its NDI sender sound: NDI audio on, and the
    /// sender can carry it.
    pub ndi_audio: bool,
}

/// `applyConfig` for one surface. `ndi_sender` is whether the surface is an
/// NDI sender whose binding can send audio.
pub fn surface_audio(
    surface: &str,
    audio_surface: Option<&str>,
    audio: &ProgramAudioSettings,
    ndi_sender: bool,
    camera: Option<&str>,
    camera_audio: Option<&str>,
) -> SurfaceAudio {
    let here = audio_surface == Some(surface);
    SurfaceAudio {
        enabled: audio.enabled && here,
        volume: audio.volume,
        output_label: audio.output_label.clone(),
        camera_audible: here && camera.is_some() && camera_audio.is_some(),
        ndi_audio: audio.ndi && ndi_sender,
    }
}

/// `ProgramAudioFallback`: the operator window plays the live background
/// video's sound when sound is on and no screen or NDI output that follows
/// the service and shows backgrounds is ready to play it. Video cards and
/// Syphon do not count, so a rig of only those hears the fallback.
pub fn fallback_active(
    outputs: &[OverlayOutput],
    ready: impl Fn(&str) -> bool,
    audio_enabled: bool,
    live_is_video: bool,
) -> bool {
    let surface_live = outputs.iter().any(|o| {
        if !o.enabled || !o.show.backgrounds {
            return false;
        }
        if o.kind == OverlayOutputKind::Screen && o.source == OutputSource::Playlist {
            return false;
        }
        if o.kind != OverlayOutputKind::Screen && o.kind != OverlayOutputKind::Ndi {
            return false;
        }
        ready(&o.id)
    });
    audio_enabled && live_is_video && !surface_live
}

#[cfg(test)]
mod tests {
    use super::*;
    use kairo_domain::overlay_defaults::make_overlay_output;

    fn output(id: &str, kind: OverlayOutputKind) -> OverlayOutput {
        let mut o = make_overlay_output(id, kind);
        o.enabled = true;
        o.show.backgrounds = true;
        o
    }

    #[test]
    fn the_first_live_screen_wins_else_the_primary_ndi() {
        let ndi = output("ndi", OverlayOutputKind::Ndi);
        let ndi2 = output("ndi2", OverlayOutputKind::Ndi);
        let a = output("a", OverlayOutputKind::Screen);
        let b = output("b", OverlayOutputKind::Screen);
        let all = vec![ndi.clone(), ndi2.clone(), a.clone(), b.clone()];
        assert_eq!(audio_surface(&all, |_| true), Some("a"));
        // A screen that is not ready has no surface.
        assert_eq!(audio_surface(&all, |o| o.id != "a"), Some("b"));
        assert_eq!(
            audio_surface(&all, |o| o.kind == OverlayOutputKind::Ndi),
            Some("ndi")
        );
        // Only the primary NDI output: a second sender never makes sound.
        assert_eq!(audio_surface(&all, |o| o.id == "ndi2"), None);
        // A screen that hides backgrounds, or is off, does not count.
        let mut hidden = a.clone();
        hidden.show.backgrounds = false;
        let mut off = b.clone();
        off.enabled = false;
        assert_eq!(
            audio_surface(&[hidden, off, ndi.clone()], |_| true),
            Some("ndi")
        );
    }

    #[test]
    fn a_lobby_screen_is_never_the_audio_surface() {
        let mut lobby = output("lobby", OverlayOutputKind::Screen);
        lobby.source = OutputSource::Playlist;
        let card = output("card", OverlayOutputKind::Device);
        // With no screen or NDI feed, the program's video card sounds
        // (DECISIONS 113); the lobby screen still never does.
        assert_eq!(
            audio_surface(&[lobby.clone(), card.clone()], |_| true),
            Some("card")
        );
        let ndi = output("ndi", OverlayOutputKind::Ndi);
        assert_eq!(audio_surface(&[lobby, card, ndi], |_| true), Some("ndi"));
    }

    #[test]
    fn only_the_audio_surface_is_audible_and_camera_sound_needs_an_input() {
        let audio = ProgramAudioSettings {
            enabled: true,
            volume: 0.7,
            output_label: "USB".into(),
            ndi: true,
            extra: Default::default(),
        };
        let here = surface_audio("a", Some("a"), &audio, false, Some("Cam"), Some("Mic"));
        assert!(here.enabled && here.camera_audible && !here.ndi_audio);
        assert_eq!(here.volume, 0.7);
        let other = surface_audio("b", Some("a"), &audio, true, Some("Cam"), Some("Mic"));
        assert!(!other.enabled && !other.camera_audible && other.ndi_audio);
        let silent_cam = surface_audio("a", Some("a"), &audio, false, Some("Cam"), None);
        assert!(!silent_cam.camera_audible);
        let off = ProgramAudioSettings {
            enabled: false,
            ndi: false,
            ..audio.clone()
        };
        let none = surface_audio("a", Some("a"), &off, true, None, None);
        assert!(!none.enabled && !none.ndi_audio);
    }

    #[test]
    fn the_fallback_plays_only_when_no_screen_or_ndi_is_ready() {
        let screen = output("s", OverlayOutputKind::Screen);
        let ndi = output("n", OverlayOutputKind::Ndi);
        let card = output("c", OverlayOutputKind::Device);
        let syphon = output("y", OverlayOutputKind::Syphon);
        let mut lobby = output("l", OverlayOutputKind::Screen);
        lobby.source = OutputSource::Playlist;
        let all = vec![screen, ndi, card, syphon, lobby];
        assert!(!fallback_active(&all, |_| true, true, true));
        // Hardware outputs only (and a lobby): the operator window plays.
        assert!(fallback_active(
            &all,
            |id| id == "c" || id == "y" || id == "l",
            true,
            true
        ));
        assert!(!fallback_active(&all, |_| false, false, true), "sound off");
        assert!(
            !fallback_active(&all, |_| false, true, false),
            "not a video"
        );
        assert!(fallback_active(&[], |_| true, true, true));
    }
}
