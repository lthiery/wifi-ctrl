use std::collections::HashMap;
use std::fmt::Display;

use serde::de::value::MapDeserializer;
use serde::de::{self, Error, IntoDeserializer, Visitor};
use serde::{forward_to_deserialize_any, Deserialize};

type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Debug, thiserror::Error, PartialEq, Eq, Clone)]
pub enum ConfigError {
    #[error("Missing '=' delimiter in config line")]
    MissingDelimterEqual,
    #[error("escape code is not made up of valid hex code")]
    InvalidEscape,
    #[error("escape code is incomplete")]
    IncompleteEscape,
    #[error("escaped value is not valid uft8 after unescaping")]
    NonUtf8Escape,
    #[error("Value could not be decoded")]
    SerdeError(String),
}

impl Error for ConfigError {
    fn custom<T>(msg: T) -> Self
    where
        T: Display,
    {
        Self::SerdeError(msg.to_string())
    }
}

#[derive(Default)]
pub struct Deserializer<'de> {
    input: Vec<&'de str>,
}

impl<'de> Deserializer<'de> {
    fn only(&self) -> Result<&'de str> {
        if self.input.len() == 1 {
            Ok(self.input[0])
        } else {
            Err(ConfigError::SerdeError("did not expect seq".to_owned()))
        }
    }
}

impl<'de> IntoDeserializer<'de, ConfigError> for Deserializer<'de> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self::Deserializer {
        self
    }
}

pub fn from_str<'a, T>(s: &'a str) -> Result<T>
where
    T: Deserialize<'a>,
{
    let mut map: HashMap<&str, Deserializer<'_>> = HashMap::new();
    for line in s.trim().lines() {
        let (k, v) = line
            .split_once('=')
            .ok_or(ConfigError::MissingDelimterEqual)?;
        let (k, i) = if let Some((k, i)) = k.split_once('[') {
            if let Some((i, "")) = i.rsplit_once(']') {
                (k, i.parse().map_err(ConfigError::custom)?)
            } else {
                return Err(ConfigError::custom("invalid key"));
            }
        } else {
            (k, 0)
        };
        let values = &mut map.entry(k.trim()).or_default().input;
        if values.len() != i {
            return Err(ConfigError::custom("Duplicate key"));
        }
        values.push(v);
    }
    T::deserialize(MapDeserializer::new(map.into_iter()))
}

macro_rules! forward_to_from_str {
    ($func:ident $method:ident) => {
        #[inline]
        fn $func<V>(self, visitor: V) -> Result<V::Value>
        where
            V: Visitor<'de>,
        {
            visitor.$method(self.only()?.parse().map_err(ConfigError::custom)?)
        }
    };
}

impl<'de> de::Deserializer<'de> for Deserializer<'de> {
    type Error = ConfigError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        self.deserialize_string(visitor)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        match self.only()? {
            "true" | "TRUE" | "enabled" | "ENABLED" => visitor.visit_bool(true),
            "false" | "FALSE" | "disabled" | "DISABLED" => visitor.visit_bool(false),
            s => Err(ConfigError::SerdeError(format!("Invalid bool {}", s))),
        }
    }

    forward_to_from_str!(deserialize_i8  visit_i8);
    forward_to_from_str!(deserialize_i16 visit_i16);
    forward_to_from_str!(deserialize_i32 visit_i32);
    forward_to_from_str!(deserialize_i64 visit_i64);

    forward_to_from_str!(deserialize_u8  visit_u8);
    forward_to_from_str!(deserialize_u16 visit_u16);
    forward_to_from_str!(deserialize_u32 visit_u32);
    forward_to_from_str!(deserialize_u64 visit_u64);

    forward_to_from_str!(deserialize_f32 visit_f32);
    forward_to_from_str!(deserialize_f64 visit_f64);

    forward_to_from_str!(deserialize_char visit_char);

    // these are not really supported (nor used) as deserialize_any will always deserialize to a String
    forward_to_deserialize_any! {str unit unit_struct bytes byte_buf map struct newtype_struct enum tuple tuple_struct identifier ignored_any}

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_string(unprintf(self.only()?)?)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        if self.only()?.is_empty() {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(
            self.input
                .into_iter()
                .map(|s| Deserializer { input: vec![s] })
                .collect::<Vec<_>>()
                .into_deserializer(),
        )
    }
}

pub(crate) fn unprintf(escaped: &str) -> std::result::Result<String, ConfigError> {
    let mut bytes = escaped.as_bytes().iter().copied();
    let mut unescaped = vec![];
    // undo "printf_encode"
    loop {
        unescaped.push(match bytes.next() {
            Some(b'\\') => match bytes.next().ok_or(ConfigError::IncompleteEscape)? {
                b'n' => b'\n',
                b'r' => b'\r',
                b't' => b'\t',
                b'e' => b'\x1b',
                b'x' => {
                    let hex = [
                        bytes.next().ok_or(ConfigError::IncompleteEscape)?,
                        bytes.next().ok_or(ConfigError::IncompleteEscape)?,
                    ];
                    u8::from_str_radix(
                        std::str::from_utf8(&hex).or(Err(ConfigError::InvalidEscape))?,
                        16,
                    )
                    .or(Err(ConfigError::InvalidEscape))?
                }
                c => c,
            },
            Some(c) => c,
            None => break,
        })
    }
    String::from_utf8(unescaped).or(Err(ConfigError::NonUtf8Escape))
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn test_deserializer() {
        let resp = r#"
        state=ENABLED
        shrug=¯\\_(\xe3\x83\x84)_/¯
        "#;
        let status: HashMap<String, String> = from_str(resp).unwrap();
        assert_eq!(status.get("state").unwrap(), "ENABLED");
        assert_eq!(status.get("shrug").unwrap(), r#"¯\_(ツ)_/¯"#);
    }
}
