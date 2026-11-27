//! The assistant in front of image generation: it reads the operator's
//! message, decides what to make (an image, a slide deck, an edit, or a
//! question back) and writes the slide plan. One Responses API call with a
//! strict JSON schema.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::ImagineError;
use crate::deck::{DeckTheme, Layout, Point, Shape, Side};
use crate::store::{Kind, Project};

/// Plans and replies. Mid-priced; image cost dominates.
pub const AGENT_MODEL: &str = "gpt-6.1-terra";
/// Most slides a deck may have.
pub const MAX_SLIDES: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// A new single image.
    Image,
    /// A new slide deck.
    Deck,
    /// Change the current image.
    EditImage,
    /// Change some slides of the current deck.
    EditSlides,
    /// Only reply (a question, or advice).
    Ask,
    /// Save the current image to Media moving (`Plan::motion`).
    Motion,
}

/// One slide as the assistant writes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SlidePlan {
    /// The slide's one job in the story.
    #[serde(default)]
    pub role: String,
    pub layout: Layout,
    /// Why this layout fits and where things sit; the assistant's working.
    #[serde(default)]
    pub plan: String,
    /// The side a picture or the stronger half of a contrast takes.
    #[serde(default)]
    pub side: Side,
    /// A short kicker above the title.
    #[serde(default)]
    pub eyebrow: String,
    pub title: String,
    /// The subtitle, the sentence under the title, or the quotation.
    #[serde(default)]
    pub lead: String,
    /// Cards, rows, steps, figures or bars.
    #[serde(default)]
    pub points: Vec<Point>,
    /// A highlighted takeaway at the bottom.
    #[serde(default)]
    pub callout: String,
    /// Free decorative shapes behind the words.
    #[serde(default)]
    pub shapes: Vec<Shape>,
    pub notes: String,
    /// A picture for a text-and-picture slide.
    pub art_prompt: Option<String>,
    /// Keep the current deck's slide at this position unchanged.
    pub keep: bool,
}

/// A whole deck as the assistant writes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeckPlan {
    pub title: String,
    /// The sequence of layouts, written before the slides to check variety.
    #[serde(default)]
    pub rhythm: Vec<String>,
    pub theme: DeckTheme,
    /// The shared background art.
    pub background_prompt: String,
    pub slides: Vec<SlidePlan>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// A short name for what is made, used for the creation and for files
    /// saved from it.
    #[serde(default)]
    pub title: String,
    pub reply: String,
    pub action: Action,
    pub image_prompt: Option<String>,
    pub deck: Option<DeckPlan>,
    /// A `crate::motion::MotionPreset` key: for [`Action::Motion`], and for
    /// an image or edit that should be saved moving.
    #[serde(default)]
    pub motion: Option<String>,
}

