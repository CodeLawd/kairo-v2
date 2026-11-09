//! JSON text exactly as electron-store writes it: `JSON.stringify(value,
//! undefined, '\t')`. Numbers print as JavaScript prints them (`1e+21`,
//! `0.000001`), object keys keep their order, and strings escape as
//! `JSON.stringify` escapes them, so a file written here is byte-identical to
//! one Electron writes from the same value.

use kairo_domain::js::number_to_string;
use serde_json::{Map, Value};

/// `JSON.stringify(value, undefined, '\t')`.
pub fn stringify_tabs(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out
}

/// [`stringify_tabs`] of an object, without wrapping it in a [`Value`].
pub fn stringify_tabs_object(map: &Map<String, Value>) -> String {
    let mut out = String::new();
    write_object(&mut out, map, 0);
    out
}

fn indent(out: &mut String, depth: usize) {
    out.push('\n');
    out.extend(std::iter::repeat_n('\t', depth));
}

fn write_value(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => match (n.as_i64(), n.as_u64()) {
            (Some(i), _) => out.push_str(&js_int(i128::from(i))),
            (_, Some(u)) => out.push_str(&js_int(i128::from(u))),
            _ => {
                let f = n.as_f64().unwrap_or(f64::NAN);
                // JSON.stringify writes non-finite numbers as null; JSON cannot hold them anyway.
                out.push_str(&if f.is_finite() {
                    number_to_string(f)
                } else {
                    "null".into()
                });
            }
        },
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                indent(out, depth + 1);
                write_value(out, item, depth + 1);
            }
            indent(out, depth);
            out.push(']');
        }
        Value::Object(map) => write_object(out, map, depth),
    }
}

fn write_object(out: &mut String, map: &Map<String, Value>, depth: usize) {
    if map.is_empty() {
        out.push_str("{}");
        return;
    }
    out.push('{');
    for (i, (key, item)) in map.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        indent(out, depth + 1);
        write_string(out, key);
        out.push_str(": ");
        write_value(out, item, depth + 1);
    }
    indent(out, depth);
    out.push('}');
}

/// An integer as JavaScript prints the nearest double.
fn js_int(i: i128) -> String {
    let f = i as f64;
    if f.abs() < 1e21 && (f as i128) == i {
        i.to_string()
    } else {
        number_to_string(f)
    }
}

/// `JSON.stringify` and serde_json escape strings the same way.
fn write_string(out: &mut String, s: &str) {
    out.push_str(&serde_json::to_string(s).expect("a string serializes"));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_json_stringify_with_tabs() {
        let value = json!({ "b": [1, 2.5, { "c": [] }], "a": {}, "s": "q\"\\\n\u{1}é\u{2028}", "n": null, "t": true });
        assert_eq!(
            stringify_tabs(&value),
            "{\n\t\"b\": [\n\t\t1,\n\t\t2.5,\n\t\t{\n\t\t\t\"c\": []\n\t\t}\n\t],\n\t\"a\": {},\n\t\"s\": \"q\\\"\\\\\\n\\u0001é\u{2028}\",\n\t\"n\": null,\n\t\"t\": true\n}"
        );
    }

    #[test]
    fn numbers_print_as_javascript() {
        for (value, text) in [
            (json!(1e21), "1e+21"),
            (json!(0.000001), "0.000001"),
            (json!(1e-7), "1e-7"),
            (json!(0.1), "0.1"),
            (json!(-0.0), "0"),
            (json!(1_700_000_000_000_i64), "1700000000000"),
            (json!(9_007_199_254_740_993_u64), "9007199254740992"),
            (json!(u64::MAX), "18446744073709552000"),
        ] {
            assert_eq!(stringify_tabs(&value), text, "{value}");
        }
    }
}
