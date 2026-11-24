//! The values that style expressions and feature properties hold.

use std::sync::Arc;

use crate::style::color::Rgba;

#[derive(Clone, Debug, PartialEq, Default)]
pub(crate) enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(Arc<str>),
    Color(Rgba),
    Array(Arc<[Value]>),
    Object(Arc<serde_json::Map<String, serde_json::Value>>),
}

impl Value {
    pub fn from_json(json: &serde_json::Value) -> Value {
        match json {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
            serde_json::Value::String(s) => Value::String(s.as_str().into()),
            serde_json::Value::Array(items) => {
                Value::Array(items.iter().map(Value::from_json).collect())
            }
            serde_json::Value::Object(map) => Value::Object(Arc::new(map.clone())),
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(n) => Some(*n),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    /// MapLibre's `to-boolean`.
    pub fn truthy(&self) -> bool {
        match self {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            _ => true,
        }
    }

    /// MapLibre's `to-string`.
    pub fn to_text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => format_number(*n),
            Value::String(s) => s.to_string(),
            Value::Color(c) => {
                let [r, g, b, a] = c.0;
                format!(
                    "rgba({},{},{},{})",
                    (r * 255.0).round(),
                    (g * 255.0).round(),
                    (b * 255.0).round(),
                    a
                )
            }
            Value::Array(items) => {
                let parts: Vec<String> = items.iter().map(Value::to_json_text).collect();
                format!("[{}]", parts.join(","))
            }
            Value::Object(map) => serde_json::Value::Object((**map).clone()).to_string(),
        }
    }

    fn to_json_text(&self) -> String {
        match self {
            Value::String(s) => serde_json::Value::String(s.to_string()).to_string(),
            other => other.to_text(),
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Color(_) => "color",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    }
}

fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}
