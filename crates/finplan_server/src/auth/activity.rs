//! Collect explicitly allowlisted submitted keys without retaining values for logs.
//! Typed deserialization, including duplicate/unknown-field errors, stays with serde.

use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use std::fmt;

pub(crate) trait ActivityFields {
    const FIELDS: &'static [&'static str];
}

pub(crate) struct Submitted<T> {
    pub body: T,
    pub fields: Vec<&'static str>,
}

impl<'de, T: Deserialize<'de> + ActivityFields> Deserialize<'de> for Submitted<T> {
    fn deserialize<D: Deserializer<'de>>(inner: D) -> Result<Self, D::Error> {
        let mut fields = Vec::new();
        let body = T::deserialize(FieldsDeserializer {
            inner,
            fields: &mut fields,
            allowed: T::FIELDS,
        })?;
        Ok(Self { body, fields })
    }
}

struct FieldsDeserializer<'a, D> {
    inner: D,
    fields: &'a mut Vec<&'static str>,
    allowed: &'static [&'static str],
}
impl<'de, D: Deserializer<'de>> Deserializer<'de> for FieldsDeserializer<'_, D> {
    type Error = D::Error;
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_any(FieldsVisitor {
            inner: visitor,
            fields: self.fields,
            allowed: self.allowed,
        })
    }
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.inner.deserialize_struct(
            name,
            fields,
            FieldsVisitor {
                inner: visitor,
                fields: self.fields,
                allowed: self.allowed,
            },
        )
    }
    fn deserialize_map<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_map(FieldsVisitor {
            inner: visitor,
            fields: self.fields,
            allowed: self.allowed,
        })
    }
    // Every supported activity body is a struct. These methods preserve the
    // required Deserializer contract for serde's derived implementations.
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct enum
        identifier ignored_any
    }
}

struct FieldsVisitor<'a, V> {
    inner: V,
    fields: &'a mut Vec<&'static str>,
    allowed: &'static [&'static str],
}
impl<'de, V: Visitor<'de>> Visitor<'de> for FieldsVisitor<'_, V> {
    type Value = V::Value;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        self.inner.expecting(formatter)
    }
    fn visit_map<M: MapAccess<'de>>(self, inner: M) -> Result<V::Value, M::Error> {
        self.inner.visit_map(ObservedMap {
            inner,
            fields: self.fields,
            allowed: self.allowed,
        })
    }
    // Serde permits positional JSON arrays for some struct types. Preserve
    // that existing behavior without claiming named fields were submitted.
    fn visit_seq<S: SeqAccess<'de>>(self, seq: S) -> Result<V::Value, S::Error> {
        self.inner.visit_seq(seq)
    }
}

struct ObservedMap<'a, M> {
    inner: M,
    fields: &'a mut Vec<&'static str>,
    allowed: &'static [&'static str],
}
impl<'de, M: MapAccess<'de>> MapAccess<'de> for ObservedMap<'_, M> {
    type Error = M::Error;
    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, Self::Error> {
        self.inner.next_key_seed(KeySeed {
            inner: seed,
            fields: self.fields,
            allowed: self.allowed,
        })
    }
    fn next_value_seed<V: DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, Self::Error> {
        self.inner.next_value_seed(seed)
    }
    fn size_hint(&self) -> Option<usize> {
        self.inner.size_hint()
    }
}

