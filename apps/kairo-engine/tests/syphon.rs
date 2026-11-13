//! A Syphon surface in the real engine, received by a Syphon client in this
//! process the way OBS finds it (through the Syphon directory): a slide
//! arrives, a rename restarts the server under the new name, and closing
//! the surface takes the server away. Runs on the main thread (no harness),
//! because discovery needs its run loop.

#[cfg(target_os = "macos")]
fn main() {
    use kairo_core::engine::*;
    use kairo_domain::overlay::OverlayBackgroundType;
    use kairo_domain::theme_library::BUILT_IN_THEMES;
    use kairo_engine_proto::*;
    use kairo_native_sys::syphon::receive;
    use std::time::Duration;

    if std::env::args().any(|a| a == "--list") {
        println!("syphon_surface_end_to_end: test");
        return;
    }
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mut options = EngineOptions::new(env!("CARGO_BIN_EXE_kairo-engine"));
    options.grace = Duration::from_secs(2);
    let engine = runtime.block_on(async { EngineHandle::spawn(options) });
    let mut events = engine.subscribe();
    // Waits for `output`'s next status.
    let mut status = |want: &str| -> SurfaceStatus {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        runtime.block_on(async {
            loop {
                // One deadline for the wait: health and stats events keep arriving.
                let event = tokio::time::timeout_at(deadline, events.recv())
                    .await
                    .unwrap_or_else(|_| panic!("timed out waiting for {want}"))
                    .expect("events");
                match event {
                    EngineEvent::Engine(Event::SurfaceStatus { output, status })
                        if output == "y" =>
                    {
                        return status;
                    }
                    EngineEvent::Lost { reason, .. }
                        if reason.contains("exit status: 70")
                            && std::env::var_os("KAIRO_REQUIRE_ENGINE").is_none() =>
                    {
                        eprintln!("skipped: no window system ({reason})");
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        })
    };
    let open = |name: &str| {
        engine.command(Command::OpenSurface(SurfaceSpec {
            output: "y".into(),
            label: "Syphon (test)".into(),
            sink: SinkSpec::syphon(name, 0),
        }))
    };
    let unique = format!("Syphon test {}", std::process::id());
    open(&unique);
    assert_eq!(status("ready"), SurfaceStatus::Ready);

    let mut theme = BUILT_IN_THEMES[0].theme.clone();
    theme.background.kind = OverlayBackgroundType::Color;
    theme.background.color = "#1E3CC8".into();
    theme.background.opacity = 1.0;
    let sent = engine.command(Command::ShowSlide {
        output: "y".into(),
        source: "y".into(),
        slide: SlideContent {
            reference: "John 3:16 (KJV)".into(),
            text: "For God so loved the world".into(),
            theme: Json::encode(&theme),
            colored_lines: None,
        },
        instant: true,
    });
    assert!(sent.accepted);
    assert!(runtime.block_on(engine.wait_frame("y", sent.seq, Duration::from_secs(10))));

    let got = receive(&format!("Kairo {unique}"), 10_000.0, 200.0).expect("a Syphon frame");
    assert_eq!((got.width, got.height), (1920, 1080));
    // The corner is the slide's colour, opaque: BGRA of #1E3CC8.
    assert_eq!(&got.bgra[..4], &[0xC8, 0x3C, 0x1E, 0xFF]);

    // A rename restarts the server under the new name; the old one leaves.
    let renamed = format!("{unique} renamed");
    // Ready stays Ready, so no new status is sent; the server is the proof.
    open(&renamed);
    let got = receive(&format!("Kairo {renamed}"), 10_000.0, 200.0).expect("renamed server");
    assert_eq!(
        &got.bgra[..4],
        &[0xC8, 0x3C, 0x1E, 0xFF],
        "the slide comes back"
    );
    assert!(receive(&format!("Kairo {unique}"), 1_000.0, 0.0).is_err());

    // Closing the surface takes the server away.
    engine.command(Command::CloseSurface {
        output: "y".into(),
        destroy: true,
    });
    std::thread::sleep(Duration::from_millis(500));
    assert!(receive(&format!("Kairo {renamed}"), 1_000.0, 0.0).is_err());
    engine.shutdown();
    println!("test syphon_surface_end_to_end ... ok");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
