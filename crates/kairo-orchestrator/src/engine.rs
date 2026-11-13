//! [`EngineClient`] over the real output engine.
//!
//! `kairo-core` owns the engine client and supervisor ([`EngineHandle`]);
//! the trait lives here because `kairo-core` cannot depend on this crate
//! (DECISIONS 039 and 050). Surface ids are output ids. Which outputs exist
//! and what each may show come from settings, which the surface manager
//! (`kairo-output`, prompt 065) passes in with [`EngineSurfaces::set_routing`]:
//! the outputs, and which surfaces follow the service. Lobby screens and
//! stage displays are engine surfaces too, but never take a push or a Clear.

use crate::clients::{ColoredLine, EngineClient, ShowKind};
use crate::voice::BoxFuture;
use kairo_core::engine::{EngineHandle, OutputEngine, PAINT_WAIT};
use kairo_domain::documents::DocumentSlide;
use kairo_domain::overlay::{OverlayBackground, OverlayOutput, OverlayTheme};
use kairo_domain::overlay_outputs::{follows_program, is_rendered_kind};
use kairo_engine_proto::{Command, Json, SlideContent};
use std::sync::RwLock;

pub struct EngineSurfaces<E: OutputEngine = EngineHandle> {
    engine: E,
    routing: RwLock<Routing>,
}

#[derive(Default)]
struct Routing {
    outputs: Vec<OverlayOutput>,
    /// Surfaces that follow the service, in Clear order (`allSurfaces`).
    /// `None` until the surface manager has said: every surface then counts.
    followers: Option<Vec<String>>,
}

impl<E: OutputEngine> EngineSurfaces<E> {
    pub fn new(engine: E) -> Self {
        Self {
            engine,
            routing: RwLock::new(Routing::default()),
        }
    }

    pub fn engine(&self) -> &E {
        &self.engine
    }

    /// The configured outputs, in settings order.
    pub fn set_outputs(&self, outputs: Vec<OverlayOutput>) {
        self.routing
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .outputs = outputs;
    }

    /// The outputs, and the surfaces a push or a Clear may reach.
    pub fn set_routing(&self, outputs: Vec<OverlayOutput>, followers: Vec<String>) {
        let mut routing = self.routing.write().unwrap_or_else(|e| e.into_inner());
        routing.outputs = outputs;
        routing.followers = Some(followers);
    }

    fn slide(
        reference: String,
        text: String,
        theme: &OverlayTheme,
        colored: Option<Vec<ColoredLine>>,
    ) -> SlideContent {
        SlideContent {
            reference,
            text,
            theme: Json::encode(theme),
            colored_lines: colored.as_ref().map(Json::encode),
        }
    }
}

impl<E: OutputEngine> EngineClient for EngineSurfaces<E> {
    fn unavailable_reason(&self, output: &OverlayOutput) -> Option<String> {
        self.engine.unavailable_reason(&output.id)
    }

    fn show_slide(
        &self,
        output_id: String,
        reference: String,
        text: String,
        theme: OverlayTheme,
        colored: Option<Vec<ColoredLine>>,
    ) -> BoxFuture<'_, bool> {
        let slide = Self::slide(reference, text, &theme, colored);
        let sent = self.engine.command(Command::ShowSlide {
            output: output_id.clone(),
            source: output_id,
            slide,
            instant: false,
        });
        Box::pin(async move { sent.accepted })
    }

    fn set_background(
        &self,
        output_id: String,
        background: OverlayBackground,
    ) -> BoxFuture<'_, bool> {
        let sent = self.engine.command(Command::SetBackground {
            output: output_id,
            background: Json::encode(&background),
        });
        Box::pin(async move { sent.accepted })
    }

    fn swap_document_page(
        &self,
        output_id: String,
        media_path: String,
        slide: Option<DocumentSlide>,
    ) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            let sent = self.engine.command(Command::SwapPage {
                output: output_id.clone(),
                source: output_id.clone(),
                media_path,
                document: slide.as_ref().map(Json::encode),
            });
            if sent.accepted {
                // `nextPaint`: the turn returns once its frame is out, or
                // after `PAINT_WAIT` if the sink never says so.
                self.engine
                    .wait_frame(&output_id, sent.seq, PAINT_WAIT)
                    .await;
            }
            sent.accepted
        })
    }

    fn show_document(
        &self,
        output_id: String,
        theme: OverlayTheme,
        slide: Option<DocumentSlide>,
    ) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            // A page is the presentation itself, not decorative theme media.
            let mut theme = theme;
            theme.background.blur_px = Some(0.0);
            let sent = self.engine.command(Command::ShowDocument {
                output: output_id.clone(),
                source: output_id.clone(),
                slide: Self::slide(String::new(), String::new(), &theme, None),
                document: slide.as_ref().map(Json::encode),
            });
            if sent.accepted {
                self.engine
                    .wait_frame(&output_id, sent.seq, PAINT_WAIT)
                    .await;
            }
            sent.accepted
        })
    }

    fn clear_text(
        &self,
        surface_id: String,
        background: Option<OverlayBackground>,
    ) -> BoxFuture<'_, bool> {
        let sent = self.engine.command(Command::ClearText {
            output: surface_id,
            background: background.as_ref().map(Json::encode),
        });
        Box::pin(async move { sent.accepted })
    }

    fn clear(&self, surface_id: String) -> BoxFuture<'_, ()> {
        self.engine.command(Command::Clear { output: surface_id });
        Box::pin(async {})
    }

    /// `liveTargets`: rendered, enabled outputs whose filter allows `show`
    /// and whose surface takes pushes now.
    fn live_targets(&self, show: Option<ShowKind>) -> Vec<OverlayOutput> {
        let routing = self.routing.read().unwrap_or_else(|e| e.into_inner());
        routing
            .outputs
            .iter()
            .filter(|output| is_rendered_kind(output.kind) && output.enabled)
            // `surfaceFor`: never a lobby.
            .filter(|output| follows_program(output.kind, output.source))
            .filter(|output| match show {
                Some(ShowKind::Documents) => output.show.documents,
                Some(ShowKind::Backgrounds) => output.show.backgrounds,
                None => true,
            })
            .filter(|output| self.engine.is_available(&output.id))
            .cloned()
            .collect()
    }

    /// `allSurfaces`: every surface that follows the service.
    fn all_surfaces(&self) -> Vec<String> {
        let routing = self.routing.read().unwrap_or_else(|e| e.into_inner());
        let surfaces = self.engine.surfaces();
        match &routing.followers {
            Some(followers) => followers
                .iter()
                .filter(|id| surfaces.contains(id))
                .cloned()
                .collect(),
            None => surfaces,
        }
    }
}
