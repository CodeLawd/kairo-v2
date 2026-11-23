//! Plan, generate and refine: ask the transport, then record the result. An
//! error, or the future being dropped (cancel), leaves the store as it was:
//! nothing is written until every image of a job has arrived.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures_util::{StreamExt, stream};

use crate::agent::{self, DeckPlan, Plan};
use crate::deck::{self, DeckTheme, Layout};
use crate::store::{Kind, NewVersion, Slide};
use crate::{
    ImageModel, ImageRequest, ImageTransport, ImagineError, ImagineStore, Project, Quality,
};

/// Slides drawn at once.
const PARALLEL: usize = 3;

fn locked(store: &Mutex<ImagineStore>) -> std::sync::MutexGuard<'_, ImagineStore> {
    store.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the assistant will do with `message`, given the open creation.
/// `thought` hears its reasoning summary as it streams. If the account
/// can't have summaries, the plan is asked for again without them.
pub async fn plan(
    transport: &dyn ImageTransport,
    project: Option<&Project>,
    message: &str,
    thought: &(dyn Fn(&str) + Send + Sync),
) -> Result<Plan, ImagineError> {
    let streamed = transport
        .respond_streaming(agent::plan_body_with_thoughts(project, message), thought)
        .await;
    let body = match streamed {
        Ok(body) => body,
        Err(ImagineError::Api(_) | ImagineError::NeedsVerification) => {
            transport
                .respond(agent::plan_body(project, message))
                .await?
        }
        Err(e) => return Err(e),
    };
    agent::parse_plan(&body, project)
}

/// The settings every image of a job shares.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub model: ImageModel,
    pub quality: Quality,
}

/// A new image project. `message` is what the operator wrote; `prompt` is
/// what the image model is asked for.
pub async fn generate(
    store: &Mutex<ImagineStore>,
    transport: &dyn ImageTransport,
    message: &str,
    prompt: &str,
    reply: Option<String>,
    look: Look,
) -> Result<Project, ImagineError> {
    let bytes = transport
        .image(ImageRequest {
            model: look.model,
            prompt: prompt.trim().into(),
            quality: look.quality,
            reference: None,
        })
        .await?;
    let store = locked(store);
    let file = store.write_image(&bytes)?;
    store.create(
        Kind::Image,
        NewVersion {
            instruction: message.into(),
            model: look.model,
            quality: look.quality,
            file,
            reply,
            prompt: Some(prompt.trim().into()),
            slides: Vec::new(),
            theme: None,
            background: None,
        },
    )
}

/// A new version of an image project: an edit of version `from`. Earlier
/// versions are never touched.
#[allow(clippy::too_many_arguments)]
pub async fn refine(
    store: &Mutex<ImagineStore>,
    transport: &dyn ImageTransport,
    project: &str,
    from: &str,
    message: &str,
    prompt: &str,
    reply: Option<String>,
    look: Look,
) -> Result<Project, ImagineError> {
    let reference = {
        let store = locked(store);
        let current = store.get(project)?;
        store.read_image(&current, from)?
    };
    let bytes = transport
        .image(ImageRequest {
            model: look.model,
            prompt: prompt.trim().into(),
            quality: look.quality,
            reference: Some(reference),
        })
        .await?;
    let store = locked(store);
    let file = store.write_image(&bytes)?;
    store.append(
        project,
        from,
        NewVersion {
            instruction: message.into(),
            model: look.model,
            quality: look.quality,
            file,
            reply,
            prompt: Some(prompt.trim().into()),
            slides: Vec::new(),
            theme: None,
            background: None,
        },
    )
}

