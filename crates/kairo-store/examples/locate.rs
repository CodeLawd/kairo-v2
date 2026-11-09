//! Prints where this build looks for the Electron app's data, and whether
//! each place exists. Reads only; writes nothing.
//!
//! A packaged-style check: `KAIRO_API_URL=https://… KAIRO_WEB_URL=https://…
//! cargo run --release -p kairo-store --example locate`.

fn main() {
    let paths = match kairo_store::paths::detect() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let show = |label: &str, path: Option<&std::path::Path>| match path {
        Some(path) => println!(
            "{label:<18} {} ({})",
            path.display(),
            if path.exists() { "exists" } else { "missing" }
        ),
        None => println!("{label:<18} (none)"),
    };
    println!("packaged           {}", paths.packaged);
    println!("dev profile        {}", paths.dev_profile);
    show("userData", Some(&paths.user_data));
    show("settings", Some(&paths.store_file("kairo-settings")));
    show("logs", Some(&paths.logs));
    show("electron log", Some(&paths.electron_log_file));
    show("crashDumps", Some(&paths.crash_dumps));
    show("documents", paths.documents.as_deref());
    show("workspace", Some(&paths.default_workspace_root()));
    show("videos", paths.videos.as_deref());
    show("temp", Some(&paths.temp));
    show("exe", paths.exe.as_deref());
    println!("locale             {}", kairo_store::os_locale());
}