// Observe the original key seed rather than first deserializing a String. This
// preserves serde_path_to_error's field path for deny_unknown_fields failures.
struct KeySeed<'a, K> {
    inner: K,
    fields: &'a mut Vec<&'static str>,
    allowed: &'static [&'static str],
}
impl<'de, K: DeserializeSeed<'de>> DeserializeSeed<'de> for KeySeed<'_, K> {
    type Value = K::Value;
    fn deserialize<D: Deserializer<'de>>(self, inner: D) -> Result<Self::Value, D::Error> {
        self.inner.deserialize(KeyDeserializer {
            inner,
            fields: self.fields,
            allowed: self.allowed,
        })
    }
}
struct KeyDeserializer<'a, D> {
    inner: D,
    fields: &'a mut Vec<&'static str>,
    allowed: &'static [&'static str],
}
impl<'de, D: Deserializer<'de>> Deserializer<'de> for KeyDeserializer<'_, D> {
    type Error = D::Error;
    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_any(KeyVisitor {
            inner: visitor,
            fields: self.fields,
            allowed: self.allowed,
        })
    }
    fn deserialize_identifier<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.inner.deserialize_identifier(KeyVisitor {
            inner: visitor,
            fields: self.fields,
            allowed: self.allowed,
        })
    }
    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char str string bytes byte_buf
        option unit unit_struct newtype_struct seq tuple tuple_struct enum map struct ignored_any
    }
}
struct KeyVisitor<'a, V> {
    inner: V,
    fields: &'a mut Vec<&'static str>,
    allowed: &'static [&'static str],
}
impl<V> KeyVisitor<'_, V> {
    fn record(&mut self, key: &str) {
        if let Some(&field) = self.allowed.iter().find(|&&field| field == key)
            && !self.fields.contains(&field)
        {
            self.fields.push(field);
        }
    }
}
impl<'de, V: Visitor<'de>> Visitor<'de> for KeyVisitor<'_, V> {
    type Value = V::Value;
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        self.inner.expecting(formatter)
    }
    fn visit_str<E: serde::de::Error>(mut self, value: &str) -> Result<V::Value, E> {
        self.record(value);
        self.inner.visit_str(value)
    }
    fn visit_borrowed_str<E: serde::de::Error>(mut self, value: &'de str) -> Result<V::Value, E> {
        self.record(value);
        self.inner.visit_borrowed_str(value)
    }
    fn visit_string<E: serde::de::Error>(mut self, value: String) -> Result<V::Value, E> {
        self.record(&value);
        self.inner.visit_string(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{accounts::UpdateAccount, assets::UpdateAsset};
    use crate::auth::routes::UpdateUserProfile;

    #[test]
    fn records_present_null_and_default_fields_but_never_unknown_keys_or_values() {
        let submitted: Submitted<UpdateAsset> = serde_json::from_str(
            r#"{"name":"private account","return_profile_id":null,"tracking_error":0,"secret_token":"do not retain","principal":42}"#,
        ).unwrap();
        assert_eq!(
            submitted.fields,
            ["name", "return_profile_id", "tracking_error"]
        );
        assert_eq!(submitted.body.return_profile_id, Some(None));
        let omitted: Submitted<UpdateAsset> = serde_json::from_str("{}").unwrap();
        assert!(omitted.fields.is_empty());
        assert!(omitted.body.return_profile_id.is_none());
        let flattened: Submitted<UpdateAccount> = serde_json::from_str(
            r#"{"name":"private","flavor":"Liability","principal":42,"interest_rate":0.04}"#,
        )
        .unwrap();
        assert_eq!(
            flattened.fields,
            ["name", "flavor", "principal", "interest_rate"]
        );
        assert!(flattened.body.flavor.is_some());
    }

    fn same_rejection<T: serde::de::DeserializeOwned + ActivityFields>(json: &str) {
        let original = axum::Json::<T>::from_bytes(json.as_bytes()).err().unwrap();
        let observed = axum::Json::<Submitted<T>>::from_bytes(json.as_bytes())
            .err()
            .unwrap();
        assert_eq!(original.status(), observed.status(), "{json}");
        assert_eq!(original.body_text(), observed.body_text(), "{json}");
    }

    #[test]
    fn preserves_json_data_syntax_unknown_and_duplicate_field_rejections() {
        for json in [
            r#"{"name":32}"#,
            r#"{"name":"a","name":"b"}"#,
            "true",
            "null",
            "{",
            r#"{"return_profile_id":"bad"}"#,
        ] {
            same_rejection::<UpdateAsset>(json);
        }
        same_rejection::<UpdateUserProfile>(r#"{"secret_token":"private"}"#);
        same_rejection::<UpdateAccount>(r#"{"name":32}"#);
    }
}