/// A deck the assistant wrote: a new project, or (with `edit` = project
/// and version) a new version of one. The image model paints the shared
/// background (once per deck) and pictures for text-and-picture slides;
/// Kairo lays out and draws every slide. Kept slides keep their words and
/// picture; unchanged pictures are reused. `progress` hears pictures painted
/// and the total.
#[allow(clippy::too_many_arguments)]
pub async fn deck(
    store: &Mutex<ImagineStore>,
    transport: &dyn ImageTransport,
    edit: Option<(&str, &str)>,
    message: &str,
    reply: Option<String>,
    plan: &DeckPlan,
    look: Look,
    progress: &(dyn Fn(Step) + Sync),
) -> Result<Project, ImagineError> {
    if plan.slides.is_empty() {
        return Err(ImagineError::Api("the deck has no slides".into()));
    }
    let previous = match edit {
        Some((project, from)) => {
            let store = locked(store);
            let p = store.get(project)?;
            Some(p.version(from).ok_or(ImagineError::Missing)?.clone())
        }
        None => None,
    };
    let previous_title = match edit {
        Some((project, _)) => Some(locked(store).get(project)?.title),
        None => None,
    };
    let old: &[Slide] = previous.as_ref().map_or(&[], |v| v.slides.as_slice());
    let theme = plan.theme.clone();

    // The words and pictures of every slide, before painting.
    let mut slides: Vec<Slide> = Vec::with_capacity(plan.slides.len());
    let mut paint: Vec<(Option<usize>, String)> = Vec::new(); // (slide, prompt); None = background
    for (i, s) in plan.slides.iter().enumerate() {
        let before = old.get(i).filter(|o| o.editable());
        let slide = match before {
            Some(o) if s.keep => o.clone(),
            _ => {
                // Only layouts that place a picture get one; a third or more
                // of a good deck is pure type.
                let wants_art = s.layout.takes_art()
                    && s.art_prompt
                        .as_deref()
                        .is_some_and(|p| !p.trim().is_empty());
                let prompt = s.art_prompt.clone().unwrap_or_default();
                let art = before
                    .filter(|o| wants_art && o.art.is_some() && o.prompt == prompt)
                    .and_then(|o| o.art.clone());
                if wants_art && art.is_none() && !prompt.trim().is_empty() {
                    paint.push((Some(i), deck::art_prompt(&theme, &prompt, s.layout, s.side)));
                }
                Slide {
                    title: s.title.trim().into(),
                    prompt,
                    file: String::new(),
                    layout: Some(s.layout),
                    body: Vec::new(),
                    eyebrow: s.eyebrow.trim().into(),
                    lead: s.lead.trim().into(),
                    points: s
                        .points
                        .iter()
                        .filter(|p| !p.heading.trim().is_empty() || !p.text.trim().is_empty())
                        .cloned()
                        .collect(),
                    callout: s.callout.trim().into(),
                    shapes: s.shapes.clone(),
                    side: s.side,
                    role: s.role.trim().into(),
                    notes: s.notes.trim().into(),
                    art,
                }
            }
        };
        slides.push(slide);
    }
    let mut background = previous.as_ref().and_then(|v| v.background.clone());
    // A picture only behind title, section and quote slides, and only when asked for.
    let wants_background = !plan.background_prompt.trim().is_empty()
        && slides
            .iter()
            .any(|s| s.layout.is_some_and(Layout::is_dark) && s.art.is_none());
    if background.is_none() && wants_background {
        paint.push((
            None,
            deck::background_prompt(&theme, &plan.background_prompt),
        ));
    }

    let total = paint.len();
    let done = AtomicUsize::new(0);
    progress(Step::Painting { done: 0, total });
    let done = &done;
    let prompts: Vec<String> = paint.iter().map(|(_, p)| p.clone()).collect();
    let painted: Vec<Result<Vec<u8>, ImagineError>> = stream::iter(prompts)
        .map(|prompt| async move {
            let bytes = transport
                .image(ImageRequest {
                    model: look.model,
                    prompt,
                    quality: look.quality,
                    reference: None,
                })
                .await?;
            progress(Step::Painting {
                done: done.fetch_add(1, Ordering::SeqCst) + 1,
                total,
            });
            Ok(bytes)
        })
        .buffered(PARALLEL)
        .collect()
        .await;
    // Every picture arrived before anything is written.
    let painted = painted.into_iter().collect::<Result<Vec<_>, _>>()?;

    let store = locked(store);
    for ((slot, _), bytes) in paint.iter().zip(&painted) {
        let file = store.write_image(bytes)?;
        match slot {
            Some(i) => slides[*i].art = Some(file),
            None => background = Some(file),
        }
    }
    let title = if plan.title.trim().is_empty() {
        previous_title.unwrap_or_default()
    } else {
        plan.title.trim().to_owned()
    };
    draw_all(
        &store,
        &theme,
        &title,
        background.as_deref(),
        &mut slides,
        progress,
    )?;
    let new = NewVersion {
        instruction: message.into(),
        model: look.model,
        quality: look.quality,
        file: slides[0].file.clone(),
        reply,
        prompt: None,
        slides,
        theme: Some(theme),
        background,
    };
    let mut project = match edit {
        Some((project, from)) => store.append(project, from, new)?,
        None => store.create(Kind::Deck, new)?,
    };
    if edit.is_none() && !plan.title.trim().is_empty() {
        project = store.rename(&project.id, plan.title.trim())?;
    }
    Ok(project)
}

