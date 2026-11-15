//! The electron-builder update feed (`latest-mac.yml`, `latest.yml`,
//! `latest-linux.yml`) and the rules for which file this machine takes and
//! where it may come from (`mac-self-update.ts`).

use std::fmt;

/// The release repository electron-builder publishes to (`build.publish`).
pub const RELEASE_REPO: &str = "CodeLawd/kairo-presenter";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Mac,
    Windows,
    Linux,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arch {
    X64,
    Arm64,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

impl Arch {
    pub fn current() -> Self {
        if cfg!(target_arch = "aarch64") {
            Self::Arm64
        } else {
            Self::X64
        }
    }
}

/// The channel file electron-builder writes for this machine. macOS merges
/// both architectures into one file; Linux on ARM has its own.
pub fn feed_file(platform: Platform, arch: Arch) -> &'static str {
    match (platform, arch) {
        (Platform::Mac, _) => "latest-mac.yml",
        (Platform::Windows, _) => "latest.yml",
        (Platform::Linux, Arch::X64) => "latest-linux.yml",
        (Platform::Linux, Arch::Arm64) => "latest-linux-arm64.yml",
    }
}

/// One `files` entry. `sha512` is base64, as electron-builder writes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateFile {
    pub url: String,
    pub sha512: String,
    pub size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateFeed {
    pub version: String,
    pub files: Vec<UpdateFile>,
    pub path: Option<String>,
    pub sha512: Option<String>,
    pub release_date: Option<String>,
    pub release_notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeedError(pub String);

impl fmt::Display for FeedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Invalid update feed: {}", self.0)
    }
}

impl std::error::Error for FeedError {}

/// A YAML scalar as js-yaml writes one: bare, `'single'` (with `''` for a
/// quote) or `"double"` (with backslash escapes).
fn scalar(raw: &str) -> Result<String, FeedError> {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('\'') {
        let inner = inner
            .strip_suffix('\'')
            .ok_or_else(|| FeedError(format!("unterminated quote: {raw}")))?;
        return Ok(inner.replace("''", "'"));
    }
    if let Some(inner) = raw.strip_prefix('"') {
        let inner = inner
            .strip_suffix('"')
            .ok_or_else(|| FeedError(format!("unterminated quote: {raw}")))?;
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('/') => out.push('/'),
                other => return Err(FeedError(format!("unsupported escape \\{other:?}"))),
            }
        }
        return Ok(out);
    }
    Ok(raw.to_owned())
}

fn split_pair(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    (!key.is_empty() && !key.contains(' ')).then_some((key, value))
}

/// Reads the subset of YAML electron-builder writes: top-level scalars and
/// `files:`, a list of flat maps, indented (js-yaml) or not (PyYAML). Block scalars (`releaseNotes: |`) are read
/// as text. Unknown keys are ignored; a feed without `version` is refused.
pub fn parse_feed(text: &str) -> Result<UpdateFeed, FeedError> {
    let mut version = None;
    let mut files: Vec<UpdateFile> = Vec::new();
    let mut path = None;
    let mut sha512 = None;
    let mut release_date = None;
    let mut release_notes = None;
    let mut current: Option<(Option<String>, Option<String>, Option<u64>)> = None;
    let mut in_files = false;
    let mut lines = text.lines().peekable();

    let finish = |entry: (Option<String>, Option<String>, Option<u64>),
                  files: &mut Vec<UpdateFile>|
     -> Result<(), FeedError> {
        match entry {
            (Some(url), Some(sha512), size) => {
                files.push(UpdateFile { url, sha512, size });
                Ok(())
            }
            _ => Err(FeedError("a file has no url or sha512".into())),
        }
    };

    while let Some(line) = lines.next() {
        let trimmed = line.trim_end();
        if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        let indent = trimmed.len() - trimmed.trim_start().len();
        let content = trimmed.trim_start();
        // PyYAML (the release workflow's arm64 + x64 merge) writes the
        // `files` items at column 0; js-yaml indents them.
        let zero_indent_item = indent == 0 && in_files && content.starts_with("- ");
        if indent == 0 && !zero_indent_item {
            if let Some(entry) = current.take() {
                finish(entry, &mut files)?;
            }
            in_files = false;
            let (key, value) = split_pair(content)
                .ok_or_else(|| FeedError(format!("unexpected line: {content}")))?;
            let value = value.trim();
            match key {
                "files" if value.is_empty() => in_files = true,
                "releaseNotes"
                    if value == "|" || value == "|-" || value == ">" || value == ">-" =>
                {
                    let mut block = Vec::new();
                    while let Some(next) = lines.peek() {
                        if !next.is_empty() && !next.starts_with(' ') {
                            break;
                        }
                        block.push(
                            next.strip_prefix("  ")
                                .unwrap_or(next.trim_start())
                                .to_owned(),
                        );
                        lines.next();
                    }
                    while block.last().is_some_and(|l| l.is_empty()) {
                        block.pop();
                    }
                    release_notes = Some(block.join("\n"));
                }
                "version" => version = Some(scalar(value)?),
                "path" => path = Some(scalar(value)?),
                "sha512" => sha512 = Some(scalar(value)?),
                "releaseDate" => release_date = Some(scalar(value)?),
                "releaseNotes" => release_notes = Some(scalar(value)?),
                _ => {}
            }
            continue;
        }
        if !in_files {
            // Nested content under a key this reader ignores.
            continue;
        }
        let item = if let Some(rest) = content.strip_prefix("- ") {
            if let Some(entry) = current.take() {
                finish(entry, &mut files)?;
            }
            current = Some((None, None, None));
            rest
        } else {
            content
        };
        let entry = current
            .as_mut()
            .ok_or_else(|| FeedError(format!("unexpected line: {content}")))?;
        let (key, value) =
            split_pair(item).ok_or_else(|| FeedError(format!("unexpected line: {item}")))?;
        match key {
            "url" => entry.0 = Some(scalar(value)?),
            "sha512" => entry.1 = Some(scalar(value)?),
            "size" => {
                entry.2 = Some(
                    scalar(value)?
                        .parse()
                        .map_err(|_| FeedError(format!("bad size: {value}")))?,
                )
            }
            _ => {}
        }
    }
    if let Some(entry) = current.take() {
        finish(entry, &mut files)?;
    }
    let version = version
        .filter(|v| !v.is_empty())
        .ok_or_else(|| FeedError("no version".into()))?;
    // Old feeds list only `path` and `sha512`.
    if files.is_empty()
        && let (Some(url), Some(sha)) = (&path, &sha512)
    {
        files.push(UpdateFile {
            url: url.clone(),
            sha512: sha.clone(),
            size: None,
        });
    }
    Ok(UpdateFeed {
        version,
        files,
        path,
        sha512,
        release_date,
        release_notes,
    })
}

