//! JSON as JavaScript writes it.
//!
//! Every TypeScript `number` is an `f64` here, and serde writes `1.0_f64` as
//! `1.0` where `JSON.stringify` writes `1`. These helpers write whole numbers
//! the JavaScript way, so a file the Electron app wrote comes back with the
//! same number text. Whole numbers from 2^63 up to 1e21 keep serde's
//! exponent form where JavaScript writes every digit; no stored value comes
//! near that. From 1e21 both write `1e+21` style.

use serde::Serialize;
use serde_json::{Number, Value};

/// 2^63: every whole `f64` below it converts to `i64` exactly.
const I64_LIMIT: f64 = 9_223_372_036_854_775_808.0;

/// `value` as a JSON tree, with whole floats written as integers and `-0` as `0`.
pub fn to_js_value<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<Value> {
    let mut tree = serde_json::to_value(value)?;
    integral_floats_to_integers(&mut tree);
    Ok(tree)
}

/// `value` as JSON text, numbers as [`to_js_value`] writes them.
pub fn to_js_string<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    serde_json::to_string(&to_js_value(value)?)
}

fn integral_floats_to_integers(value: &mut Value) {
    match value {
        Value::Number(number) => {
            if let Some(float) = number.as_f64().filter(|_| number.is_f64())
                && float.fract() == 0.0
                && float.abs() < I64_LIMIT
            {
                // Whole and below 2^63, so exact; `-0.0 as i64` is 0.
                *number = Number::from(float as i64);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(integral_floats_to_integers),
        Value::Object(map) => map.values_mut().for_each(integral_floats_to_integers),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_are_written_like_json_stringify() {
        let value = json!([1.0, -0.0, 0.5, -3.0, 9_007_199_254_740_993.0, 1e300, 7]);
        assert_eq!(
            to_js_string(&value).unwrap(),
            "[1,0,0.5,-3,9007199254740992,1e+300,7]"
        );
    }
}
