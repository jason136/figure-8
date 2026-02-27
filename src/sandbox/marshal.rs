use std::collections::HashMap;
use std::fmt;

use thiserror::Error;

pub trait TsTyped {
    fn ts_type() -> TsType;
}

pub trait FromV8: Sized {
    fn from_v8(
        scope: &mut v8::PinScope<'_, '_>,
        value: v8::Local<v8::Value>,
    ) -> Result<Self, MarshalError>;
}

pub trait ToV8: TsTyped {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum TsType {
    Number,
    String,
    Boolean,
    Void,
    Unknown,
    Array(Box<TsType>),
    Optional(Box<TsType>),
    Promise(Box<TsType>),
    Record(Box<TsType>, Box<TsType>),
}

impl fmt::Display for TsType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TsType::Number => write!(f, "number"),
            TsType::String => write!(f, "string"),
            TsType::Boolean => write!(f, "boolean"),
            TsType::Void => write!(f, "void"),
            TsType::Unknown => write!(f, "unknown"),
            TsType::Array(inner) => write!(f, "{inner}[]"),
            TsType::Optional(inner) => write!(f, "{inner} | undefined"),
            TsType::Promise(inner) => write!(f, "Promise<{inner}>"),
            TsType::Record(k, v) => write!(f, "Record<{k}, {v}>"),
        }
    }
}

impl TsTyped for f64 {
    fn ts_type() -> TsType {
        TsType::Number
    }
}

impl TsTyped for i32 {
    fn ts_type() -> TsType {
        TsType::Number
    }
}

impl TsTyped for u32 {
    fn ts_type() -> TsType {
        TsType::Number
    }
}

impl TsTyped for String {
    fn ts_type() -> TsType {
        TsType::String
    }
}

impl TsTyped for bool {
    fn ts_type() -> TsType {
        TsType::Boolean
    }
}

impl TsTyped for () {
    fn ts_type() -> TsType {
        TsType::Void
    }
}

impl<T: TsTyped> TsTyped for Vec<T> {
    fn ts_type() -> TsType {
        TsType::Array(Box::new(T::ts_type()))
    }
}

impl<T: TsTyped> TsTyped for Option<T> {
    fn ts_type() -> TsType {
        TsType::Optional(Box::new(T::ts_type()))
    }
}

impl TsTyped for serde_json::Value {
    fn ts_type() -> TsType {
        TsType::Unknown
    }
}

impl<V: TsTyped> TsTyped for HashMap<String, V> {
    fn ts_type() -> TsType {
        TsType::Record(Box::new(TsType::String), Box::new(V::ts_type()))
    }
}

impl FromV8 for f64 {
    fn from_v8(
        scope: &mut v8::PinScope<'_, '_>,
        value: v8::Local<v8::Value>,
    ) -> Result<Self, MarshalError> {
        value.number_value(scope).ok_or(MarshalError::TypeMismatch {
            expected: "number",
            got: value.to_rust_string_lossy(scope),
        })
    }
}

impl FromV8 for i32 {
    fn from_v8(
        scope: &mut v8::PinScope<'_, '_>,
        value: v8::Local<v8::Value>,
    ) -> Result<Self, MarshalError> {
        value.int32_value(scope).ok_or(MarshalError::TypeMismatch {
            expected: "number (i32)",
            got: value.to_rust_string_lossy(scope),
        })
    }
}

impl FromV8 for u32 {
    fn from_v8(
        scope: &mut v8::PinScope<'_, '_>,
        value: v8::Local<v8::Value>,
    ) -> Result<Self, MarshalError> {
        value.uint32_value(scope).ok_or(MarshalError::TypeMismatch {
            expected: "number (u32)",
            got: value.to_rust_string_lossy(scope),
        })
    }
}

impl FromV8 for String {
    fn from_v8(
        scope: &mut v8::PinScope<'_, '_>,
        value: v8::Local<v8::Value>,
    ) -> Result<Self, MarshalError> {
        if !value.is_string() {
            return Err(MarshalError::TypeMismatch {
                expected: "string",
                got: value.to_rust_string_lossy(scope),
            });
        }
        Ok(value.to_rust_string_lossy(scope))
    }
}

impl FromV8 for bool {
    fn from_v8(
        scope: &mut v8::PinScope<'_, '_>,
        value: v8::Local<v8::Value>,
    ) -> Result<Self, MarshalError> {
        if !value.is_boolean() {
            return Err(MarshalError::TypeMismatch {
                expected: "boolean",
                got: value.to_rust_string_lossy(scope),
            });
        }
        Ok(value.boolean_value(scope))
    }
}

impl<T: FromV8> FromV8 for Vec<T> {
    fn from_v8(
        scope: &mut v8::PinScope<'_, '_>,
        value: v8::Local<v8::Value>,
    ) -> Result<Self, MarshalError> {
        let array: v8::Local<v8::Array> =
            value.try_into().map_err(|_| MarshalError::TypeMismatch {
                expected: "Array",
                got: value.to_rust_string_lossy(scope),
            })?;

        (0..array.length())
            .map(|i| {
                T::from_v8(
                    scope,
                    array.get_index(scope, i).ok_or_else(|| {
                        MarshalError::ConversionFailed(format!(
                            "failed to get array element at index {i}"
                        ))
                    })?,
                )
            })
            .collect::<Result<Vec<_>, _>>()
    }
}

impl<T: FromV8> FromV8 for Option<T> {
    fn from_v8(
        scope: &mut v8::PinScope<'_, '_>,
        value: v8::Local<v8::Value>,
    ) -> Result<Self, MarshalError> {
        if value.is_undefined() || value.is_null() {
            Ok(None)
        } else {
            T::from_v8(scope, value).map(Some)
        }
    }
}

