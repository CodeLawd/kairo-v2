//! Imagine's own drafts: one JSON file per project and write-once image
//! files. Image files are written before the project that names them, so a
//! failed or cancelled job never changes a finished version.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use kairo_store::fs_atomic::write_atomic;
use serde::{Deserialize, Serialize};

use crate::deck::{Content, DeckTheme, Layout, Point, Shape, Side};
use crate::{ImageModel, ImagineError, Quality};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    pub id: String,
    /// The version this one refined; `None` for the first.
    pub parent: Option<String>,
    /// The prompt (first version) or the refinement instruction.
    pub instruction: String,
    pub quality: Quality,
    /// The model that made it; `None` for drafts made before models were
    /// selectable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ImageModel>,
    pub created: f64,
    /// File name under `files/`. A deck's is its first slide.
    pub file: String,
    /// What the assistant said about this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    /// The prompt the image model was given: the operator's words as the
    /// assistant enhanced them. `None` for decks and older drafts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// A deck version's slides, in order; empty for an image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slides: Vec<Slide>,
    /// A drawn deck's look and its shared background art.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<DeckTheme>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

/// One slide of a deck version. Unchanged files are shared across versions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Slide {
    pub title: String,
    /// The art prompt (decks drawn by Kairo), or the whole-slide prompt
    /// (older decks painted by the image model).
    #[serde(default)]
    pub prompt: String,
    /// The finished slide image.
    pub file: String,
    /// `None` for older decks whose slides are only images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Layout>,
    /// Older decks' bullets (or subtitle, or quotation).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub body: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub eyebrow: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub lead: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<Point>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub callout: String,
    /// Free decorative shapes the assistant placed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<Shape>,
    /// The side a picture or the stronger half of a contrast takes.
    #[serde(default)]
    pub side: Side,
    /// The slide's job in the story (hook, proof, call to action...).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// Speaker notes.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// The slide's own picture, for a text-and-picture slide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub art: Option<String>,
}

impl Slide {
    /// Whether Kairo drew this slide (so its words can be edited).
    pub fn editable(&self) -> bool {
        self.layout.is_some()
    }

    /// The slide's items; older decks' bullets become headings.
    pub fn content_points(&self) -> Vec<Point> {
        if !self.points.is_empty() || self.layout.is_some_and(Layout::is_display) {
            return self.points.clone();
        }
        self.body
            .iter()
            .map(|b| Point {
                heading: b.clone(),
                ..Point::default()
            })
            .collect()
    }

    /// What Kairo draws for this slide, as page `page` of `pages` in `deck`.
    pub fn content(&self, page: usize, pages: usize, deck: &str, background: bool) -> Content {
        let layout = self.layout.unwrap_or_default();
        let lead = if self.lead.is_empty() && layout.is_display() {
            self.body.join("\n")
        } else {
            self.lead.clone()
        };
        Content {
            layout,
            eyebrow: self.eyebrow.clone(),
            title: self.title.clone(),
            lead,
            points: self.content_points(),
            callout: self.callout.clone(),
            page,
            pages,
            deck: deck.to_owned(),
            background,
            art: self.art.is_some(),
            side: self.side,
            shapes: self.shapes.clone(),
        }
    }
}

/// What a project makes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Image,
    Deck,
}

/// A version about to be recorded.
#[derive(Clone, Debug, PartialEq)]
pub struct NewVersion {
    pub instruction: String,
    pub model: ImageModel,
    pub quality: Quality,
    pub file: String,
    pub reply: Option<String>,
    pub prompt: Option<String>,
    pub slides: Vec<Slide>,
    pub theme: Option<DeckTheme>,
    pub background: Option<String>,
}

