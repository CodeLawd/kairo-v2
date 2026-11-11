//! Launch the Electron reference capture against a read-only source checkout.

use std::{
    path::Path,
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (checkout, output, extra) = match args.as_slice() {
        [checkout, output] => (checkout, output, Vec::new()),
        // `visuals FIXTURES` captures the prompt 053 corpus.
        // `programs FIXTURES` captures the prompt 054 corpus.
        [checkout, output, mode, fixtures] if mode == "visuals" || mode == "programs" => {
            (checkout, output, vec![mode.clone(), fixtures.clone()])
        }
        _ => {
            eprintln!(
                "usage: golden-capture ELECTRON_CHECKOUT OUTPUT_DIRECTORY [visuals|programs FIXTURES]"
            );
            return ExitCode::FAILURE;
        }
    };
    let checkout = Path::new(checkout);
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("capture.cjs");
    let result = Command::new("node")
        .arg(checkout.join("node_modules/electron/cli.js"))
        .arg(script)
        .arg(checkout)
        .arg(output)
        .args(extra)
        .status();
    match result {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(status) => {
            eprintln!("Electron capture failed: {status}");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!("Could not start Electron capture through Node: {error}");
            ExitCode::FAILURE
        }
    }
}
