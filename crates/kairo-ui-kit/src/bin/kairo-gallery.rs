//! Opens the design-system gallery.
//!
//! `cargo run -p kairo-ui-kit --bin kairo-gallery -- [--light] [--reduced-motion] [--scroll <px>] [--open confirm|alert|settings] [--check-fonts]`

use kairo_ui_kit::{Appearance, KairoAssets};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let appearance = if args.iter().any(|a| a == "--light") {
        Appearance::Light
    } else {
        Appearance::Dark
    };
    let reduced = args.iter().any(|a| a == "--reduced-motion");
    let scroll: f32 = args
        .iter()
        .position(|a| a == "--scroll")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let demo = args
        .iter()
        .position(|a| a == "--open")
        .and_then(|i| args.get(i + 1))
        .cloned();
    let check_fonts = args.iter().any(|a| a == "--check-fonts");
    gpui_kit::application()
        .with_assets(KairoAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            kairo_ui_kit::init(appearance, cx);
            kairo_ui_kit::set_reduced_motion(reduced, cx);
            if check_fonts {
                // Prints the interface families (Inter) the platform text system sees.
                let names = cx.text_system().all_font_names();
                let found: Vec<_> = names.iter().filter(|n| n.starts_with("Inter")).collect();
                println!("inter: {found:?}");
                cx.quit();
                return;
            }
            match kairo_ui_kit::gallery::open(cx) {
                Ok((handle, gallery)) => {
                    gallery.read(cx).scroll_to(scroll);
                    if let Some(demo) = demo.clone() {
                        let _ = handle.update(cx, |_, window, cx| {
                            gallery.update(cx, |gallery, cx| gallery.open_demo(&demo, window, cx));
                        });
                    }
                }
                Err(error) => {
                    eprintln!("kairo-gallery: could not open the window: {error}");
                    cx.quit();
                }
            }
            cx.activate(true);
        });
}
