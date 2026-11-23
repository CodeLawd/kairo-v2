//! Writes every template's preview art: `cargo run -p kairo-imagine --example previews -- out_dir`.

fn main() {
    let dir = std::path::PathBuf::from(std::env::args().nth(1).expect("an output directory"));
    std::fs::create_dir_all(&dir).expect("dir");
    for key in [
        "worship",
        "sermon",
        "scripture",
        "announcement",
        "watercolor",
        "stained-glass",
        "cinematic",
        "gradient",
    ] {
        let png = kairo_imagine::preview::template_preview(key, 640, 480).expect(key);
        std::fs::write(dir.join(format!("{key}.png")), png).expect("write");
    }
}
