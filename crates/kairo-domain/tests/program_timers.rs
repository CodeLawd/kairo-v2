use kairo_domain::program::{
    ProgramTimerState, idle_timer, pause_timer, reset_timer, start_timer, start_timer_for,
    timer_readout,
};

#[test]
fn countdown_preserves_overrun_when_paused_and_resumed() {
    let idle = ProgramTimerState {
        duration_sec: 10.0,
        remaining_sec: 10.0,
        ends_at: None,
    };
    let run = start_timer(&idle, 1000.0);
    assert_eq!(timer_readout(&run, 10_001.0, false).seconds, 1.0);
    assert!(timer_readout(&run, 11_000.0, false).time_up);
    assert_eq!(timer_readout(&run, 12_001.0, false).seconds, 0.0);
    let paused = pause_timer(&run, 12_001.0);
    let readout = timer_readout(&paused, 20_000.0, true);
    assert_eq!(readout.seconds, -1.0);
    assert!(readout.time_up && readout.paused);
    assert_eq!(start_timer(&paused, 20_000.0).ends_at, Some(19_000.0));
    assert_eq!(reset_timer(&paused, Some(86_401.0)).duration_sec, 86_400.0);
    assert_eq!(reset_timer(&paused, Some(2.5)).duration_sec, 3.0);
}

#[test]
fn time_of_day_start_resolves_the_deadline_again_after_pausing() {
    let def = serde_json::from_value(serde_json::json!({
        "id": "wall", "name": "Wall", "kind": "countdownTo",
        "durationSec": 0, "targetTime": "09:30", "overrun": false,
        "theme": kairo_domain::program::default_timer_theme("#5eead4", "#f87171")
    }))
    .unwrap();
    let idle = idle_timer(&def, 1_000.0, |target, now| {
        assert_eq!((target, now), ("09:30", 1_000.0));
        10_001.0
    });
    assert_eq!(idle.duration_sec, 10.0);
    let run = start_timer_for(&def, &idle, 2_000.0, |_, _| 10_001.0);
    let paused = pause_timer(&run, 10_500.0);
    let next = start_timer_for(&def, &paused, 11_000.0, |_, _| 86_410_001.0);
    assert_eq!(next.ends_at, Some(86_410_001.0));
    assert_eq!(next.duration_sec, 86_400.0);
}

#[test]
fn countdown_display_formats_hours_and_overrun() {
    use kairo_domain::program::format_timer;
    assert_eq!(format_timer(300.9), "5:00");
    assert_eq!(format_timer(3723.0), "1:02:03");
    assert_eq!(format_timer(-12.9), "-0:12");
    assert_eq!(format_timer(f64::NAN), "0:00");
}

#[test]
#[ignore = "release timing gate"]
fn program_state_rebuild_under_50_microseconds() {
    use kairo_domain::program::{
        PresentationSettings, ProgramState, ProgramTimerDef, program_state_with_timer_run,
    };
    use std::{hint::black_box, time::Instant};
    let defs: Vec<_> = (0..20)
        .map(|i| ProgramTimerDef {
            id: format!("timer-{i}"),
            ..ProgramTimerDef::default()
        })
        .collect();
    let timer = ProgramTimerState {
        duration_sec: 300.0,
        ends_at: Some(312_000.0),
        remaining_sec: 300.0,
    };
    let state = ProgramState {
        screen_timer_id: Some(defs[10].id.clone()),
        message: Some("Welcome to the service".into()),
        stage_message: Some("Five minutes remaining".into()),
        active_prop_ids: (0..12).map(|i| format!("prop-{i}")).collect(),
        timers: defs
            .iter()
            .map(|def| (def.id.clone(), timer.clone()))
            .collect(),
        countdown_on_screens: true,
        ..ProgramState::default()
    };
    let settings = PresentationSettings {
        timers: defs,
        ..PresentationSettings::default()
    };
    let new_run = pause_timer(&timer, 15_500.0);
    let id = "timer-10".to_owned();
    let rebuilt = program_state_with_timer_run(&state, &settings, id.clone(), new_run.clone());
    assert_eq!(rebuilt.timer, new_run);
    let mut times: Vec<_> = (0..21)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..1_000 {
                black_box(program_state_with_timer_run(
                    black_box(&state),
                    black_box(&settings),
                    id.clone(),
                    new_run.clone(),
                ));
            }
            start.elapsed().as_secs_f64() * 1_000.0
        })
        .collect();
    times.sort_by(f64::total_cmp);
    println!(
        "ProgramState rebuild median {:.3} µs, slowest batch mean {:.3} µs",
        times[10], times[20]
    );
    assert!(times[10] < 50.0);
}
