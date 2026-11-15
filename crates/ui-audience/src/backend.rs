//! What the Engage page reads and asks for. The application implements
//! [`AudienceBackend`] over `AudienceService`, `EngageFlows`, the account and
//! settings (`window.api.audience.*` in Electron); tests drive a scripted fake.

use std::sync::Arc;

use kairo_domain::audience::{
    AudienceQuestionState, AudienceRoomSettings, AudienceSourceOptions, AudienceState,
    EngageCommand, EngagementIndex, EngagementInput,
};
use kairo_domain::program::ProgramQuestionStyle;

/// The Kairo account as Engage needs it (`useAccountStore`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccountView {
    /// Signed in (active or stale).
    pub signed_in: bool,
    /// Signed in and reachable (`session.state === 'active'`).
    pub online: bool,
}

/// What the page reads at start.
#[derive(Clone, Debug, PartialEq)]
pub struct AudienceSnapshot {
    pub state: AudienceState,
    pub account: AccountView,
    /// `presentation.question`, defaults filled.
    pub style: ProgramQuestionStyle,
}

/// A push from the core.
#[derive(Clone, Debug, PartialEq)]
pub enum AudienceFeed {
    /// `audience:changed`.
    State(Box<AudienceState>),
    Account(AccountView),
    Style(Box<ProgramQuestionStyle>),
}

/// One `window.api.audience` request.
#[derive(Clone, Debug, PartialEq)]
pub enum AudienceCall {
    GetState,
    Start {
        kind: String,
        options: AudienceSourceOptions,
    },
    Stop(String),
    Open(String),
    Moderate {
        id: String,
        state: AudienceQuestionState,
    },
    Show(String),
    Clear,
    ShowJoinCode,
    ShowPoll(String),
    ClearPoll,
    Command(EngageCommand),
    SetRoomSettings(AudienceRoomSettings),
    Invite {
        email: String,
        name: String,
    },
    Revoke(String),
    ListEngagements,
    CreateEngagement(EngagementInput),
    DeleteEngagement(String),
    Resume(String),
    Reopen,
    OpenAudiencePage,
}

/// What a call answers.
#[derive(Clone, Debug, PartialEq)]
pub enum AudienceAnswer {
    State(Box<AudienceState>),
    Engagements(Vec<EngagementIndex>),
    /// `audience:page`: the address the page opens in the browser.
    Url(String),
    Done,
}

pub type Reply<T> = Box<dyn FnOnce(Result<T, String>) + Send>;
pub type FeedSink = Arc<dyn Fn(AudienceFeed) + Send + Sync>;
/// Dropping it stops the feed.
pub type Listening = Box<dyn Send>;

pub trait AudienceBackend: Send + Sync + 'static {
    fn snapshot(&self) -> AudienceSnapshot;
    fn listen(&self, sink: FeedSink) -> Listening;
    /// Errors come back cleaned for showing (`cleanIpcError`).
    fn call(&self, call: AudienceCall, reply: Reply<AudienceAnswer>);
    /// `presentation.question`.
    fn save_style(&self, style: ProgramQuestionStyle, reply: Reply<()>);
    /// Saves the join QR picture where downloads go (`downloadQrCode`).
    fn save_qr(&self, png: Vec<u8>, file_name: String, reply: Reply<()>);
    /// Per-viewer storage (`localStorage` in Electron).
    fn read_ui(&self, key: &str) -> Option<String>;
    fn write_ui(&self, key: &str, value: Option<String>);
}
