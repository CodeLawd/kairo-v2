//! Native picker contracts from the IPC handlers. Defaults are supplied at
//! request time because workspace/capture folders and export names can change.
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogKind {
    Workspace,
    SermonNotes,
    MediaFolder,
    ImportMediaFolder,
    ImportImages,
    ImportVideos,
    AddBackgrounds,
    TracksFolder,
    ImportTracksFolder,
    ImportAudio,
    CaptureFolder,
    OverlayImage,
    OverlayVideo,
    ProgramImage,
    ExportSongUsage,
    ExportKairo,
    ImportKairo,
    ImportPdf,
    ImportPowerpoint,
    ImportSlides,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogRequest {
    pub title: Option<&'static str>,
    pub button_label: Option<&'static str>,
    pub directory: bool,
    pub create_directory: bool,
    pub multiple: bool,
    pub save: bool,
    pub filters: Vec<FileFilter>,
    pub default_path: Option<PathBuf>,
}

pub fn dialog_request(kind: DialogKind, default_path: Option<PathBuf>) -> DialogRequest {
    use DialogKind::*;
    let mut request = DialogRequest {
        title: None,
        button_label: None,
        directory: false,
        create_directory: false,
        multiple: false,
        save: false,
        filters: Vec::new(),
        default_path,
    };
    let filters: &[(&'static str, &'static [&'static str])] = match kind {
        Workspace | MediaFolder | ImportMediaFolder | TracksFolder | ImportTracksFolder
        | CaptureFolder => {
            request.directory = true;
            request.create_directory = true;
            request.title = Some(match kind {
                Workspace => "Choose where Kairo keeps songs and media",
                MediaFolder => "Choose your backgrounds folder",
                ImportMediaFolder => "Choose a backgrounds folder for imported files",
                TracksFolder => "Choose your audio folder",
                ImportTracksFolder => "Choose a folder for audio files",
                _ => "Save recordings to",
            });
            if matches!(kind, Workspace | MediaFolder | TracksFolder) {
                request.button_label = Some("Use this folder");
            }
            &[]
        }
        SermonNotes => &[("Sermon notes", &["docx", "pdf", "txt", "md", "rtf"])],
        ImportImages => {
            request.title = Some("Import images");
            request.multiple = true;
            &[(
                "Images",
                &["png", "jpg", "jpeg", "webp", "gif", "avif", "bmp"],
            )]
        }
        ImportVideos => {
            request.title = Some("Import videos");
            request.multiple = true;
            &[("Videos", &["mp4", "webm", "m4v", "mov", "ogv"])]
        }
        AddBackgrounds => {
            request.title = Some("Add backgrounds");
            request.multiple = true;
            &[(
                "Backgrounds",
                &[
                    "mp4", "webm", "m4v", "mov", "ogv", "png", "jpg", "jpeg", "webp", "gif",
                    "avif", "bmp",
                ],
            )]
        }
        ImportAudio => {
            request.title = Some("Import audio");
            request.multiple = true;
            &[("Audio", &["mp3", "wav", "m4a", "aac", "ogg", "flac"])]
        }
        OverlayVideo => &[("Videos", &["mp4", "mov", "m4v", "webm"])],
        OverlayImage => &[("Images", &["png", "jpg", "jpeg", "gif", "webp", "bmp"])],
        ProgramImage => {
            request.title = Some("Choose an image");
            &[(
                "Images",
                &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg"],
            )]
        }
        ExportSongUsage => {
            request.title = Some("Export song usage");
            request.save = true;
            &[("CSV", &["csv"])]
        }
        ExportKairo => {
            request.title = Some("Export to a Kairo file");
            request.save = true;
            &[("Kairo file", &["kairo"])]
        }
        ImportKairo => {
            request.title = Some("Import Kairo files");
            request.multiple = true;
            &[("Kairo files", &["kairo", "json"])]
        }
        ImportPdf => {
            request.title = Some("Import PDF");
            &[("PDF", &["pdf"])]
        }
        ImportPowerpoint => {
            request.title = Some("Import PowerPoint");
            &[("PowerPoint", &["ppt", "pptx"])]
        }
        ImportSlides => {
            request.title = Some("Import slides");
            &[
                ("PDF or PowerPoint", &["pdf", "ppt", "pptx"]),
                ("PDF", &["pdf"]),
                ("PowerPoint", &["ppt", "pptx"]),
            ]
        }
    };
    request.filters = filters
        .iter()
        .map(|(name, extensions)| FileFilter { name, extensions })
        .collect();
    request
}
