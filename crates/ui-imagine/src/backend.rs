//! What the Imagine page asks for. The application implements
//! [`ImagineBackend`] over Imagine's store, the assistant, the key in the
//! keychain, the media library and Slides; tests drive a scripted fake.

use std::path::PathBuf;
use std::sync::Arc;

pub use kairo_imagine::{
    Action, DeckTheme, ImageModel, Kind, Layout, Project, Quality, Slide, Version,
};

/// Source text the operator attached for the assistant.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    pub name: String,
    pub text: String,
    /// The file was longer than the assistant is sent.
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImagineSnapshot {
    /// An OpenAI key is saved (or held for this session).
    pub connected: bool,
    pub projects: Vec<Project>,
    /// Set when the key can't be kept past this launch.
    pub notice: Option<String>,
}

/// One request. Jobs are numbered by the page so it can cancel them.
#[derive(Clone, Debug, PartialEq)]
pub enum ImagineCall {
    /// Answers [`ImagineAnswer::Snapshot`].
    Load,
    /// Checks and saves the key. Answers [`ImagineAnswer::Snapshot`].
    Connect(String),
    /// Forgets the key. Answers [`ImagineAnswer::Snapshot`].
    Disconnect,
    /// A message to the assistant about `project` (or a new creation). The
    /// assistant plans ([`ImagineFeed::Planned`]), then makes the image or
    /// deck ([`ImagineFeed::Progress`]). Answers [`ImagineAnswer::Project`],
    /// or [`ImagineAnswer::Reply`] when it only replied.
    Send {
        job: u64,
        project: Option<String>,
        /// The version changes start from.
        from: Option<String>,
        /// What the operator wrote.
        message: String,
        /// A template's style, added for the assistant.
        style: Option<String>,
        /// Attached source text.
        attachment: Option<Attachment>,
        model: ImageModel,
        quality: Quality,
    },
    /// Picks a document (txt, md, pdf, docx) and reads its text. Answers
    /// [`ImagineAnswer::Attachment`], or [`ImagineAnswer::Done`] when the
    /// picker was cancelled.
    PickAttachment,
    /// Stops a job; its call answers `Err("Cancelled.")`. Answers
    /// [`ImagineAnswer::Done`].
    Cancel(u64),
    /// Restores a version. Answers [`ImagineAnswer::Project`].
    Select { project: String, version: String },
    /// Copies an image version into the media library (not live). Answers
    /// [`ImagineAnswer::Project`] with the version marked saved.
    /// `motion` names a `kairo_imagine::motion::MotionPreset`; a depth
    /// preset makes one more image request for the parallax layer.
    SaveToMedia {
        project: String,
        version: String,
        motion: Option<String>,
    },
    /// Draws a `kairo_imagine::motion::MotionLoop` locally (no image
    /// request) and adds it to the media library, moving. Answers
    /// [`ImagineAnswer::Done`].
    SaveLoop(String),
    /// Adds a deck version to Slides. Answers [`ImagineAnswer::Project`]
    /// with the version marked saved.
    SaveToSlides { project: String, version: String },
    /// Writes a deck version as a PowerPoint file to Downloads and shows it
    /// in Finder. Answers [`ImagineAnswer::File`].
    DownloadPptx { project: String, version: String },
    /// Answers [`ImagineAnswer::Done`].
    Delete(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ImagineAnswer {
    Done,
    Snapshot(ImagineSnapshot),
    Project(Project),
    /// The assistant answered without making anything.
    Reply(String),
    Attachment(Attachment),
    /// The open image was saved to Media moving, as the assistant chose.
    Moved {
        reply: String,
        /// The `MotionPreset` key.
        motion: String,
        project: Project,
        /// The picture was made by this message (its reply is the version's).
        made: bool,
    },
    /// A file written for the operator.
    File(PathBuf),
}

/// Progress of a [`ImagineCall::Send`].
#[derive(Clone, Debug, PartialEq)]
pub enum ImagineFeed {
    /// The account changed (restored at launch, signed in or out): whether
    /// a key is connected may have changed with it, so the page loads again.
    AccountChanged,
    /// The assistant's reply and what it will make.
    Planned {
        job: u64,
        reply: String,
        action: Action,
        /// Slides in the planned deck (0 for an image).
        slides: usize,
        /// Their titles, in order.
        titles: Vec<String>,
    },
    /// More of the assistant's reasoning summary, while it plans.
    Thought { job: u64, text: String },
    /// Images drawn so far.
    Progress { job: u64, done: usize, total: usize },
    /// Kairo is laying out and drawing slide `done + 1` of `total`.
    Designing { job: u64, done: usize, total: usize },
}

pub type Reply = Box<dyn FnOnce(Result<ImagineAnswer, String>) + Send>;
pub type FeedSink = Arc<dyn Fn(ImagineFeed) + Send + Sync>;

pub trait ImagineBackend: Send + Sync + 'static {
    fn call(&self, call: ImagineCall, reply: Reply);
    /// Where feeds go. Called once, when the page opens.
    fn listen(&self, sink: FeedSink);
    /// Where a version's image file is, for drawing it.
    fn image_path(&self, file: &str) -> PathBuf;
}