const INSTRUCTIONS: &str = "You are Imagine, the creative assistant inside Kairo, presentation software for churches. \
You make images and slide decks.\n\
Decide the action:\n\
- image: they want one picture (a background, artwork, a graphic).\n\
- deck: they want slides or a presentation. Use the slide count they ask for; otherwise 5. At most 12.\n\
- edit_image: they want to change the current image.\n\
- edit_slides: they want to change the current deck (wording, order, style, add or remove slides).\n\
- ask: the request is unclear, or they only asked a question. Reply without making anything.\n\
- motion: they want the current image to move (a motion, moving, animated or living version, a loop). Kairo moves a \
still on screen without video: a slow camera move with an optional atmosphere drawn over it, or depth (the foreground \
cut out and drifting for parallax, which costs one more image). Set motion to the best fit: zoom (slow zoom), dust \
(drift with floating dust), rays (pan with light rays), bokeh (zoom with soft light spots), haze (drift with haze), \
depth (parallax). Kairo saves it to Media moving; your reply says which motion and that it is in Media. Never say \
you cannot animate.\n\
When they ask for a new or changed picture that moves (flowing, drifting, slow motion, a loop, a motion background), \
use image or edit_image and also set motion: Kairo makes the still, then moves it. Write image_prompt for the still \
frame only, leaving out words about movement, looping or video. For deck, edit_slides and ask, motion is null.\n\
For image and edit_image, image_prompt is the operator's request rewritten by an expert prompt \
writer for the image model; the operator's words are often short or loose, so enhance them without changing what \
they asked for. Keep every detail they gave (subject, scripture story, colours, mood, style, anything to avoid) and \
add what they left out, in this order: the subject and action, concrete and specific; the setting and era; the \
composition for a wide 16:9 screen (camera angle, framing, where the subject sits, and calm open space where words \
will go when it is a background); lighting and time of day; a colour palette; the medium or style (photograph, \
painting, illustration...) with its texture; and the mood. Write it as flowing sentences of 60 to 120 words, not a \
list of tags. Keep it reverent and appropriate for church: dignified figures, no gore, no caricature. Unless the \
operator asks for words in the picture, end with: no text, letters or logos. For edit_image, describe the change \
precisely and say what must stay the same (composition, subject, faces, palette) unless they asked to change it. \
deck is null.\n\
For deck and edit_slides, work as a presentation art director and visual systems architect. Kairo \
draws every slide from your structured content; the image model only paints pictures, which never contain text.\n\
LAYOUT ENGINE. Read the exact shape of each slide's information and choose the composition that fits it; never \
force content into a default grid, and never use the same layout on two consecutive slides. Archetypes:\n\
- title: the opening. eyebrow, title, lead as subtitle, callout as a one-line tagline.\n\
- section: a break between parts. eyebrow, title, optional lead.\n\
- hero: monumental statement. One oversized thesis as the title in open space, optional eyebrow and a short lead. No points.\n\
- bento: asymmetric modules for a multi-faceted idea. The first point is the large focal block (label may be a figure), then 2 to 3 smaller supporting points.\n\
- kpi: data spotlight. 1 to 3 massive figures as label (or value), heading a micro-label of 2 to 4 words, text one short line. No long text.\n\
- timeline: sequential flow of 3 to 5 connected milestones; label names the stage (Week 1, 2019), heading and a short text.\n\
- full_bleed: an edge-to-edge picture (art_prompt required) with minimal high-contrast words in its open space; side is where the subject sits, the words go opposite. Up to 2 points.\n\
- split: 60/40 split screen, only to contrast two opposing states (exactly 2 points, label Before/After or similar, the stronger state second) or to pair tight copy with a picture (give art_prompt, up to 3 points). side is where the picture or stronger state sits; alternate left and right across the deck.\n\
- panoramic: headline and takeaway on top, then a wide picture (art_prompt) or a row of 3 to 5 comparison columns below.\n\
- editorial: a minimalist typographic list, 2 to 5 points with bold lead-in headings, a vertical accent line, no cards.\n\
- cards: 2 to 6 even cards. checklist: 3 to 5 rules. steps: 2 to 4 boxed stages. stats: 2 to 4 boxed figures.\n\
- chart: 3 to 7 bars; each point has heading, value (a number) and label (the value as shown). Only real figures the operator gave or well-known ones.\n\
- quote: lead is the quotation, title the attribution.\n\
- image_text: 2 to 4 points with a picture beside them (art_prompt). bullets: only when nothing else fits.\n\
Write rhythm first: one entry per slide, like \"1 hook: hero\", so you can check the sequence varies and alternates dense \
analytical slides with breathing-room slides. For each slide give role (its one job: hook, context, tension, proof, \
mechanism, comparison, vision, call_to_action) and plan (one sentence: why this layout fits the content and where things \
sit on the 16:9 canvas) before its copy.\n\
COPY. Headlines are sharp standalone takeaways of at most 10 words, never vague topic labels. Points use a bold lead-in \
heading and at most 10 words of text; at most 4 points unless the layout says otherwise. Cut filler. Content slides may \
have an eyebrow (2 to 4 words), a lead (one framing sentence) and a callout (one memorable line). Add speaker notes of one \
to three sentences.\n\
PICTURES. Leave 30 to 40 percent of slides as pure typography, data or geometric structure so the visual slides hit \
harder. Lock one art direction for the whole deck in theme.style: medium, lighting, colour palette (matching the theme) and \
texture; every picture follows it. Each art_prompt is a careful brief: a concrete, unexpected visual metaphor for that \
slide's specific idea (never handshakes, puzzle pieces, light bulbs, glowing brains, people pointing at screens or other \
stock clichés), framed for its layout (wide with negative space opposite the subject for full_bleed, one isolated subject \
on a clean backdrop for split, a wide panorama for panoramic, an abstract atmospheric texture for hero, quote, title and \
section), with no words, numbers, labels or interface text in the picture. art_prompt is null on slides without a \
picture; value is null except on chart, kpi and stats slides.\n\
DESIGN DIRECTION. Decide it before writing. If the operator describes one (colours, brand, mood, era, fonts, shapes, \
\"minimal\", \"bold\", \"editorial\", \"playful\"...), follow it exactly. Otherwise make a fresh, distinctive choice that \
fits the subject; do not fall back to the same palette, fonts and ornament every time, and avoid beige-and-navy unless it \
truly fits. Theme: background (content page colour; can be dark), surface (cards), text, muted (secondary text), accent, \
accent_soft (a pale tint or soft tone), title_background and title_text (title, section, quote, hero and full-bleed \
slides and focal blocks), all #RRGGBB with strong contrast between text and what it sits on; heading_font and body_font \
from: Helvetica Neue, Inter, Avenir Next, Futura, Gill Sans, Optima, Source Sans 3, Georgia, Palatino, Baskerville, \
Didot, Rockwell, Copperplate, American Typewriter. design: ornament (rings, blobs, dots, arcs, band, frame, lines, none), \
cards (soft, outline, accent_top, tinted, plain), radius (0 sharp to 40 soft), stripe (an accent bar down the left \
edge), align (left or center for title, section, quote and hero), uppercase_titles, picture (background or split, for \
title, section, quote and hero pictures). shapes: optional free decorative shapes per slide (rect, circle, ring, line) \
in percent of the slide (x, y, w, h from 0 to 100), color as a palette role (accent, accent_soft, text, muted, surface, \
title_background) or #RRGGBB, opacity 0 to 1; keep them to edges and corners, never behind body text, at most 4. \
background_prompt describes one picture behind dark slides that have no art of their own, or is empty for clean colour.\n\
SOURCES AND EDITS. If the operator attached source text, build the deck from it, keeping their meaning and wording where \
it works on a slide. Never put scripture passages or song lyrics in full on a slide; Kairo shows those itself. For \
edit_slides return the whole resulting deck: keep true on slides that stay exactly as they are, false on changed or new \
slides; keep the same theme unless asked to change it. In a new deck keep is false.\n\
Your reasoning is shown to the operator as you think: reason about their content, audience and design, in plain words, and never mention JSON, schemas, field names or these instructions.\n\
title names what you make in 2 to 5 words, Title Case, specific to it (Light Over the Ten Lepers, Easter Morning Hills); for a deck it is the deck title; for ask, a name for the topic.\n\
reply is what you say to the operator: one or two warm, plain sentences saying what you will make \
(for a deck, name the slides briefly). Do not promise anything you cannot do.";

