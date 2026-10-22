use serde::{
    Deserialize, Deserializer,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value, value::RawValue};
use std::fmt;

// Preserve serde_json's default nesting budget across borrowed-token parsers.
const MAX_JSON_DEPTH: usize = 128;

// Construct literal JSON objects without Value's raw_value magic-key coercion.
// Reject duplicates and retain a depth bound. Original config wire integers must
// fit i64/u64; canonical JCS numbers and HTTP components may instead be f64.
// Disabling token checks on canonical output is necessary for e.g. 1e20, whose
// legal floating wire spelling canonicalizes to an overflowing integer token.
pub(crate) struct JsonValue<const CHECK_INTEGER_TOKENS: bool>(pub(crate) Value);
impl<'de, const CHECK_INTEGER_TOKENS: bool> Deserialize<'de> for JsonValue<CHECK_INTEGER_TOKENS> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        JsonSeed::<CHECK_INTEGER_TOKENS> { depth: 0 }.deserialize(deserializer)
    }
}
struct JsonSeed<const CHECK_INTEGER_TOKENS: bool> {
    depth: usize,
}
impl<'de, const CHECK_INTEGER_TOKENS: bool> DeserializeSeed<'de>
    for JsonSeed<CHECK_INTEGER_TOKENS>
{
    type Value = JsonValue<CHECK_INTEGER_TOKENS>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        if self.depth >= MAX_JSON_DEPTH {
            return Err(de::Error::custom("JSON nesting exceeds supported limit"));
        }
        let raw = <&RawValue>::deserialize(deserializer)?;
        if CHECK_INTEGER_TOKENS && !has_supported_integer_token(raw.get()) {
            return Err(de::Error::custom("integer outside supported range"));
        }
        let mut parser = serde_json::Deserializer::from_str(raw.get());
        parser
            .deserialize_any(JsonVisitor::<CHECK_INTEGER_TOKENS> { depth: self.depth })
            .map_err(de::Error::custom)
    }
}
// RawValue borrows the original token; serde_json would otherwise turn an
// overflowing integer into f64 before the visitor can check its exact value.
fn has_supported_integer_token(raw: &str) -> bool {
    let is_number = raw
        .as_bytes()
        .first()
        .is_some_and(|first| first.is_ascii_digit() || *first == b'-');
    if !is_number || raw.contains(['.', 'e', 'E']) {
        return true;
    }
    if raw.starts_with('-') {
        raw.parse::<i64>().is_ok()
    } else {
        raw.parse::<u64>().is_ok()
    }
}
struct JsonVisitor<const CHECK_INTEGER_TOKENS: bool> {
    depth: usize,
}
impl<'de, const CHECK_INTEGER_TOKENS: bool> Visitor<'de> for JsonVisitor<CHECK_INTEGER_TOKENS> {
    type Value = JsonValue<CHECK_INTEGER_TOKENS>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unique-key JSON")
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(JsonValue(v.into()))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        Ok(JsonValue(v.into()))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(JsonValue(v.into()))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
        serde_json::Number::from_f64(v)
            .map(|n| JsonValue(Value::Number(n)))
            .ok_or_else(|| E::custom("invalid number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(JsonValue(v.into()))
    }
    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        Ok(JsonValue(Value::Null))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(JsonValue(value)) =
            sequence.next_element_seed(JsonSeed::<CHECK_INTEGER_TOKENS> {
                depth: self.depth + 1,
            })?
        {
            values.push(value);
        }
        Ok(JsonValue(Value::Array(values)))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<Self::Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = object.next_key::<String>()? {
            let JsonValue(value) = object.next_value_seed(JsonSeed::<CHECK_INTEGER_TOKENS> {
                depth: self.depth + 1,
            })?;
            if values.insert(key, value).is_some() {
                return Err(de::Error::custom("duplicate key"));
            }
        }
        Ok(JsonValue(Value::Object(values)))
    }
}
