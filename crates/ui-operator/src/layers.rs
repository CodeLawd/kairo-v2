//! The clear-layer strip under the monitor (`LiveLayerStrip.tsx`).

use kairo_ui_kit::KairoIcon;

use crate::backend::LiveSnapshot;

/// One thing that can be on screen, in the strip's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Layer {
    Audio,
    Messages,
    Props,
    Slide,
    Background,
    Camera,
}

impl Layer {
    pub const ALL: [Layer; 6] = [
        Layer::Audio,
        Layer::Messages,
        Layer::Props,
        Layer::Slide,
        Layer::Background,
        Layer::Camera,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Layer::Audio => "audio",
            Layer::Messages => "messages",
            Layer::Props => "props",
            Layer::Slide => "slide",
            Layer::Background => "background",
            Layer::Camera => "camera",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Layer::Audio => "Audio",
            Layer::Messages => "Messages",
            Layer::Props => "Props",
            Layer::Slide => "Slide text",
            Layer::Background => "Background",
            Layer::Camera => "Video",
        }
    }

    pub fn icon(self) -> KairoIcon {
        match self {
            Layer::Audio => KairoIcon::Music2,
            Layer::Messages => KairoIcon::MessageSquare,
            Layer::Props => KairoIcon::Layers,
            Layer::Slide => KairoIcon::AlignCenter,
            Layer::Background => KairoIcon::Image,
            Layer::Camera => KairoIcon::Video,
        }
    }

    /// Something is on this layer now.
    pub fn is_on(self, live: &LiveSnapshot) -> bool {
        match self {
            Layer::Audio => live.track_live,
            Layer::Messages => live.program.message.is_some(),
            Layer::Props => !live.program.active_prop_ids.is_empty() || live.program.logo,
            Layer::Slide => live.has_text(),
            Layer::Background => live.has_background(),
            Layer::Camera => live.program.camera.is_some(),
        }
    }

    /// The button's name: what clicking it does when lit.
    pub fn button_label(self, on: bool) -> String {
        if on {
            format!("Clear {}", self.label().to_lowercase())
        } else {
            self.label().to_string()
        }
    }
}

/// The lit layers, in strip order.
pub fn lit(live: &LiveSnapshot) -> Vec<Layer> {
    Layer::ALL.into_iter().filter(|l| l.is_on(live)).collect()
}

/// `clearAll`: text and background go together through `clearLiveAll`; then
/// every other lit layer. `None` when nothing is on (the button is disabled).
pub fn clear_all_others(live: &LiveSnapshot) -> Option<Vec<Layer>> {
    let lit = lit(live);
    if lit.is_empty() {
        return None;
    }
    Some(
        lit.into_iter()
            .filter(|l| !matches!(l, Layer::Slide | Layer::Background))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_light_from_the_snapshot() {
        let mut live = LiveSnapshot::default();
        assert!(lit(&live).is_empty());
        assert_eq!(clear_all_others(&live), None);
        live.track_live = true;
        live.program.message = Some("Welcome".into());
        live.program.logo = true;
        live.program.camera = Some("FaceTime HD".into());
        live.document_live = true;
        assert_eq!(
            lit(&live),
            vec![
                Layer::Audio,
                Layer::Messages,
                Layer::Props,
                Layer::Background,
                Layer::Camera
            ]
        );
        assert_eq!(
            clear_all_others(&live),
            Some(vec![
                Layer::Audio,
                Layer::Messages,
                Layer::Props,
                Layer::Camera
            ])
        );
    }

    #[test]
    fn a_live_id_without_its_item_is_no_background() {
        let mut live = LiveSnapshot::default();
        live.media.live_item_id = Some("gone".into());
        assert!(!Layer::Background.is_on(&live));
        assert_eq!(live.live_media(), None);
    }

    #[test]
    fn labels_name_the_action() {
        assert_eq!(Layer::Slide.button_label(true), "Clear slide text");
        assert_eq!(Layer::Camera.button_label(false), "Video");
    }
}
