//! Every error, empty and failure path of `tools/ts-oracle/runner.mjs`.
//! Needs Node, so it runs only with `KAIRO_ORACLE_MODE=live` or `refresh`.

use kairo_oracle::{Mode, Oracle, electron_dir, mode_from_env, number, undefined};
use serde_json::{Value, json};
use std::path::Path;

fn oracle() -> Option<Oracle> {
    if mode_from_env().ok()? == Mode::Cache {
        eprintln!("skipped: runner tests need Node; set KAIRO_ORACLE_MODE=live");
        return None;
    }
    let probe = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/probe");
    let unused_cache = std::env::temp_dir().join("kairo-oracle-runner-unused");
    Some(Oracle::new(Mode::Live, unused_cache, electron_dir()).with_lib(probe))
}

fn one(oracle: &mut Oracle, name: &str, args: Value) -> Result<Value, String> {
    oracle.results(name, &[args]).map(|mut r| r.remove(0))
}

#[test]
fn values_round_trip() {
    let Some(mut o) = oracle() else { return };
    let tag = |t: &str| json!({"$oracle": t});
    let cases = [
        ("probe.nothing", json!([]), undefined()),
        ("probe.nil", json!([]), Value::Null),
        (
            "probe.withUndefinedField",
            json!([]),
            json!({"a": 1, "b": tag("undefined")}),
        ),
        ("probe.sparse", json!([]), json!([1, tag("undefined"), 3])),
        (
            "probe.nonFinite",
            json!([]),
            json!([
                number(f64::NAN),
                number(f64::INFINITY),
                number(f64::NEG_INFINITY)
            ]),
        ),
        ("probe.negativeZero", json!([]), json!(0)),
        ("probe.resolves", json!([4]), json!(8)),
        ("probe.noisy", json!([3]), json!(3)),
        ("probe.defaulted", json!([1, undefined()]), json!(6)),
        ("probe.defaulted", json!([1]), json!(6)),
        ("probe.notAFunction", json!([]), json!(1)),
        (
            "probe.echo",
            json!([number(f64::NAN), undefined(), null, -0.0]),
            json!([number(f64::NAN), tag("undefined"), null, 0]),
        ),
        (
            "probe.collections",
            json!([]),
            json!({
                "when": {"$oracle": "date", "value": "2026-01-02T00:00:00.000Z"},
                "map": {"$oracle": "map", "value": [["k", 1]]},
                "set": {"$oracle": "set", "value": [1, 2]},
                "bytes": {"$oracle": "bytes", "value": [1, 255]},
            }),
        ),
    ];
    for (name, args, expected) in cases {
        let actual = one(&mut o, name, args.clone()).unwrap();
        assert_eq!(actual, expected, "{name} {args}");
    }
}

#[test]
fn tagged_arguments_are_decoded() {
    let Some(mut o) = oracle() else { return };
    for (arg, kind) in [
        (json!({"$oracle": "map", "value": [["k", 1]]}), "map"),
        (json!({"$oracle": "set", "value": [1]}), "set"),
        (
            json!({"$oracle": "date", "value": "2026-01-02T00:00:00.000Z"}),
            "date",
        ),
        (json!({"$oracle": "bytes", "value": [1]}), "bytes"),
        (undefined(), "undefined"),
    ] {
        assert_eq!(
            one(&mut o, "probe.typeOf", json!([arg])).unwrap(),
            json!(kind)
        );
    }
}

#[test]
fn throws_and_rejections_are_results() {
    let Some(mut o) = oracle() else { return };
    let thrown = one(&mut o, "probe.fails", json!([])).unwrap();
    assert_eq!(thrown["$oracle"], "throw");
    assert_eq!(thrown["name"], "RangeError");
    assert_eq!(thrown["message"], "out of range");
    assert_eq!(
        one(&mut o, "probe.failsWithString", json!([])).unwrap()["message"],
        "plain"
    );
    assert_eq!(
        one(&mut o, "probe.rejects", json!([])).unwrap()["name"],
        "TypeError"
    );
}

#[test]
fn harness_failures_are_errors() {
    let Some(mut o) = oracle() else { return };
    for (name, args, message) in [
        ("probe.missing", json!([]), "is not exported"),
        (
            "probe.notAFunction",
            json!([1]),
            "is not an exported function",
        ),
        ("oracle:missing-helper.fn", json!([]), "Cannot find module"),
        ("nonexistent.fn", json!([]), "Cannot find module"),
        ("../escape.fn", json!([]), "invalid module name"),
        ("noDot", json!([]), "expected module.export"),
        (
            "probe.echo",
            json!({"not": "an array"}),
            "args must be an array",
        ),
        ("probe.big", json!([]), "cannot encode bigint"),
        ("probe.circular", json!([]), "call stack"),
    ] {
        let error = one(&mut o, name, args).unwrap_err();
        assert!(error.contains(message), "{name}: {error}");
    }
    // A failed batch must not leave replies behind for the next one.
    let batch = [json!([]), json!([7])];
    assert!(o.results("probe.fails", &[json!([])]).is_ok());
    assert!(o.results("probe.missing", &batch).is_err());
    assert_eq!(one(&mut o, "probe.noisy", json!([7])).unwrap(), json!(7));
}

#[test]
fn empty_batch_needs_no_process() {
    let Some(mut o) = oracle() else { return };
    assert_eq!(o.results("probe.echo", &[]).unwrap(), Vec::<Value>::new());
}
