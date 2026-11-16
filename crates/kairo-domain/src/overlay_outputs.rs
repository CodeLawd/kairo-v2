//! Output routing for Kairo screens, video cards, NDI and Syphon, from
//! `src/lib/overlay-outputs.ts`.

use crate::overlay::{
    OutputSource, OverlayBackgroundType, OverlayContentKind, OverlayOutput, OverlayOutputKind,
    OverlayOutputVariant, OverlayTheme,
};
use crate::serde_util::Extra;
use crate::settings::OverlaySettings;
use serde_json::Value;

/// `OVERLAY_OUTPUT_KINDS`.
pub const OVERLAY_OUTPUT_KINDS: &[OverlayOutputKind] = OverlayOutputKind::ALL;
/// `MAX_NDI_OUTPUTS`.
pub const MAX_NDI_OUTPUTS: usize = 4;
/// `MAX_SYPHON_OUTPUTS`.
pub const MAX_SYPHON_OUTPUTS: usize = 4;

/// `outputDestinationLabel`.
pub fn output_destination_label(kind: OverlayOutputKind) -> &'static str {
    match kind {
        OverlayOutputKind::Screen => "External display",
        OverlayOutputKind::Device => "Video card",
        OverlayOutputKind::Ndi => "NDI feed",
        OverlayOutputKind::Syphon => "Syphon",
    }
}

/// `isRenderedKind`: every current kind is rendered by Kairo.
pub fn is_rendered_kind(kind: OverlayOutputKind) -> bool {
    matches!(
        kind,
        OverlayOutputKind::Ndi
            | OverlayOutputKind::Screen
            | OverlayOutputKind::Device
            | OverlayOutputKind::Syphon
    )
}

/// `isLocalKind`: a display or a video card, which can run its own playlist.
pub fn is_local_kind(kind: OverlayOutputKind) -> bool {
    matches!(kind, OverlayOutputKind::Screen | OverlayOutputKind::Device)
}

/// `followsProgram`: false only for a local output running its own playlist.
pub fn follows_program(kind: OverlayOutputKind, source: OutputSource) -> bool {
    !(is_local_kind(kind) && source == OutputSource::Playlist)
}

fn follows(output: &OverlayOutput) -> bool {
    follows_program(output.kind, output.source)
}

/// `surfaceOutputs`: enabled outputs Kairo renders that follow the program.
pub fn surface_outputs(outputs: &[OverlayOutput]) -> Vec<&OverlayOutput> {
    outputs
        .iter()
        .filter(|o| o.enabled && is_rendered_kind(o.kind) && follows(o))
        .collect()
}