/// How far a deck has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Pictures painted by the image model.
    Painting { done: usize, total: usize },
    /// Slides laid out and drawn by Kairo.
    Designing { done: usize, total: usize },
}

/// Draws every slide's image from its words and art.
fn draw_all(
    store: &ImagineStore,
    theme: &DeckTheme,
    title: &str,
    background: Option<&str>,
    slides: &mut [Slide],
    progress: &(dyn Fn(Step) + Sync),
) -> Result<(), ImagineError> {
    let background = background.map(|f| store.read_file(f)).transpose()?;
    let pages = slides.len();
    for (i, slide) in slides.iter_mut().enumerate() {
        progress(Step::Designing {
            done: i,
            total: pages,
        });
        let art = slide
            .art
            .as_deref()
            .map(|f| store.read_file(f))
            .transpose()?;
        let content = slide.content(i + 1, pages, title, background.is_some());
        let png = deck::render(theme, &content, background.as_deref(), art.as_deref())?;
        slide.file = store.write_image(&png)?;
    }
    Ok(())
}

/// A drawn deck version as a PowerPoint file with editable text, and the
/// deck's title. Older picture-only decks export their slides as pictures.
pub fn export_pptx(
    store: &Mutex<ImagineStore>,
    project: &str,
    version: &str,
) -> Result<(String, Vec<u8>), ImagineError> {
    let store = locked(store);
    let p = store.get(project)?;
    let v = p.version(version).ok_or(ImagineError::Missing)?;
    if v.slides.is_empty() {
        return Err(ImagineError::Api(
            "only decks can be saved as PowerPoint".into(),
        ));
    }
    let theme = v.theme.clone().unwrap_or_default();
    let background = v
        .background
        .as_deref()
        .map(|f| store.read_file(f))
        .transpose()?;
    let pages = v.slides.len();
    let mut slides = Vec::with_capacity(pages);
    for (i, s) in v.slides.iter().enumerate() {
        slides.push(match s.layout {
            Some(_) => {
                let content = s.content(i + 1, pages, &p.title, background.is_some());
                let (page, marks) = deck::compose(&theme, &content);
                crate::pptx::ExportSlide {
                    page,
                    marks,
                    background: background.clone(),
                    art: s.art.as_deref().map(|f| store.read_file(f)).transpose()?,
                    notes: s.notes.clone(),
                }
            }
            // Older picture-only decks: the slide as one picture.
            None => crate::pptx::ExportSlide::picture(store.read_file(&s.file)?, s.notes.clone()),
        });
    }
    Ok((p.title.clone(), crate::pptx::write(&p.title, &slides)?))
}
