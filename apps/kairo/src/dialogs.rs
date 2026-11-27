//! Native file panels. Called on the UI thread; the core waits on a channel,
//! never on a blocking platform dialog from a Tokio worker.
use kairo_core::dialogs::DialogRequest;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
pub fn show(request: &DialogRequest) -> Result<Vec<PathBuf>, String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseOK, NSOpenPanel, NSSavePanel};
    use objc2_foundation::{NSArray, NSString, NSURL};
    let mtm = MainThreadMarker::new().ok_or("A native file dialog must run on the main thread.")?;
    let open = (!request.save).then(|| NSOpenPanel::openPanel(mtm));
    let save = request.save.then(|| NSSavePanel::savePanel(mtm));
    let panel: &NSSavePanel = match (&open, &save) {
        (Some(open), _) => open,
        (_, Some(save)) => save,
        _ => unreachable!("one panel kind"),
    };
    if let Some(title) = request.title {
        panel.setTitle(Some(&NSString::from_str(title)));
    }
    if let Some(label) = request.button_label {
        panel.setPrompt(Some(&NSString::from_str(label)));
    }
    panel.setCanCreateDirectories(request.create_directory);
    let extensions: Vec<_> = request
        .filters
        .iter()
        .flat_map(|f| f.extensions.iter())
        .map(|ext| NSString::from_str(ext))
        .collect();
    if !extensions.is_empty() {
        #[allow(deprecated)]
        panel.setAllowedFileTypes(Some(&NSArray::from_retained_slice(&extensions)));
    }
    if let Some(default) = &request.default_path {
        let directory = if request.save {
            default.parent().filter(|p| !p.as_os_str().is_empty())
        } else {
            Some(default.as_path())
        };
        if let Some(directory) = directory {
            panel.setDirectoryURL(Some(&NSURL::fileURLWithPath(&NSString::from_str(
                &directory.to_string_lossy(),
            ))));
        }
        if request.save
            && let Some(name) = default.file_name()
        {
            panel.setNameFieldStringValue(&NSString::from_str(&name.to_string_lossy()));
        }
    }
    if let Some(open) = &open {
        open.setCanChooseDirectories(request.directory);
        open.setCanChooseFiles(!request.directory);
        open.setAllowsMultipleSelection(request.multiple);
    }
    if panel.runModal() != NSModalResponseOK {
        return Ok(Vec::new());
    }
    Ok(match open {
        Some(open) => open
            .URLs()
            .iter()
            .filter_map(|url| url.path().map(|p| PathBuf::from(p.to_string())))
            .collect(),
        None => panel
            .URL()
            .and_then(|url| url.path())
            .map(|p| vec![PathBuf::from(p.to_string())])
            .unwrap_or_default(),
    })
}

#[cfg(target_os = "linux")]
pub fn show(request: &DialogRequest) -> Result<Vec<PathBuf>, String> {
    // Zenity uses GTK's native file chooser. No shell interpolation: paths,
    // labels and filters are passed as separate process arguments.
    let mut command = std::process::Command::new("zenity");
    command.arg("--file-selection");
    if let Some(title) = request.title {
        command.arg(format!("--title={title}"));
    }
    if let Some(label) = request.button_label {
        command.arg(format!("--ok-label={label}"));
    }
    if request.directory {
        command.arg("--directory");
    }
    if request.multiple {
        command.args(["--multiple", "--separator=\n"]);
    }
    if request.save {
        command.args(["--save", "--confirm-overwrite"]);
    }
    if let Some(path) = &request.default_path {
        command.arg(format!("--filename={}", path.display()));
    }
    for filter in &request.filters {
        command.arg(format!(
            "--file-filter={} | {}",
            filter.name,
            filter
                .extensions
                .iter()
                .map(|ext| format!("*.{ext}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }
    let output = command
        .output()
        .map_err(|e| format!("Could not open the native file picker: {e}"))?;
    if output.status.code() == Some(1) {
        return Ok(Vec::new());
    }
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect())
}

#[cfg(target_os = "windows")]
pub fn show(request: &DialogRequest) -> Result<Vec<PathBuf>, String> {
    // Windows Forms delegates to the operating system's common file dialogs.
    // Data enters through environment variables, never PowerShell source.
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
if ($env:KAIRO_PICK_DIRECTORY -eq '1') {
    $d = New-Object System.Windows.Forms.FolderBrowserDialog
    $d.Description = $env:KAIRO_PICK_TITLE
    $d.SelectedPath = $env:KAIRO_PICK_DEFAULT
    $d.ShowNewFolderButton = $true
    if ($d.ShowDialog() -eq 'OK') { [Console]::WriteLine($d.SelectedPath) }
} else {
    if ($env:KAIRO_PICK_SAVE -eq '1') { $d = New-Object System.Windows.Forms.SaveFileDialog }
    else { $d = New-Object System.Windows.Forms.OpenFileDialog; $d.Multiselect = ($env:KAIRO_PICK_MULTIPLE -eq '1') }
    $d.Title = $env:KAIRO_PICK_TITLE
    if ($env:KAIRO_PICK_FILTER) { $d.Filter = $env:KAIRO_PICK_FILTER }
    $d.FileName = $env:KAIRO_PICK_DEFAULT
    if ($d.ShowDialog() -eq 'OK') { foreach ($p in $d.FileNames) { [Console]::WriteLine($p) } }
}
"#;
    let filter = request
        .filters
        .iter()
        .map(|f| {
            format!(
                "{}|{}",
                f.name,
                f.extensions
                    .iter()
                    .map(|ext| format!("*.{ext}"))
                    .collect::<Vec<_>>()
                    .join(";")
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    let mut command = std::process::Command::new("powershell.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: only the picker shows, not PowerShell's terminal.
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .args(["-NoProfile", "-STA", "-Command", script])
        .env("KAIRO_PICK_TITLE", request.title.unwrap_or(""))
        .env(
            "KAIRO_PICK_DIRECTORY",
            if request.directory { "1" } else { "0" },
        )
        .env("KAIRO_PICK_SAVE", if request.save { "1" } else { "0" })
        .env(
            "KAIRO_PICK_MULTIPLE",
            if request.multiple { "1" } else { "0" },
        )
        .env(
            "KAIRO_PICK_DEFAULT",
            request
                .default_path
                .as_deref()
                .unwrap_or(std::path::Path::new("")),
        )
        .env("KAIRO_PICK_FILTER", filter)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect())
}

/// Asynchronous handoff to the UI thread that owns native file panels.
pub trait NativeDialogs: Send + Sync {
    fn pick(
        &self,
        request: DialogRequest,
    ) -> kairo_orchestrator::voice::BoxFuture<'_, Result<Vec<PathBuf>, String>>;
}
