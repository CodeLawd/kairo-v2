//! Report Electron changes that the native app may need to port.

use electron_sync::{Options, run};
use std::{path::PathBuf, process::ExitCode, time::Instant};

const USAGE: &str =
    "usage: electron-sync [--root DIR] [--electron DIR] [--write] [--advance] [--apply-generated]";

fn parse() -> Result<Options, String> {
    let mut options = Options {
        root: PathBuf::from("."),
        electron: None,
        write: false,
        advance: false,
        apply_generated: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => options.root = args.next().ok_or(USAGE)?.into(),
            "--electron" => options.electron = Some(args.next().ok_or(USAGE)?.into()),
            "--write" => options.write = true,
            "--advance" => options.advance = true,
            "--apply-generated" => options.apply_generated = true,
            _ => return Err(format!("unknown argument {arg}\n{USAGE}")),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    let started = Instant::now();
    let result = parse().and_then(|options| run(&options));
    match result {
        Ok(report) => {
            print!("{report}");
            println!("\nCheck took {} ms", started.elapsed().as_millis());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("electron-sync: {error}");
            ExitCode::FAILURE
        }
    }
}
