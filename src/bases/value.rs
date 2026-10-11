use chrono::{DateTime, Local};
use indexmap::IndexMap;
use std::cmp::Ordering;

#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    List(Vec<Value>),
    Object(IndexMap<String, Value>),
    Date(DateTime<Local>),
    DateOnly(DateTime<Local>),
    Duration {
        months: f64,
        days: f64,
        milliseconds: f64,
    },
    File(usize),
    This(usize),
    Formulas,
    Link {
        path: String,
        display: Option<Box<Value>>,
        resolved: Option<usize>,
    },
    Image(String),
    Icon(String),
    Html(String),
    Regex {
        pattern: String,
        flags: String,
    },
}
impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(v) => *v,
            Self::Number(v) => *v != 0.0 && !v.is_nan(),
            Self::String(v) => !v.is_empty(),
            _ => true,
        }
    }
    pub fn empty(&self) -> bool {
        match self {
            Self::Null => true,
            Self::String(v) => v.is_empty(),
            Self::List(v) => v.is_empty(),
            Self::Object(v) => v.is_empty(),
            _ => false,
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "boolean",
            Self::Number(_) => "number",
            Self::String(_) => "string",
            Self::List(_) => "list",
            Self::Object(_) => "object",
            Self::Date(_) | Self::DateOnly(_) => "date",
            Self::Duration { .. } => "duration",
            Self::File(_) | Self::This(_) => "file",
            Self::Link { .. } => "link",
            Self::Image(_) => "image",
            Self::Icon(_) => "icon",
            Self::Html(_) => "html",
            Self::Regex { .. } => "regexp",
            Self::Formulas => "object",
        }
    }
    pub fn number(&self) -> anyhow::Result<f64> {
        if let Self::Number(v) = self {
            Ok(*v)
        } else {
            anyhow::bail!("expected number, got {}", self.type_name())
        }
    }
    pub fn string(&self) -> anyhow::Result<&str> {
        if let Self::String(v) = self {
            Ok(v)
        } else {
            anyhow::bail!("expected string, got {}", self.type_name())
        }
    }
    pub fn text(&self) -> String {
        match self {
            Self::Null => "null".into(),
            Self::Bool(v) => v.to_string(),
            Self::Number(v) => ryu_js::Buffer::new().format(*v).to_string(),
            Self::String(v) => v.clone(),
            Self::List(v) => v.iter().map(Value::text).collect::<Vec<_>>().join(", "),
            Self::Object(v) => format!(
                "{{{}}}",
                v.iter()
                    .map(|(k, v)| format!("{k}: {}", v.text()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Date(v) => v.format("%Y-%m-%dT%H:%M:%S").to_string(),
            Self::DateOnly(v) => v.format("%Y-%m-%d").to_string(),
            Self::Duration {
                months,
                days,
                milliseconds,
            } => super::dates::humanize(*months, *days * 86400000.0 + *milliseconds),
            Self::File(i) | Self::This(i) => format!("file[{i}]"),
            Self::Formulas => "formulas".into(),
            Self::Link { path, display, .. } => {
                if let Some(display) = display {
                    format!("[[{path}|{}]]", display.text())
                } else {
                    format!("[[{path}]]")
                }
            }
            Self::Image(v) => format!("![]({v})"),
            Self::Icon(v) => v.clone(),
            Self::Html(v) => v.clone(),
            Self::Regex { pattern, flags } => format!("/{pattern}/{flags}"),
        }
    }
    pub fn equal(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Date(a) | Self::DateOnly(a), Self::Date(b) | Self::DateOnly(b)) => a == b,
            (Self::File(a) | Self::This(a), Self::File(b) | Self::This(b)) => a == b,
            (
                Self::Link {
                    resolved: Some(a), ..
                },
                Self::File(b) | Self::This(b),
            )
            | (
                Self::File(b) | Self::This(b),
                Self::Link {
                    resolved: Some(a), ..
                },
            ) => a == b,
            (
                Self::Link {
                    path: a,
                    resolved: ra,
                    ..
                },
                Self::Link {
                    path: b,
                    resolved: rb,
                    ..
                },
            ) => match (ra, rb) {
                (Some(a), Some(b)) => a == b,
                _ => a == b,
            },
            (Self::List(a), Self::List(b)) => {
                a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.equal(b))
            }
            (Self::Object(a), Self::Object(b)) => {
                a.len() == b.len() && a.iter().all(|(k, v)| b.get(k).is_some_and(|b| v.equal(b)))
            }
            (
                Self::Duration {
                    months: a,
                    days: b,
                    milliseconds: c,
                },
                Self::Duration {
                    months: d,
                    days: e,
                    milliseconds: f,
                },
            ) => a == d && b == e && c == f,
            (Self::Image(a), Self::Image(b))
            | (Self::Icon(a), Self::Icon(b))
            | (Self::Html(a), Self::Html(b)) => a == b,
            _ => false,
        }
    }
    pub fn compare(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::Null, _) => Ordering::Less,
            (_, Self::Null) => Ordering::Greater,
            (Self::Number(a), Self::Number(b)) => a.total_cmp(b),
            (Self::Date(a) | Self::DateOnly(a), Self::Date(b) | Self::DateOnly(b)) => a.cmp(b),
            (Self::Bool(a), Self::Bool(b)) => a.cmp(b),
            _ => self.text().cmp(&other.text()),
        }
    }
}

pub fn escape_cell(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '|' => out.push_str("&#124;"),
            '\n' => out.push_str("<br>"),
            '\r' => out.push_str("&#13;"),
            '\\' | '`' | '*' | '_' | '[' | ']' => {
                out.push('\\');
                out.push(ch);
            }
            _ => out.push(ch),
        }
    }
    out
}