/// `previewOutputs`: every configured output, by `order` (stable).
pub fn preview_outputs(outputs: &[OverlayOutput]) -> Vec<&OverlayOutput> {
    let mut sorted: Vec<&OverlayOutput> = outputs.iter().collect();
    // `a.order - b.order`; a NaN difference compares as equal, as in JavaScript.
    sorted.sort_by(|a, b| {
        a.order
            .partial_cmp(&b.order)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    sorted
}

/// `primaryRenderedOutput`: an enabled screen, else an enabled video card,
/// else an enabled NDI output, else any NDI output.
pub fn primary_rendered_output(outputs: &[OverlayOutput]) -> Option<&OverlayOutput> {
    let enabled = |kind| outputs.iter().find(|o| o.kind == kind && o.enabled);
    enabled(OverlayOutputKind::Screen)
        .or_else(|| enabled(OverlayOutputKind::Device))
        .or_else(|| enabled(OverlayOutputKind::Ndi))
        .or_else(|| find_ndi_output(outputs))
}

/// `renderedOutputs`: enabled, not fallback-only, following the program, and
/// accepted by `include` (TypeScript's default accepts every output).
pub fn rendered_outputs(
    outputs: &[OverlayOutput],
    include: impl Fn(&OverlayOutput) -> bool,
) -> Vec<&OverlayOutput> {
    outputs
        .iter()
        .filter(|o| o.enabled && !o.fallback_only && follows(o) && include(o))
        .collect()
}

/// `fallbackOutputs`: enabled fallback-only outputs, by `order`.
pub fn fallback_outputs(outputs: &[OverlayOutput]) -> Vec<&OverlayOutput> {
    let mut list: Vec<&OverlayOutput> = outputs
        .iter()
        .filter(|o| o.enabled && o.fallback_only && follows(o))
        .collect();
    list.sort_by(|a, b| {
        a.order
            .partial_cmp(&b.order)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    list
}

/// `primaryNdiOutputId`: the first enabled NDI output keeps the sender identity.
pub fn primary_ndi_output_id(outputs: &[OverlayOutput]) -> Option<&str> {
    outputs
        .iter()
        .find(|o| o.kind == OverlayOutputKind::Ndi && o.enabled)
        .map(|o| o.id.as_str())
}

/// `findNdiOutput`: the first enabled NDI output, else the first configured one.
pub fn find_ndi_output(outputs: &[OverlayOutput]) -> Option<&OverlayOutput> {
    outputs
        .iter()
        .find(|o| o.kind == OverlayOutputKind::Ndi && o.enabled)
        .or_else(|| outputs.iter().find(|o| o.kind == OverlayOutputKind::Ndi))
}

/// `OverlayDispatchPlan`.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayDispatchPlan<'a> {
    pub rendered: Vec<&'a OverlayOutput>,
    pub fallbacks: Vec<&'a OverlayOutput>,
}

/// `getDispatchPlan`.
pub fn get_dispatch_plan<'a>(
    outputs: &'a [OverlayOutput],
    include: impl Fn(&OverlayOutput) -> bool,
) -> OverlayDispatchPlan<'a> {
    OverlayDispatchPlan {
        rendered: rendered_outputs(outputs, &include),
        fallbacks: fallback_outputs(outputs)
            .into_iter()
            .filter(|o| include(o))
            .collect(),
    }
}

// ─── Content kinds ─────────────────────────────────────────────────────────────

/// `OVERLAY_CONTENT_KINDS`.
pub const OVERLAY_CONTENT_KINDS: &[OverlayContentKind] = OverlayContentKind::ALL;

/// `contentKindLabel`.
pub fn content_kind_label(kind: OverlayContentKind) -> &'static str {
    match kind {
        OverlayContentKind::Lyrics => "Lyrics",
        OverlayContentKind::Scripture => "Scripture",
    }
}

/// The two text layers of a theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverlayLayer {
    Verse,
    Reference,
}

/// `overlayLayerLabel`: what each text layer is called for a content kind.
pub fn overlay_layer_label(kind: OverlayContentKind, layer: OverlayLayer) -> &'static str {
    match (kind, layer) {
        (OverlayContentKind::Lyrics, OverlayLayer::Verse) => "Lyrics",
        (OverlayContentKind::Lyrics, OverlayLayer::Reference) => "Song title",
        (OverlayContentKind::Scripture, OverlayLayer::Verse) => "Scripture",
        (OverlayContentKind::Scripture, OverlayLayer::Reference) => "Reference",
    }
}

/// `inheritLyricsTheme`: type and boxes only; no background, backdrop,
/// elements or fit-to-box.
pub fn inherit_lyrics_theme(scripture_theme: &OverlayTheme) -> OverlayTheme {
    let mut theme = scripture_theme.clone();
    theme.elements = Vec::new();
    theme.background.kind = OverlayBackgroundType::Transparent;
    theme.layout.backdrop_box = false;
    theme.layout.auto_fit_text = false;
    theme
}

/// `outputVariant`: the lyrics override, if any. Scripture never has one.
pub fn output_variant(
    output: &OverlayOutput,
    kind: OverlayContentKind,
) -> Option<&OverlayOutputVariant> {
    match kind {
        OverlayContentKind::Lyrics => output.lyrics.as_ref(),
        OverlayContentKind::Scripture => None,
    }
}

/// `hasContentOverride`.
pub fn has_content_override(output: &OverlayOutput, kind: OverlayContentKind) -> bool {
    output_variant(output, kind).is_some()
}

/// `outputThemeFor`: the theme an output renders `kind` with.
pub fn output_theme_for(output: &OverlayOutput, kind: OverlayContentKind) -> OverlayTheme {
    match output_variant(output, kind) {
        Some(variant) => variant.theme.clone(),
        None if kind == OverlayContentKind::Lyrics => inherit_lyrics_theme(&output.theme),
        None => output.theme.clone(),
    }
}

/// `outputThemeIdFor`. Display only.
pub fn output_theme_id_for(output: &OverlayOutput, kind: OverlayContentKind) -> Option<&str> {
    match output_variant(output, kind) {
        Some(variant) => variant.theme_id.as_deref(),
        None => output.theme_id.as_deref(),
    }
}