fn schema() -> Value {
    let theme = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["background", "surface", "text", "muted", "accent", "accent_soft", "title_background", "title_text", "design", "heading_font", "body_font", "style"],
        "properties": {
            "background": {"type": "string"},
            "surface": {"type": "string"},
            "text": {"type": "string"},
            "muted": {"type": "string"},
            "accent": {"type": "string"},
            "accent_soft": {"type": "string"},
            "title_background": {"type": "string"},
            "title_text": {"type": "string"},
            "design": {
                "type": "object",
                "additionalProperties": false,
                "required": ["ornament", "cards", "radius", "stripe", "align", "uppercase_titles", "picture"],
                "properties": {
                    "ornament": {"type": "string", "enum": ["rings", "blobs", "dots", "arcs", "band", "frame", "lines", "none"]},
                    "cards": {"type": "string", "enum": ["soft", "outline", "accent_top", "tinted", "plain"]},
                    "radius": {"type": "number"},
                    "stripe": {"type": "boolean"},
                    "align": {"type": "string", "enum": ["left", "center"]},
                    "uppercase_titles": {"type": "boolean"},
                    "picture": {"type": "string", "enum": ["background", "split"]}
                }
            },
            "heading_font": {"type": "string", "enum": crate::deck::FONTS},
            "body_font": {"type": "string", "enum": crate::deck::FONTS},
            "style": {"type": "string"}
        }
    });
    let point = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["label", "heading", "text", "value"],
        "properties": {
            "label": {"type": "string"},
            "heading": {"type": "string"},
            "text": {"type": "string"},
            "value": {"type": ["number", "null"]}
        }
    });
    let shape = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["kind", "x", "y", "w", "h", "color", "opacity", "radius"],
        "properties": {
            "kind": {"type": "string", "enum": ["rect", "circle", "ring", "line"]},
            "x": {"type": "number"}, "y": {"type": "number"},
            "w": {"type": "number"}, "h": {"type": "number"},
            "color": {"type": "string"},
            "opacity": {"type": "number"},
            "radius": {"type": "number"}
        }
    });
    let layouts: Vec<Value> = Layout::ALL
        .iter()
        .map(|l| serde_json::to_value(l).unwrap_or(Value::Null))
        .collect();
    let slide = json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["role", "layout", "plan", "side", "eyebrow", "title", "lead", "points", "callout", "shapes", "notes", "art_prompt", "keep"],
        "properties": {
            "role": {"type": "string", "enum": ["hook", "context", "tension", "proof", "mechanism", "comparison", "vision", "call_to_action"]},
            "layout": {"type": "string", "enum": layouts},
            "plan": {"type": "string"},
            "side": {"type": "string", "enum": ["left", "right"]},
            "eyebrow": {"type": "string"},
            "title": {"type": "string"},
            "lead": {"type": "string"},
            "points": {"type": "array", "items": point},
            "callout": {"type": "string"},
            "shapes": {"type": "array", "items": shape},
            "notes": {"type": "string"},
            "art_prompt": {"type": ["string", "null"]},
            "keep": {"type": "boolean"}
        }
    });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["title", "reply", "action", "image_prompt", "deck", "motion"],
        "properties": {
            "title": {"type": "string"},
            "reply": {"type": "string"},
            "action": {"type": "string", "enum": ["image", "deck", "edit_image", "edit_slides", "ask", "motion"]},
            "image_prompt": {"type": ["string", "null"]},
            "motion": {"type": ["string", "null"], "enum": ["zoom", "dust", "rays", "bokeh", "haze", "depth", null]},
            "deck": {"anyOf": [
                {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "theme", "background_prompt", "rhythm", "slides"],
                    "properties": {
                        "title": {"type": "string"},
                        "rhythm": {"type": "array", "items": {"type": "string"}},
                        "theme": theme,
                        "background_prompt": {"type": "string"},
                        "slides": {"type": "array", "items": slide}
                    }
                },
                {"type": "null"}
            ]}
        }
    })
}

