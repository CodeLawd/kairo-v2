//! `kairo-engine`: the output process. `kairo` launches it with
//! `--socket <name>` and the handshake token in `KAIRO_ENGINE_TOKEN`, and
//! tells it everything it draws. winit owns the main thread.

mod app;
mod display;
#[cfg(target_os = "macos")]
mod mac;
mod program;
mod server;

use std::process::ExitCode;
use std::time::Duration;
use winit::event_loop::EventLoop;

/// How long the picture is held after the client goes, unless overridden.
const DEFAULT_GRACE_MS: u64 = 10_000;

struct Arguments {
    socket: String,
    grace: Duration,
}

fn arguments() -> Result<Arguments, String> {
    let mut socket = None;
    let mut grace = DEFAULT_GRACE_MS;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--socket" => socket = args.next(),
            "--grace-ms" => {
                grace = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--grace-ms needs a whole number of milliseconds")?;
            }
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Arguments {
        socket: socket
            .filter(|s| !s.is_empty())
            .ok_or("--socket is required")?,
        grace: Duration::from_millis(grace),
    })
}

/// Standard error that drops what it cannot write.
struct QuietStderr;

impl std::io::Write for QuietStderr {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        Ok(())
    }
}

fn main() -> ExitCode {
    // Logs go to stderr; the supervisor writes them into Kairo's log. When
    // the UI dies so does the pipe, and a failed log write must not panic
    // (it did: `eprintln!` from the subscriber), so writes never fail.
    tracing_subscriber::fmt()
        .with_writer(|| QuietStderr)
        .log_internal_errors(false)
        .with_ansi(false)
        .with_target(false)
        .without_time()
        .init();
    let arguments = match arguments() {
        Ok(arguments) => arguments,
        Err(error) => {
            tracing::error!("{error}");
            return ExitCode::from(64);
        }
    };
    let token = std::env::var(kairo_engine_proto::TOKEN_ENV).unwrap_or_default();
    if token.is_empty() {
        tracing::error!("{} is not set", kairo_engine_proto::TOKEN_ENV);
        return ExitCode::from(64);
    }
    // SAFETY: no other thread exists yet. Children of the engine (helpers,
    // web views) must not inherit the token.
    unsafe { std::env::remove_var(kairo_engine_proto::TOKEN_ENV) };

    #[allow(unused_mut)]
    let mut builder = EventLoop::<server::Message>::with_user_event();
    #[cfg(target_os = "macos")]
    {
        // No Dock or Cmd-Tab entry, and never activate over the controls.
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        builder.with_activation_policy(ActivationPolicy::Accessory);
        builder.with_activate_ignoring_other_apps(false);
        builder.with_default_menu(false);
    }
    let event_loop = match builder.build() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            tracing::error!("cannot start the window system: {error}");
            return ExitCode::from(70);
        }
    };
    let outbox = server::Outbox::default();
    if let Err(error) = server::listen(
        &arguments.socket,
        token,
        event_loop.create_proxy(),
        outbox.clone(),
    ) {
        tracing::error!("cannot listen on {}: {error}", arguments.socket);
        return ExitCode::from(71);
    }
    tracing::info!(
        "engine {} listening, protocol {}, pid {}",
        env!("CARGO_PKG_VERSION"),
        kairo_engine_proto::PROTOCOL_VERSION,
        std::process::id()
    );
    let mut engine = app::Engine::new(outbox, event_loop.create_proxy(), arguments.grace);
    match event_loop.run_app(&mut engine) {
        Ok(()) => engine.exit_code.map_or(ExitCode::SUCCESS, ExitCode::from),
        Err(error) => {
            tracing::error!("event loop failed: {error}");
            ExitCode::from(70)
        }
    }
}