/// `outputTemplateFor`.
pub fn output_template_for(output: &OverlayOutput, kind: OverlayContentKind) -> &str {
    output_variant(output, kind).map_or(output.template.as_str(), |v| v.template.as_str())
}

/// `Partial<OverlayOutputVariant>`: each field set replaces the stored one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VariantPatch {
    pub theme_id: Option<Option<String>>,
    pub theme: Option<OverlayTheme>,
    pub template: Option<String>,
}

/// `withContentPatch`: writes the patch to the override when one exists,
/// otherwise to the output's own fields.
pub fn with_content_patch(
    output: &OverlayOutput,
    kind: OverlayContentKind,
    patch: VariantPatch,
) -> OverlayOutput {
    let mut next = output.clone();
    let has_variant = output_variant(output, kind).is_some();
    if has_variant {
        let variant = next.lyrics.as_mut().expect("checked above");
        if let Some(theme_id) = patch.theme_id {
            variant.theme_id = theme_id;
        }
        if let Some(theme) = patch.theme {
            variant.theme = theme;
        }
        if let Some(template) = patch.template {
            variant.template = template;
        }
    } else {
        if let Some(theme_id) = patch.theme_id {
            next.theme_id = theme_id;
        }
        if let Some(theme) = patch.theme {
            next.theme = theme;
        }
        if let Some(template) = patch.template {
            next.template = template;
        }
    }
    next
}

/// `setContentOverride`: turns the lyrics override on (seeded from the
/// scripture look, without the citation) or off. Scripture is left alone.
pub fn set_content_override(
    output: &OverlayOutput,
    kind: OverlayContentKind,
    on: bool,
) -> OverlayOutput {
    if kind != OverlayContentKind::Lyrics {
        return output.clone();
    }
    if !on {
        return OverlayOutput {
            lyrics: None,
            ..output.clone()
        };
    }
    if output.lyrics.is_some() {
        return output.clone();
    }
    let mut theme = output.theme.clone();
    // A lyric slide has no citation to print.
    theme.reference.show = false;
    OverlayOutput {
        lyrics: Some(OverlayOutputVariant {
            theme_id: output.theme_id.clone(),
            theme: inherit_lyrics_theme(&theme),
            template: "{Text}".into(),
            extra: Extra::new(),
        }),
        ..output.clone()
    }
}

// ─── Theme selection ───────────────────────────────────────────────────────────

/// `liveOverlayTheme`: the theme a push of `kind` looks like right now.
pub fn live_overlay_theme(overlay: &OverlaySettings, kind: OverlayContentKind) -> OverlayTheme {
    match primary_rendered_output(&overlay.outputs) {
        Some(rendered) => output_theme_for(rendered, kind),
        None => overlay.theme.clone(),
    }
}

// ─── Media allowlist ───────────────────────────────────────────────────────────

/// `configuredMediaPaths`: every background and element media path in the
/// raw stored overlay and theme library, untrimmed, in TypeScript's order.
/// Reads raw values on purpose: this runs per media request.
pub fn configured_media_paths(
    stored_overlay: Option<&Value>,
    stored_theme_library: Option<&Value>,
) -> Vec<String> {
    let mut paths = Vec::new();

    fn add(paths: &mut Vec<String>, media_path: Option<&Value>) {
        if let Some(Value::String(path)) = media_path
            && !crate::js::trim(path).is_empty()
        {
            paths.push(path.clone());
        }
    }
    fn get<'a>(v: Option<&'a Value>, key: &str) -> Option<&'a Value> {
        v.and_then(|v| v.as_object()).and_then(|m| m.get(key))
    }
    fn collect(paths: &mut Vec<String>, theme: Option<&Value>) {
        add(paths, get(get(theme, "background"), "mediaPath"));
        if let Some(Value::Array(elements)) = get(theme, "elements") {
            for element in elements {
                add(paths, get(Some(element), "mediaPath"));
            }
        }
    }

    collect(&mut paths, get(stored_overlay, "theme"));
    if let Some(Value::Array(outputs)) = get(stored_overlay, "outputs") {
        for output in outputs {
            collect(&mut paths, get(Some(output), "theme"));
            collect(&mut paths, get(get(Some(output), "lyrics"), "theme"));
        }
    }
    if let Some(Value::Array(items)) = stored_theme_library {
        for item in items {
            collect(&mut paths, get(Some(item), "theme"));
            collect(&mut paths, get(Some(item), "baseline"));
        }
    }
    paths
}