impl NewVersion {
    fn into_version(self, parent: Option<String>) -> Version {
        Version {
            id: uuid::Uuid::new_v4().to_string(),
            parent,
            instruction: self.instruction.trim().into(),
            quality: self.quality,
            model: Some(self.model),
            created: now(),
            file: self.file,
            reply: self.reply,
            prompt: self.prompt,
            slides: self.slides,
            theme: self.theme,
            background: self.background,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub kind: Kind,
    pub created: f64,
    /// The version shown and refined next.
    pub selected: String,
    pub versions: Vec<Version>,
    /// Versions the operator saved to Media.
    #[serde(default)]
    pub saved: Vec<String>,
}

impl Project {
    pub fn version(&self, id: &str) -> Option<&Version> {
        self.versions.iter().find(|v| v.id == id)
    }

    /// Whether the title is still the one made from the first message
    /// (before the assistant named creations).
    pub fn has_default_title(&self) -> bool {
        self.versions
            .first()
            .is_some_and(|v| self.title == title_of(&v.instruction))
    }

    pub fn current(&self) -> &Version {
        self.version(&self.selected).unwrap_or(&self.versions[0])
    }

    pub fn original(&self) -> &Version {
        &self.versions[0]
    }
}

pub struct ImagineStore {
    root: PathBuf,
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or_default()
}

/// Ids come back from the UI; only uuid-shaped ones name files.
fn valid_id(id: &str) -> bool {
    id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}

fn title_of(prompt: &str) -> String {
    let line = prompt.trim().lines().next().unwrap_or("").trim();
    match line.char_indices().nth(48) {
        Some((cut, _)) => format!("{}…", line[..cut].trim_end()),
        None if line.is_empty() => "Untitled".into(),
        None => line.into(),
    }
}

impl ImagineStore {
    /// Opens `<user_data>/imagine`, removing image files no project names
    /// (left by a job that stopped between writing its image and its project).
    pub fn open(user_data: &Path) -> Result<Self, ImagineError> {
        let store = Self {
            root: user_data.join("imagine"),
        };
        fs::create_dir_all(store.root.join("projects"))?;
        fs::create_dir_all(store.root.join("files"))?;
        store.sweep()?;
        Ok(store)
    }

    pub fn file_path(&self, file: &str) -> PathBuf {
        self.root.join("files").join(file)
    }

    fn project_path(&self, id: &str) -> Result<PathBuf, ImagineError> {
        if !valid_id(id) {
            return Err(ImagineError::Missing);
        }
        Ok(self.root.join("projects").join(format!("{id}.json")))
    }

    /// Every project, newest first. Unreadable files are skipped.
    pub fn list(&self) -> Result<Vec<Project>, ImagineError> {
        let mut projects = Vec::new();
        for entry in fs::read_dir(self.root.join("projects"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json")
                && let Some(project) = fs::read(&path)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Project>(&bytes).ok())
                    .filter(|p| !p.versions.is_empty())
            {
                projects.push(project);
            }
        }
        projects.sort_by(|a, b| b.created.total_cmp(&a.created));
        Ok(projects)
    }

    pub fn get(&self, id: &str) -> Result<Project, ImagineError> {
        let bytes = fs::read(self.project_path(id)?).map_err(|_| ImagineError::Missing)?;
        serde_json::from_slice(&bytes).map_err(|_| ImagineError::Missing)
    }

    fn put(&self, project: &Project) -> Result<(), ImagineError> {
        let json =
            serde_json::to_vec_pretty(project).map_err(|e| ImagineError::Storage(e.to_string()))?;
        write_atomic(&self.project_path(&project.id)?, &json)?;
        Ok(())
    }

    /// Writes image bytes as returned, under a new name. Never overwrites.
    pub fn write_image(&self, bytes: &[u8]) -> Result<String, ImagineError> {
        let file = format!("{}.png", uuid::Uuid::new_v4());
        write_atomic(&self.file_path(&file), bytes)?;
        Ok(file)
    }

    pub fn read_file(&self, file: &str) -> Result<Vec<u8>, ImagineError> {
        fs::read(self.file_path(file)).map_err(|_| ImagineError::Missing)
    }

    pub fn read_image(&self, project: &Project, version: &str) -> Result<Vec<u8>, ImagineError> {
        let version = project.version(version).ok_or(ImagineError::Missing)?;
        fs::read(self.file_path(&version.file)).map_err(|_| ImagineError::Missing)
    }

    pub fn create(&self, kind: Kind, new: NewVersion) -> Result<Project, ImagineError> {
        let title = title_of(&new.instruction);
        let version = new.into_version(None);
        let project = Project {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            kind,
            created: version.created,
            selected: version.id.clone(),
            versions: vec![version],
            saved: Vec::new(),
        };
        self.put(&project)?;
        Ok(project)
    }

    /// Adds a version made from `parent` and selects it.
    pub fn append(
        &self,
        project: &str,
        parent: &str,
        new: NewVersion,
    ) -> Result<Project, ImagineError> {
        let mut project = self.get(project)?;
        if project.version(parent).is_none() {
            return Err(ImagineError::Missing);
        }
        let version = new.into_version(Some(parent.into()));
        project.selected = version.id.clone();
        project.versions.push(version);
        self.put(&project)?;
        Ok(project)
    }

    /// Restores (selects) an earlier version. Nothing is deleted.
    pub fn select(&self, project: &str, version: &str) -> Result<Project, ImagineError> {
        let mut project = self.get(project)?;
        if project.version(version).is_none() {
            return Err(ImagineError::Missing);
        }
        project.selected = version.into();
        self.put(&project)?;
        Ok(project)
    }

    /// Gives a creation a new title.
    pub fn rename(&self, project: &str, title: &str) -> Result<Project, ImagineError> {
        let mut project = self.get(project)?;
        project.title = title_of(title);
        self.put(&project)?;
        Ok(project)
    }

    pub fn mark_saved(&self, project: &str, version: &str) -> Result<Project, ImagineError> {
        let mut project = self.get(project)?;
        if project.version(version).is_none() {
            return Err(ImagineError::Missing);
        }
        if !project.saved.iter().any(|v| v == version) {
            project.saved.push(version.into());
            self.put(&project)?;
        }
        Ok(project)
    }

    pub fn delete(&self, project: &str) -> Result<(), ImagineError> {
        let path = self.project_path(project)?;
        match fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        self.sweep()
    }

    fn sweep(&self) -> Result<(), ImagineError> {
        let used: HashSet<String> = self
            .list()?
            .into_iter()
            .flat_map(|p| {
                p.versions.into_iter().flat_map(|v| {
                    std::iter::once(v.file).chain(v.background).chain(
                        v.slides
                            .into_iter()
                            .flat_map(|s| std::iter::once(s.file).chain(s.art)),
                    )
                })
            })
            .collect();
        for entry in fs::read_dir(self.root.join("files"))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if !used.contains(&name) {
                let _ = fs::remove_file(entry.path());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_short_single_lines() {
        assert_eq!(title_of("  Sunrise over hills\nmore"), "Sunrise over hills");
        assert_eq!(title_of(""), "Untitled");
        assert!(title_of(&"a".repeat(80)).ends_with('…'));
    }

    #[test]
    fn drafts_saved_before_model_choice_still_load() {
        let old = r#"{"id":"a","parent":null,"instruction":"x","quality":"medium","created":1,"file":"f.png"}"#;
        let v: Version = serde_json::from_str(old).unwrap();
        assert_eq!(v.model, None);
        // And are written back without a model field.
        assert!(!serde_json::to_string(&v).unwrap().contains("model"));
        let new = r#"{"id":"a","parent":null,"instruction":"x","quality":"high","model":"gpt-image-2.6-sunburst","created":1,"file":"f.png"}"#;
        let v: Version = serde_json::from_str(new).unwrap();
        assert_eq!(v.model, Some(ImageModel::Sunburst));
    }

    #[test]
    fn rejects_path_like_ids() {
        assert!(!valid_id("../../etc/passwd"));
        assert!(valid_id(&uuid::Uuid::new_v4().to_string()));
    }
}
