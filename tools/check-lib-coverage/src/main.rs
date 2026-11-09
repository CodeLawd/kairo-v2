//! Run the shared-export inventory using Electron's pinned TypeScript compiler.

use std::{
    path::Path,
    process::{Command, ExitCode},
};

fn main() -> ExitCode {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("check.mjs");
    match Command::new("node")
        .arg(script)
        .args(std::env::args_os().skip(1))
        .status()
    {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("Could not run the coverage checker with Node: {error}");
            ExitCode::FAILURE
        }
    }
}
