//! Serde helpers that keep TypeScript's JSON shapes exact.
//!
//! | TypeScript          | Rust                         | Attributes                         |
//! |---------------------|------------------------------|------------------------------------|
//! | `x: T`              | `T`                          |                                    |
//! | `x?: T`             | `Option<T>`                  | `default, skip…, deserialize_with = "present"` |
//! | `x: T \| null`      | `Option<T>`                  | `deserialize_with = "required"`    |
//! | `x?: T \| null`     | `Option<Option<T>>`          | `default, skip…, with = "nullable"` |
//! | `number`            | `f64`                        |                                    |
//! | unknown keys        | [`Extra`]                    | `flatten`                          |

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Keys this build does not know, kept so a load and save never drops them.
///
/// `serde_json::Value` is used on purpose: the keys come from older or newer
/// versions, so their shape is unknown by definition.
pub type Extra = serde_json::Map<String, serde_json::Value>;

/// For `x: T | null`: the key must be present, and `null` reads as `None`.
/// Without this an absent key would also read as `None` and be written back
/// as `null`, which changes the file.
pub fn required<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// For `x?: T`: absent is `None`, and `null` is an error rather than `None`.
///
/// Plain `Option` reads `null` as `None` and then writes nothing, which would
/// drop the key. `null` is not a value of `x?: T`, so it fails the parse, and
/// the enclosing [`crate::settings::Stored`] keeps the JSON as found.
pub fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// For `x?: T | null`: absent is `None`, `null` is `Some(None)`.
pub mod nullable {
    use super::*;

    pub fn serialize<S, T>(value: &Option<Option<T>>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        match value {
            Some(inner) => inner.serialize(serializer),
            // Only reached without `skip_serializing_if`; absent has no JSON form.
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Option::<T>::deserialize(deserializer).map(Some)
    }
}

/// The literal type `true`, as in `isFinal: true`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct True;

impl Serialize for True {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for True {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if bool::deserialize(deserializer)? {
            Ok(True)
        } else {
            Err(serde::de::Error::custom("expected the literal `true`"))
        }
    }
}

/// A string-literal union: `enum` with each variant renamed to its JSON text.
macro_rules! string_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($(#[$vmeta:meta])* $variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
        $vis enum $name { $($(#[$vmeta])* #[serde(rename = $text)] $variant),+ }

        impl $name {
            /// Every value, in declaration order.
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            /// The JSON text of this value.
            pub fn as_str(self) -> &'static str {
                match self { $($name::$variant => $text),+ }
            }

            /// The value whose JSON text is exactly `text`.
            pub fn from_name(text: &str) -> Option<Self> {
                match text { $($text => Some($name::$variant),)+ _ => None }
            }
        }

        impl $crate::normalize::StringEnum for $name {
            fn from_name(name: &str) -> Option<Self> {
                $name::from_name(name)
            }
            fn name(&self) -> &'static str {
                self.as_str()
            }
        }
    };
}

/// A number-literal union such as `0 | 1 | 2 | 3`. Written as a JSON integer;
/// read from any JSON number equal to one of the values.
macro_rules! number_enum {
    ($(#[$meta:meta])* $vis:vis enum $name:ident { $($variant:ident = $value:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        $vis enum $name { $($variant),+ }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn value(self) -> u32 {
                match self { $($name::$variant => $value),+ }
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_u32(self.value())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let number = f64::deserialize(deserializer)?;
                $(if number == f64::from($value) { return Ok($name::$variant); })+
                Err(serde::de::Error::custom(format!(
                    concat!("{} is not a valid ", stringify!($name)), number
                )))
            }
        }
    };
}

pub(crate) use {number_enum, string_enum};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Shapes {
        #[serde(deserialize_with = "required")]
        nullable: Option<u8>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "present"
        )]
        optional: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
        both: Option<Option<u8>>,
        #[serde(flatten)]
        extra: Extra,
    }

    #[test]
    fn shapes_round_trip() {
        for value in [
            json!({"nullable": null}),
            json!({"nullable": 1, "optional": 2, "both": null}),
            json!({"nullable": 1, "both": 3, "future": {"a": [1]}}),
        ] {
            let parsed: Shapes = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
        }
        assert!(serde_json::from_value::<Shapes>(json!({})).is_err());
        // `null` is not a value of an optional field, so it cannot be dropped.
        assert!(
            serde_json::from_value::<Shapes>(json!({"nullable": 1, "optional": null})).is_err()
        );
    }

    #[test]
    fn literal_true() {
        assert!(serde_json::from_value::<True>(json!(true)).is_ok());
        assert!(serde_json::from_value::<True>(json!(false)).is_err());
    }

    number_enum!(
        enum Delay {
            Zero = 0,
            Three = 3,
        }
    );
    string_enum!(
        enum Side {
            Left = "left",
            Right = "right",
        }
    );

    #[test]
    fn literal_unions() {
        assert_eq!(
            serde_json::from_value::<Delay>(json!(3.0)).unwrap(),
            Delay::Three
        );
        assert!(serde_json::from_value::<Delay>(json!(2)).is_err());
        assert_eq!(serde_json::to_value(Delay::Zero).unwrap(), json!(0));
        assert_eq!(serde_json::to_value(Side::Right).unwrap(), json!("right"));
        assert_eq!(Side::Left.as_str(), "left");
        assert_eq!((Delay::ALL.len(), Side::ALL.len()), (2, 2));
        assert!(serde_json::from_value::<Side>(json!("up")).is_err());
    }
}
