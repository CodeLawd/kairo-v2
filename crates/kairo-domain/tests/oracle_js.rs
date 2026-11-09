//! `kairo_domain::js` against the JavaScript built-ins it reproduces, through
//! `tools/ts-oracle/modules/js.ts`.

mod common;

use common::*;
use kairo_domain::js;
use kairo_oracle::{assert_all_match, number, undefined};
use proptest::prelude::*;
use serde_json::{Value, json};

/// Any double, weighted towards values whose printing or rounding is tricky.
fn float() -> BoxedStrategy<f64> {
    prop_oneof![
        3 => any::<f64>(),
        2 => -1e6..1e6f64,
        2 => (-1000i32..1000).prop_map(|n| f64::from(n) / 2.0),
        1 => (-30i32..30).prop_map(|e| 10f64.powi(e)),
        1 => (-30i32..30, 1u32..999).prop_map(|(e, m)| f64::from(m) * 10f64.powi(e)),
        1 => prop::sample::select(vec![
            0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.49999999999999994,
            -0.5, 1e21, 1e-7, 123e-20, 5e-324, f64::MAX, 9_007_199_254_740_993.0,
        ]),
    ]
    .boxed()
}

#[test]
fn math_helpers_match() {
    chunked(14_100, cases(2_000), (float(), float()), |inputs| {
        let mut round = Vec::new();
        let mut min = Vec::new();
        let mut max = Vec::new();
        let mut safe = Vec::new();
        let mut text = Vec::new();
        for (a, b) in &inputs {
            round.push((json!([number(*a)]), number(js::math_round(*a))));
            min.push((
                json!([number(*a), number(*b)]),
                number(js::math_min(*a, *b)),
            ));
            max.push((
                json!([number(*a), number(*b)]),
                number(js::math_max(*a, *b)),
            ));
            safe.push((json!([number(*a)]), json!(js::is_safe_integer(*a))));
            text.push((json!([number(*a)]), json!(js::number_to_string(*a))));
        }
        assert_all_match("oracle:js.mathRound", &round);
        assert_all_match("oracle:js.mathMin", &min);
        assert_all_match("oracle:js.mathMax", &max);
        assert_all_match("oracle:js.isSafeInteger", &safe);
        assert_all_match("oracle:js.numberToString", &text);
    });
}

#[test]
fn value_coercions_match() {
    chunked(14_200, cases(2_000), hostile(), |inputs| {
        let strings: Vec<_> = inputs
            .iter()
            .map(|v| (json!([v]), json!(js::value_to_string(v))))
            .collect();
        let truthy: Vec<_> = inputs
            .iter()
            .map(|v| (json!([v]), json!(js::truthy(Some(v)))))
            .collect();
        assert_all_match("oracle:js.valueToString", &strings);
        assert_all_match("oracle:js.truthy", &truthy);
    });
    assert_all_match(
        "oracle:js.truthy",
        &[(json!([undefined()]), json!(js::truthy(None)))],
    );
}

/// Strings built from the pieces of numeric syntax.
fn numeric_text() -> BoxedStrategy<String> {
    let piece = prop::sample::select(vec![
        "0", "1", "9", "12", ".", "e", "E", "+", "-", "x", "X", "0x", "0o", "0b", "f", "Z", "7",
        "Infinity", "NaN", " ", "\t", "\u{a0}", "\u{feff}", "\u{85}", "_", "1e400",
    ]);
    prop_oneof![
        4 => prop::collection::vec(piece, 0..6).prop_map(|parts| parts.concat()),
        1 => "[ -~]{0,8}",
    ]
    .boxed()
}

#[test]
fn string_to_number_and_parsers_match() {
    let radix = prop_oneof![
        3 => Just(None),
        2 => prop::sample::select(vec![0.0, 2.0, 8.0, 10.0, 16.0, 36.0, 37.0, 1.0, -1.0, 10.5, f64::NAN, 4_294_967_312.0])
            .prop_map(Some),
    ];
    chunked(14_300, cases(2_000), (numeric_text(), radix), |inputs| {
        let mut number_cases = Vec::new();
        let mut int_cases = Vec::new();
        let mut float_cases = Vec::new();
        let mut trim_cases = Vec::new();
        for (s, radix) in &inputs {
            number_cases.push((json!([s]), number(js::string_to_number(s))));
            let radix_arg = radix.map_or_else(undefined, number);
            int_cases.push((json!([s, radix_arg]), number(js::parse_int(s, *radix))));
            float_cases.push((json!([s]), number(js::parse_float(s))));
            trim_cases.push((json!([s]), json!(js::trim(s))));
        }
        assert_all_match("oracle:js.stringToNumber", &number_cases);
        assert_all_match("oracle:js.parseIntJs", &int_cases);
        assert_all_match("oracle:js.parseFloatJs", &float_cases);
        assert_all_match("oracle:js.trim", &trim_cases);
    });
}

#[test]
fn replace_all_matches() {
    // Basic Multilingual Plane only: an empty pattern splits astral characters
    // into lone surrogates, which JSON cannot carry back.
    let text = "[ab{}$&`'é ]{0,8}";
    let pattern = prop_oneof![
        Just(String::new()),
        "[ab{}]{1,3}".boxed(),
        Just("{Text}".to_owned())
    ];
    let replacement = "[ab$&`'0-9<]{0,6}";
    chunked(
        14_400,
        cases(2_000),
        (text, pattern, replacement),
        |inputs| {
            let cases: Vec<(Value, Value)> = inputs
                .iter()
                .map(|(s, p, r)| (json!([s, p, r]), json!(js::replace_all(s, p, r))))
                .collect();
            assert_all_match("oracle:js.replaceAll", &cases);
        },
    );
}
