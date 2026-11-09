use chrono::{FixedOffset, Utc};
use kairo_domain::json::to_js_value;
use kairo_domain::program::{
    ProgramTimerState, next_time_of_day, normalize_presentation_settings, timer_readout,
};
use kairo_oracle::assert_all_match;
use serde_json::{Value, json};

#[test]
fn time_of_day_matches_today_tomorrow_and_exact_deadline() {
    let dates = [
        0.0,
        3_600_000.0,
        36_000_000.0,
        36_000_001.0,
        86_399_999.0,
        1_783_721_999_999.0,
    ];
    let cases: Vec<_> = [("UTC", 0), ("Africa/Lagos", 3600), ("Asia/Kolkata", 19_800)]
        .into_iter()
        .flat_map(|(timezone, offset)| {
            let zone = FixedOffset::east_opt(offset).unwrap();
            dates.into_iter().flat_map(move |now| {
                ["00:00", "01:00", "10:00", "23:59", "24:00", "-1:15", "0:90"]
                    .into_iter()
                    .map(move |target| {
                        (
                            json!([target, now, timezone]),
                            json!(next_time_of_day(target, now, &zone)),
                        )
                    })
            })
        })
        .collect();
    assert_all_match("oracle:program.nextTimeOfDayInZone", &cases);
    assert!(next_time_of_day("broken", 0.0, &Utc).is_nan());
}

#[test]
fn readout_matches_for_running_paused_and_overrun() {
    let mut cases = Vec::new();
    for ends_at in [None, Some(3000.0)] {
        for remaining_sec in [5.0, 2.0, 0.0, -3.0] {
            for now in [0.0, 2001.0, 3000.0, 3999.0, 5000.0] {
                for rollover in [false, true] {
                    let timer = ProgramTimerState {
                        duration_sec: 5.0,
                        ends_at,
                        remaining_sec,
                    };
                    let readout = timer_readout(&timer, now, rollover);
                    cases.push((json!([timer, now, rollover]), json!({
                        "seconds": readout.seconds, "timeUp": readout.time_up, "paused": readout.paused
                    })));
                }
            }
        }
    }
    assert_all_match("program.timerReadout", &cases);
}

#[test]
fn normalizes_legacy_and_hostile_program_settings() {
    let cases = [
        Value::Null,
        json!(false),
        json!({"camera":{"sizePct":45,"marginPct":3,"corner":"top-left","layout":"pip"}}),
        json!({"timer":{"rollover":false,"color":"#abc","overrunColor":"not-color"},"timers":[{"id":" first ","targetTime":" 23:30 ","theme":{"objects":[{"kind":"shape","id":"shape","x":2,"w":0.75}]}},{"id":"first"}]}),
        json!({"logo":{"mediaPath":" /tmp/logo.png "},"props":[{"id":"old","mediaPath":"old.png","position":"center"},{"id":"new","objects":[{"id":"image","kind":"image","mediaPath":"new.png"},{"id":"image","kind":"image","mediaPath":"duplicate.png"}]}],"stageDisplays":[{"id":"a","enabled":true,"displayId":1},{"id":"b","enabled":true,"displayId":1}]}),
        json!({"transition":{"durationMs":5000,"kind":"fade"},"question":{"fontSizePx":-7,"align":"right"},"audio":{"volume":2},"messages":[{"id":"a","text":" hi "},{"id":"a","text":"duplicate"}]}),
    ].into_iter().map(|raw| {
        let result = normalize_presentation_settings(&raw);
        (json!([raw]), to_js_value(&result).unwrap())
    }).collect::<Vec<_>>();
    assert_all_match("program.normalizePresentationSettings", &cases);
}