fn is_arm(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("arm64") || lower.contains("aarch64")
}

fn ends_with(url: &str, ext: &str) -> bool {
    url.to_ascii_lowercase().ends_with(ext)
}

/// `pickMacZip`: the zip for this architecture, never a dmg.
pub fn pick_mac_zip(files: &[UpdateFile], arch: Arch) -> Option<&UpdateFile> {
    let mut zips = files.iter().filter(|f| ends_with(&f.url, ".zip"));
    match arch {
        Arch::Arm64 => zips.find(|f| is_arm(&f.url)),
        Arch::X64 => zips.find(|f| !is_arm(&f.url)),
    }
}

/// The NSIS installer. An ARM build takes an ARM installer when the release
/// has one; Kairo publishes x64 only, which Windows on ARM runs emulated.
pub fn pick_windows_installer(files: &[UpdateFile], arch: Arch) -> Option<&UpdateFile> {
    let exes: Vec<&UpdateFile> = files.iter().filter(|f| ends_with(&f.url, ".exe")).collect();
    match arch {
        Arch::Arm64 => exes
            .iter()
            .find(|f| is_arm(&f.url))
            .or_else(|| exes.first())
            .copied(),
        Arch::X64 => exes.into_iter().find(|f| !is_arm(&f.url)),
    }
}

/// The AppImage for this architecture.
pub fn pick_linux_appimage(files: &[UpdateFile], arch: Arch) -> Option<&UpdateFile> {
    let mut images = files.iter().filter(|f| ends_with(&f.url, ".appimage"));
    match arch {
        Arch::Arm64 => images.find(|f| is_arm(&f.url)),
        Arch::X64 => images.find(|f| !is_arm(&f.url)),
    }
}

/// Where releases live: `https://github.com/<repo>/releases` in production.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseSource {
    pub releases: String,
}

impl ReleaseSource {
    pub fn github(repo: &str) -> Self {
        Self {
            releases: format!("https://github.com/{repo}/releases"),
        }
    }

    /// The newest published release's feed. GitHub's `latest` skips drafts
    /// and pre-releases, as `releaseType: release` does.
    pub fn feed_url(&self, file: &str) -> String {
        format!("{}/latest/download/{file}", self.releases)
    }

    /// The release list as Atom, for release notes.
    pub fn atom_url(&self) -> String {
        format!("{}.atom", self.releases)
    }

    /// `releaseAssetUrl`: absolute download URL for a release file. Refuses
    /// anything that is not a bare file name or this repository's download.
    pub fn asset_url(&self, version: &str, file: &str) -> Result<String, String> {
        let lower = file.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            if !file.starts_with(&format!("{}/download/", self.releases)) {
                return Err(format!(
                    "Refusing update from an unexpected address: {file}"
                ));
            }
            return Ok(file.to_owned());
        }
        let bare = |s: &str| {
            !s.is_empty()
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        };
        if !bare(file) || file.contains("..") {
            return Err(format!("Unexpected update file name: {file}"));
        }
        if !bare(version) {
            return Err(format!("Unexpected version: {version}"));
        }
        // `encodeURIComponent` leaves a bare name unchanged.
        Ok(format!("{}/download/v{version}/{file}", self.releases))
    }
}

/// The release notes in GitHub's Atom feed for tag `v<version>`, as
/// electron-updater reads them (the entry's HTML `content`).
pub fn release_notes_from_atom(atom: &str, version: &str) -> Option<String> {
    let document = roxmltree::Document::parse(atom).ok()?;
    let tag = format!("/v{version}");
    document
        .descendants()
        .filter(|n| n.has_tag_name("entry"))
        .find(|entry| {
            entry.children().any(|c| {
                c.has_tag_name("link") && c.attribute("href").is_some_and(|h| h.ends_with(&tag))
            })
        })?
        .children()
        .find(|c| c.has_tag_name("content"))
        .and_then(|c| c.text())
        .map(str::to_owned)
        .filter(|n| !n.is_empty())
}