/// What the assistant knows: the open creation and the conversation so far.
fn context(project: Option<&Project>) -> String {
    let Some(p) = project else {
        return "There is no current creation; this starts a new one.".into();
    };
    let current = p.current();
    let mut out = match p.kind {
        Kind::Image => format!(
            "The current creation is an image, last made from: {}",
            current.instruction
        ),
        Kind::Deck => {
            let theme = current
                .theme
                .as_ref()
                .and_then(|t| serde_json::to_string(t).ok())
                .unwrap_or_else(|| "none (an older deck of pictures)".into());
            let mut s = format!(
                "The current creation is a {}-slide deck. Its theme: {theme}.\nSlides:",
                current.slides.len()
            );
            for (i, slide) in current.slides.iter().enumerate() {
                let layout = slide
                    .layout
                    .and_then(|l| serde_json::to_value(l).ok())
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "picture".into());
                let points: Vec<String> = slide
                    .content_points()
                    .iter()
                    .map(|p| {
                        format!("{} {}: {}", p.label, p.heading, p.text)
                            .trim()
                            .to_owned()
                    })
                    .collect();
                s.push_str(&format!(
                    "\n{}. [{layout}, role {}, side {}] eyebrow: {} | title: {} | lead: {} | points: {} | callout: {} | notes: {} | art: {}",
                    i + 1,
                    slide.role,
                    if slide.side == Side::Left { "left" } else { "right" },
                    slide.eyebrow,
                    slide.title,
                    slide.lead,
                    points.join(" / "),
                    slide.callout,
                    slide.notes,
                    slide.prompt
                ));
            }
            s
        }
    };
    out.push_str("\nConversation so far:");
    for v in p.versions.iter().rev().take(8).rev() {
        out.push_str(&format!("\nOperator: {}", v.instruction));
        if let Some(reply) = &v.reply {
            out.push_str(&format!("\nYou: {reply}"));
        }
    }
    out
}