impl FromV8 for serde_json::Value {
    fn from_v8(
        scope: &mut v8::PinScope<'_, '_>,
        value: v8::Local<v8::Value>,
    ) -> Result<Self, MarshalError> {
        match value {
            null if value.is_undefined() || value.is_null() => Ok(serde_json::Value::Null),
            bool if value.is_boolean() => Ok(serde_json::Value::Bool(value.boolean_value(scope))),
            number if value.is_number() => {
                Ok(serde_json::json!(value.number_value(scope).unwrap()))
            }
            string if value.is_string() => {
                Ok(serde_json::Value::String(value.to_rust_string_lossy(scope)))
            }
            array if value.is_array() => {
                let arr: v8::Local<v8::Array> = value.try_into().unwrap();

                Ok(serde_json::Value::Array(
                    (0..arr.length())
                        .map(|i| {
                            serde_json::Value::from_v8(scope, arr.get_index(scope, i).unwrap())
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            }
            object if value.is_object() => {
                let obj: v8::Local<v8::Object> = value.try_into().unwrap();

                let names = obj
                    .get_own_property_names(scope, v8::GetPropertyNamesArgs::default())
                    .ok_or_else(|| {
                        MarshalError::ConversionFailed("failed to get property names".into())
                    })?;

                Ok(serde_json::Value::Object(
                    (0..names.length())
                        .map(|i| {
                            let key = names.get_index(scope, i).unwrap();
                            let key_str = key.to_rust_string_lossy(scope);

                            Ok((
                                key_str,
                                serde_json::Value::from_v8(scope, obj.get(scope, key).unwrap())?,
                            ))
                        })
                        .collect::<Result<_, _>>()?,
                ))
            }
            _ => Err(MarshalError::ConversionFailed("unsupported V8 type".into())),
        }
    }
}

impl ToV8 for f64 {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        v8::Number::new(scope, *self).into()
    }
}

impl ToV8 for i32 {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        v8::Integer::new(scope, *self).into()
    }
}

impl ToV8 for u32 {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        v8::Integer::new_from_unsigned(scope, *self).into()
    }
}

impl ToV8 for String {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        v8::String::new(scope, self).unwrap().into()
    }
}

impl ToV8 for bool {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        v8::Boolean::new(scope, *self).into()
    }
}

impl ToV8 for () {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        v8::undefined(scope).into()
    }
}

impl<T: ToV8> ToV8 for Vec<T> {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        let array = v8::Array::new(scope, self.len() as i32);
        for (i, item) in self.iter().enumerate() {
            let val = item.to_v8(scope);
            array.set_index(scope, i as u32, val);
        }

        array.into()
    }
}

impl<T: ToV8> ToV8 for Option<T> {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        match self {
            Some(val) => val.to_v8(scope),
            None => v8::undefined(scope).into(),
        }
    }
}

impl ToV8 for serde_json::Value {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        match self {
            serde_json::Value::Null => v8::null(scope).into(),
            serde_json::Value::Bool(b) => v8::Boolean::new(scope, *b).into(),
            serde_json::Value::Number(n) => {
                v8::Number::new(scope, n.as_f64().unwrap_or(0.0)).into()
            }
            serde_json::Value::String(s) => v8::String::new(scope, s).unwrap().into(),
            serde_json::Value::Array(arr) => {
                let v8_arr = v8::Array::new(scope, arr.len() as i32);
                for (i, item) in arr.iter().enumerate() {
                    let val = item.to_v8(scope);
                    v8_arr.set_index(scope, i as u32, val);
                }

                v8_arr.into()
            }
            serde_json::Value::Object(map) => {
                let obj = v8::Object::new(scope);
                for (key, val) in map {
                    let k = v8::String::new(scope, key).unwrap();
                    let v = val.to_v8(scope);
                    obj.set(scope, k.into(), v);
                }

                obj.into()
            }
        }
    }
}

impl<V: ToV8> ToV8 for HashMap<String, V> {
    fn to_v8<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Value> {
        let obj = v8::Object::new(scope);
        for (key, value) in self {
            let k = v8::String::new(scope, key).unwrap();
            let v = value.to_v8(scope);
            obj.set(scope, k.into(), v);
        }

        obj.into()
    }
}

#[derive(Debug, Error)]
pub enum MarshalError {
    #[error("expected {expected}, got {got}")]
    TypeMismatch { expected: &'static str, got: String },

    #[error("conversion failed: {0}")]
    ConversionFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ts_type_display() {
        assert_eq!(TsType::Number.to_string(), "number");
        assert_eq!(TsType::String.to_string(), "string");
        assert_eq!(TsType::Boolean.to_string(), "boolean");
        assert_eq!(TsType::Void.to_string(), "void");
        assert_eq!(
            TsType::Array(Box::new(TsType::Number)).to_string(),
            "number[]"
        );
        assert_eq!(
            TsType::Optional(Box::new(TsType::String)).to_string(),
            "string | undefined"
        );
        assert_eq!(
            TsType::Promise(Box::new(TsType::String)).to_string(),
            "Promise<string>"
        );
        assert_eq!(
            TsType::Record(Box::new(TsType::String), Box::new(TsType::Unknown)).to_string(),
            "Record<string, unknown>"
        );
    }

    #[test]
    fn ts_type_from_rust_types() {
        assert_eq!(f64::ts_type(), TsType::Number);
        assert_eq!(String::ts_type(), TsType::String);
        assert_eq!(bool::ts_type(), TsType::Boolean);
        assert_eq!(<()>::ts_type(), TsType::Void);
        assert_eq!(
            <Vec<f64>>::ts_type(),
            TsType::Array(Box::new(TsType::Number))
        );
        assert_eq!(
            <Option<String>>::ts_type(),
            TsType::Optional(Box::new(TsType::String))
        );
    }
}
