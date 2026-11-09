//! The CEF subprocess entry point, copied into each macOS helper app.
use cef::{args::Args, library_loader::LibraryLoader, *};

wrap_render_process_handler! {
    struct Renderer;
    impl RenderProcessHandler {
        fn on_context_created(&self, _browser: Option<&mut Browser>, frame: Option<&mut Frame>, _context: Option<&mut V8Context>) {
            let Some(frame) = frame else { return };
            let Some(line) = command_line_get_global() else { return };
            let switch = CefString::from("kairo-init");
            if frame.is_main() == 1 && line.has_switch(Some(&switch)) == 1 {
                let script = CefString::from(&line.switch_value(Some(&switch)));
                frame.execute_java_script(Some(&script), None, 0);
            }
        }
    }
}

wrap_app! {
    struct HelperApp;
    impl App {
        fn render_process_handler(&self) -> Option<RenderProcessHandler> {
            Some(Renderer::new())
        }
    }
}

fn main() {
    let args = Args::new();
    let sandboxed = !std::env::args().any(|arg| arg == "--no-sandbox");
    let mut sandbox = sandboxed.then(cef::sandbox::Sandbox::new);
    if let Some(sandbox) = sandbox.as_mut() {
        sandbox.initialize(args.as_main_args());
    }
    let exe = std::env::current_exe().expect("current executable");
    let loader = LibraryLoader::new(&exe, true);
    assert!(loader.load(), "could not load the CEF framework");
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let mut app = HelperApp::new();
    let code = execute_process(
        Some(args.as_main_args()),
        Some(&mut app),
        std::ptr::null_mut(),
    );
    std::process::exit(code);
}