/// The body again, asking for a summary of the assistant's reasoning so
/// the operator can follow it while they wait.
pub fn plan_body_with_thoughts(project: Option<&Project>, message: &str) -> Value {
    let mut body = plan_body(project, message);
    // Enough effort to plan the content and art direction, not just fill the schema.
    body["reasoning"] = json!({"effort": "medium", "summary": "auto"});
    body
}

/// The `/v1/responses` body.
pub fn plan_body(project: Option<&Project>, message: &str) -> Value {
    json!({
        "model": AGENT_MODEL,
        "input": [
            {"role": "system", "content": INSTRUCTIONS},
            {"role": "system", "content": context(project)},
            {"role": "user", "content": message.trim()},
        ],
        "text": {"format": {"type": "json_schema", "name": "imagine_plan", "strict": true, "schema": schema()}},
    })
}

/// The plan in a Responses API answer, checked for what Imagine can do with
/// `project`.
pub fn parse_plan(body: &Value, project: Option<&Project>) -> Result<Plan, ImagineError> {
    let content = body
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        .flat_map(|item| {
            item.get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .collect::<Vec<_>>();
    if content
        .iter()
        .any(|c| c.get("type").and_then(Value::as_str) == Some("refusal"))
    {
        return Err(ImagineError::Refused);
    }
    let text = content
        .iter()
        .find(|c| c.get("type").and_then(Value::as_str) == Some("output_text"))
        .and_then(|c| c.get("text"))
        .and_then(Value::as_str)
        .ok_or_else(|| ImagineError::Api("the assistant gave no answer".into()))?;
    let mut plan: Plan = serde_json::from_str(text)
        .map_err(|_| ImagineError::Api("the assistant's answer was unreadable".into()))?;
    if let Some(deck) = &mut plan.deck {
        deck.slides.truncate(MAX_SLIDES);
    }
    let kind = project.map(|p| p.kind);
    // Edits need something to edit; otherwise make it new.
    plan.action = match (plan.action, kind) {
        (Action::EditImage, Some(Kind::Image)) | (Action::EditSlides, Some(Kind::Deck)) => {
            plan.action
        }
        (Action::EditImage, _) => Action::Image,
        (Action::EditSlides, _) => Action::Deck,
        // Motion moves an image; with none yet there is nothing to move.
        (Action::Motion, Some(Kind::Image)) => {
            let known = plan
                .motion
                .as_deref()
                .and_then(crate::motion::MotionPreset::by_key)
                .is_some_and(|p| !p.is_still());
            if !known {
                plan.motion = Some("dust".into());
            }
            Action::Motion
        }
        // Nothing to move yet: make the picture first, when it said what to draw.
        (Action::Motion, _)
            if plan
                .image_prompt
                .as_deref()
                .is_some_and(|p| !p.trim().is_empty()) =>
        {
            Action::Image
        }
        (Action::Motion, _) => {
            plan.reply = "Make or open an image first, then ask me to set it in motion.".into();
            Action::Ask
        }
        (action, _) => action,
    };
    // Only pictures move, and only in ways Kairo knows.
    let moves = matches!(
        plan.action,
        Action::Image | Action::EditImage | Action::Motion
    );
    plan.motion = plan
        .motion
        .take()
        .filter(|_| moves)
        .and_then(|m| crate::motion::MotionPreset::by_key(&m))
        .filter(|p| !p.is_still())
        .map(|p| p.key.to_owned());
    if plan.action == Action::Motion && plan.motion.is_none() {
        plan.motion = Some("dust".into());
    }
    let needs_prompt = matches!(plan.action, Action::Image | Action::EditImage);
    let needs_slides = matches!(plan.action, Action::Deck | Action::EditSlides);
    if needs_prompt
        && plan
            .image_prompt
            .as_deref()
            .is_none_or(|p| p.trim().is_empty())
    {
        return Err(ImagineError::Api(
            "the assistant didn't say what to draw".into(),
        ));
    }
    if needs_slides && plan.deck.as_ref().is_none_or(|d| d.slides.is_empty()) {
        return Err(ImagineError::Api("the assistant planned no slides".into()));
    }
    if plan.action == Action::Deck
        && let Some(deck) = &mut plan.deck
    {
        deck.slides.iter_mut().for_each(|s| s.keep = false);
    }
    if let Some(deck) = &mut plan.deck {
        vary(&mut deck.slides);
    }
    Ok(plan)
}

/// Keeps the deck's rhythm: no layout twice in a row, and pictures and
/// contrasts alternate sides. Slides kept unchanged are left alone.
pub fn vary(slides: &mut [SlidePlan]) {
    let mut last_side: Option<Side> = None;
    for i in 0..slides.len() {
        if i > 0 && !slides[i].keep && slides[i].layout == slides[i - 1].layout {
            slides[i].layout = slides[i].layout.sibling();
        }
        if matches!(slides[i].layout, Layout::Split | Layout::FullBleed) {
            if !slides[i].keep && last_side == Some(slides[i].side) {
                slides[i].side = slides[i].side.flip();
            }
            last_side = Some(slides[i].side);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(slides: Value) -> Value {
        json!({"title": "AI", "theme": {"background": "#111111", "text": "#FFFFFF", "accent": "#FFAA00",
            "heading_font": "Georgia", "body_font": "Source Sans 3", "style": "watercolour"},
            "background_prompt": "hills", "rhythm": [], "slides": slides})
    }

    fn answer(plan: Value) -> Value {
        json!({"output": [{"type": "message", "role": "assistant", "content": [
            {"type": "output_text", "text": plan.to_string()}
        ]}]})
    }

    #[test]
    fn the_request_is_strict_json_for_the_agent_model() {
        let body = plan_body(None, " make 3 slides ");
        assert_eq!(body["model"], AGENT_MODEL);
        assert_eq!(body["text"]["format"]["strict"], true);
        assert_eq!(body["input"][2]["content"], "make 3 slides");
        let schema = &body["text"]["format"]["schema"];
        assert_eq!(schema["additionalProperties"], false);
        assert_eq!(schema["required"].as_array().unwrap().len(), 6);
        assert!(
            body["input"][0]["content"]
                .as_str()
                .unwrap()
                .contains("never contain text")
        );
    }

    #[test]
    fn a_deck_plan_is_read_and_checked() {
        let plan = parse_plan(
            &answer(json!({"reply": "Here's a 2-slide deck.", "action": "deck", "image_prompt": null,
                "deck": deck(json!([
                    {"role": "hook", "layout": "title", "plan": "", "side": "right", "title": "A", "eyebrow": "", "lead": "sub", "points": [], "callout": "", "shapes": [], "notes": "", "art_prompt": null, "keep": true},
                    {"role": "proof", "layout": "image_text", "plan": "", "side": "right", "title": "B", "eyebrow": "", "lead": "", "points": [{"label": "", "heading": "x", "text": "", "value": null}], "callout": "", "shapes": [], "notes": "n", "art_prompt": "a camera", "keep": false}
                ]))})),
            None,
        )
        .unwrap();
        assert_eq!(plan.action, Action::Deck);
        let deck = plan.deck.unwrap();
        assert_eq!(deck.slides.len(), 2);
        assert_eq!(deck.slides[1].layout, Layout::ImageText);
        assert!(
            deck.slides.iter().all(|s| !s.keep),
            "a new deck keeps nothing"
        );
    }

    #[test]
    fn no_layout_repeats_and_sides_alternate() {
        let slide = |layout, side, keep| SlidePlan {
            role: String::new(),
            layout,
            plan: String::new(),
            side,
            eyebrow: String::new(),
            title: "T".into(),
            lead: String::new(),
            points: Vec::new(),
            callout: String::new(),
            shapes: Vec::new(),
            notes: String::new(),
            art_prompt: None,
            keep,
        };
        let mut slides = vec![
            slide(Layout::Cards, Side::Right, false),
            slide(Layout::Cards, Side::Right, false),
            slide(Layout::Split, Side::Right, false),
            slide(Layout::Editorial, Side::Right, false),
            slide(Layout::Split, Side::Right, false),
            slide(Layout::Editorial, Side::Right, true),
            slide(Layout::Editorial, Side::Right, true),
        ];
        vary(&mut slides);
        assert_eq!(slides[1].layout, Layout::Bento);
        assert_eq!(slides[4].side, Side::Left, "splits alternate sides");
        assert_eq!(
            slides[6].layout,
            Layout::Editorial,
            "kept slides stay as they are"
        );
        assert!(schema().to_string().contains("full_bleed"));
    }

    #[test]
    fn edits_without_a_creation_become_new_ones_and_bad_plans_fail() {
        let plan = parse_plan(
            &answer(json!({"reply": "ok", "action": "edit_image", "image_prompt": "warmer", "deck": null})),
            None,
        )
        .unwrap();
        assert_eq!(plan.action, Action::Image);
        assert!(
            parse_plan(
                &answer(
                    json!({"reply": "ok", "action": "deck", "image_prompt": null, "deck": deck(json!([]))})
                ),
                None
            )
            .is_err()
        );
        let refusal = json!({"output": [{"type": "message", "content": [{"type": "refusal", "refusal": "no"}]}]});
        assert_eq!(parse_plan(&refusal, None), Err(ImagineError::Refused));
    }
    #[test]
    fn motion_moves_an_open_image_and_otherwise_only_replies() {
        let image: Project =
            serde_json::from_value(json!({"id": "p", "title": "Hills", "kind": "image",
            "created": 0.0, "selected": "v", "versions": []}))
            .unwrap();
        let ask = |motion: Value| {
            answer(
                json!({"reply": "Drifting dust it is.", "action": "motion", "image_prompt": null,
                "deck": null, "motion": motion}),
            )
        };
        let plan = parse_plan(&ask(json!("rays")), Some(&image)).unwrap();
        assert_eq!(
            (plan.action, plan.motion.as_deref()),
            (Action::Motion, Some("rays"))
        );
        let plan = parse_plan(&ask(Value::Null), Some(&image)).unwrap();
        assert_eq!(
            plan.motion.as_deref(),
            Some("dust"),
            "a motion is always chosen"
        );
        let plan = parse_plan(&ask(json!("rays")), None).unwrap();
        assert_eq!(plan.action, Action::Ask, "nothing to move yet");
        let new = answer(
            json!({"reply": "ok", "action": "image", "image_prompt": "midnight blue light",
            "deck": null, "motion": "haze"}),
        );
        let plan = parse_plan(&new, None).unwrap();
        assert_eq!(
            (plan.action, plan.motion.as_deref()),
            (Action::Image, Some("haze")),
            "a new picture can move"
        );
        let deck_plan = answer(json!({"reply": "ok", "action": "ask", "image_prompt": null,
            "deck": null, "motion": "haze"}));
        assert_eq!(parse_plan(&deck_plan, None).unwrap().motion, None);
        assert!(schema().to_string().contains("\"motion\""));
    }
}
