use super::{data::Dataset, dates, expression::Expr, value::Value};
use anyhow::{Context, bail};
use chrono::{DateTime, Local};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
};

pub struct Eval<'a, 'q> {
    pub data: &'a Dataset<'q>,
    pub row: usize,
    pub context: usize,
    pub now: DateTime<Local>,
    pub formulas: &'a BTreeMap<String, Expr>,
    pub vars: BTreeMap<String, Value>,
    memo: HashMap<String, Value>,
    stack: Vec<String>,
    steps: usize,
    depth: usize,
}
impl<'a, 'q> Eval<'a, 'q> {
    pub fn new(
        data: &'a Dataset<'q>,
        row: usize,
        context: usize,
        now: DateTime<Local>,
        formulas: &'a BTreeMap<String, Expr>,
    ) -> Self {
        Self {
            data,
            row,
            context,
            now,
            formulas,
            vars: BTreeMap::new(),
            memo: HashMap::new(),
            stack: Vec::new(),
            steps: 0,
            depth: 0,
        }
    }
    pub fn formula(&mut self, name: &str) -> anyhow::Result<Value> {
        if let Some(value) = self.memo.get(name) {
            return Ok(value.clone());
        }
        if self.stack.iter().any(|key| key == name) {
            bail!(
                "circular formula dependency: {} -> {name}",
                self.stack.join(" -> ")
            );
        }
        let expr = self
            .formulas
            .get(name)
            .with_context(|| format!("unknown formula: {name}"))?
            .clone();
        self.stack.push(name.into());
        let outer = std::mem::take(&mut self.vars);
        let result = self.eval(&expr).with_context(|| {
            format!(
                "formula.{name} for {}",
                self.data.file(self.row).relative_path
            )
        });
        self.vars = outer;
        self.stack.pop();
        let value = result?;
        self.memo.insert(name.into(), value.clone());
        Ok(value)
    }
    pub fn property(&mut self, name: &str) -> anyhow::Result<Value> {
        if let Some(name) = name.strip_prefix("formula.") {
            self.formula(name)
        } else if let Some(name) = name.strip_prefix("file.") {
            self.member(Value::File(self.row), Value::String(name.into()))
        } else {
            Ok(self
                .data
                .props(self.row)?
                .get(name.strip_prefix("note.").unwrap_or(name))
                .cloned()
                .unwrap_or(Value::Null))
        }
    }
    pub fn eval(&mut self, expr: &Expr) -> anyhow::Result<Value> {
        self.depth += 1;
        if self.depth > 128 {
            self.depth -= 1;
            bail!("expression/formula evaluation depth exceeds 128");
        }
        let result = self.eval_inner(expr);
        self.depth -= 1;
        result
    }
    fn eval_inner(&mut self, expr: &Expr) -> anyhow::Result<Value> {
        self.steps += 1;
        if self.steps > 1_000_000 {
            bail!(
                "expression evaluation exceeds 1000000 steps for {}",
                self.data.file(self.row).relative_path
            );
        }
        match expr {
            Expr::Literal(v) => Ok(v.clone()),
            Expr::Ident(name) => Ok(match name.as_str() {
                "file" => Value::File(self.row),
                "note" => Value::Object(self.data.props(self.row)?),
                "this" => Value::This(self.context),
                "formula" => Value::Formulas,
                _ => {
                    if let Some(value) = self.vars.get(name) {
                        value.clone()
                    } else {
                        self.data
                            .props(self.row)?
                            .get(name)
                            .cloned()
                            .unwrap_or(Value::Null)
                    }
                }
            }),
            Expr::List(items) => Ok(Value::List(
                items
                    .iter()
                    .map(|item| self.eval(item))
                    .collect::<anyhow::Result<_>>()?,
            )),
            Expr::Object(items) => Ok(Value::Object(
                items
                    .iter()
                    .map(|(key, item)| Ok((key.clone(), self.eval(item)?)))
                    .collect::<anyhow::Result<_>>()?,
            )),
            Expr::Member(base, key) => {
                let base = self.eval(base)?;
                let key = self.eval(key)?;
                self.member(base, key)
            }
            Expr::Unary(op, expr) => {
                let value = self.eval(expr)?;
                Ok(match op.as_str() {
                    "!" => Value::Bool(!value.truthy()),
                    "-" => Value::Number(-coerce_number(&value)?),
                    "+" => Value::Number(coerce_number(&value)?),
                    _ => unreachable!(),
                })
            }
            Expr::Binary(op, left, right) => {
                let left = self.eval(left)?;
                if op == "&&" {
                    return if !left.truthy() {
                        Ok(left)
                    } else {
                        self.eval(right)
                    };
                }
                if op == "||" {
                    return if left.truthy() {
                        Ok(left)
                    } else {
                        self.eval(right)
                    };
                }
                let right = self.eval(right)?;
                self.binary(op, left, right)
            }
            Expr::Call(callee, args) => match callee.as_ref() {
                Expr::Ident(name) => {
                    if name == "if" {
                        let condition = self.eval(&args[0])?;
                        return if condition.truthy() {
                            self.eval(&args[1])
                        } else if args.len() == 3 {
                            self.eval(&args[2])
                        } else {
                            Ok(Value::Null)
                        };
                    }
                    let values = args
                        .iter()
                        .map(|arg| self.eval(arg))
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    self.global(name, values)
                }
                Expr::Member(base, key) => {
                    let base = self.eval(base)?;
                    let key = self.eval(key)?;
                    let name = key.string()?;
                    if matches!(base, Value::Null)
                        && !["isTruthy", "isEmpty", "isType", "toString"].contains(&name)
                    {
                        return Ok(Value::Null);
                    }
                    if matches!(name, "map" | "filter" | "reduce") {
                        let Value::List(items) = base else {
                            bail!("{name} requires a list");
                        };
                        let mut acc = if name == "reduce" {
                            self.eval(&args[1])?
                        } else {
                            Value::Null
                        };
                        let mut result = Vec::new();
                        let outer = self.vars.clone();
                        let evaluated = (|| -> anyhow::Result<()> {
                            for (index, value) in items.into_iter().enumerate() {
                                self.vars = outer.clone();
                                self.vars.insert("value".into(), value.clone());
                                self.vars
                                    .insert("index".into(), Value::Number(index as f64));
                                if name == "reduce" {
                                    self.vars.insert("acc".into(), acc.clone());
                                }
                                let transformed = self.eval(&args[0])?;
                                match name {
                                    "map" => result.push(transformed),
                                    "filter" => {
                                        if transformed.truthy() {
                                            result.push(value)
                                        }
                                    }
                                    "reduce" => acc = transformed,
                                    _ => unreachable!(),
                                }
                            }
                            Ok(())
                        })();
                        self.vars = outer;
                        evaluated?;
                        return Ok(if name == "reduce" {
                            acc
                        } else {
                            Value::List(result)
                        });
                    }
                    let values = args
                        .iter()
                        .map(|arg| self.eval(arg))
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    self.method(base, name, values)
                }
                _ => bail!("expression is not callable"),
            },
        }
    }
    fn member(&mut self, base: Value, key: Value) -> anyhow::Result<Value> {
        if let Value::List(items) = &base
            && let Value::Number(index) = key
        {
            if index.fract() != 0.0 || index < 0.0 {
                return Ok(Value::Null);
            }
            return Ok(items.get(index as usize).cloned().unwrap_or(Value::Null));
        }
        let key = key.string()?;
        match base {
            Value::Formulas => self.formula(key),
            Value::Object(properties) => Ok(properties.get(key).cloned().unwrap_or(Value::Null)),
            Value::This(index) => {
                if key == "file" {
                    Ok(Value::File(index))
                } else {
                    Ok(self
                        .data
                        .props(index)?
                        .get(key)
                        .cloned()
                        .unwrap_or(Value::Null))
                }
            }
            Value::File(index) => {
                let file = self.data.file(index);
                Ok(match key {
                    "file" => Value::File(index),
                    "name" => Value::String(file.path.file_name().unwrap_or("").into()),
                    "basename" => Value::String(file.path.file_stem().unwrap_or("").into()),
                    "path" => Value::String(file.relative_path),
                    "folder" => Value::String(
                        camino::Utf8Path::new(&file.relative_path)
                            .parent()
                            .map(|p| p.to_string())
                            .unwrap_or_default(),
                    ),
                    "ext" => Value::String(file.path.extension().unwrap_or("").into()),
                    "size" => Value::Number(file.size_bytes as f64),
                    "properties" => Value::Object(self.data.props(index)?),
                    "tags" => Value::List(self.data.tags(index)?),
                    "links" => Value::List(self.data.links(index, false)?),
                    "embeds" => Value::List(self.data.links(index, true)?),
                    "mtime" => self.data.date(index, false)?,
                    "ctime" => self.data.date(index, true)?,
                    "backlinks" => {
                        let mut links = Vec::new();
                        for other in 0..self.data.candidates {
                            if self.data.has_link(other, &Value::File(index))? {
                                let source = self.data.file(other);
                                links.push(self.data.link(
                                    &source.relative_path,
                                    Some(Value::String(
                                        source.path.file_stem().unwrap_or("").into(),
                                    )),
                                    other,
                                )?);
                            }
                        }
                        Value::List(links)
                    }
                    _ => bail!("unknown file field: {key}"),
                })
            }
            Value::Date(date) | Value::DateOnly(date) => dates::field(date, key),
            Value::String(text) if key == "length" => {
                Ok(Value::Number(text.encode_utf16().count() as f64))
            }
            Value::List(items) if key == "length" => Ok(Value::Number(items.len() as f64)),
            Value::Null => Ok(Value::Null),
            other => bail!("{} has no field {key:?}", other.type_name()),
        }
    }
    fn binary(&self, op: &str, left: Value, right: Value) -> anyhow::Result<Value> {
        if op == "==" {
            return Ok(Value::Bool(left.equal(&right)));
        }
        if op == "!=" {
            return Ok(Value::Bool(!left.equal(&right)));
        }
        if matches!(op, "<" | ">" | "<=" | ">=") {
            let order = match (&left, &right) {
                (Value::String(a), Value::String(b)) => a.encode_utf16().cmp(b.encode_utf16()),
                _ => {
                    let a = coerce_number(&left)?;
                    let b = coerce_number(&right)?;
                    let Some(order) = a.partial_cmp(&b) else {
                        return Ok(Value::Bool(false));
                    };
                    order
                }
            };
            return Ok(Value::Bool(match op {
                "<" => order == Ordering::Less,
                ">" => order == Ordering::Greater,
                "<=" => order != Ordering::Greater,
                _ => order != Ordering::Less,
            }));
        }
        let date_only = matches!(left, Value::DateOnly(_));
        if let Value::Date(date) | Value::DateOnly(date) = left {
            if let Value::Date(other) | Value::DateOnly(other) = right {
                if op == "-" {
                    return Ok(Value::Number(
                        date.signed_duration_since(other).num_milliseconds() as f64,
                    ));
                }
                bail!("two dates can only be subtracted");
            }
            let duration = if let Value::String(text) = right {
                dates::duration(&text)?
            } else {
                right
            };
            if let Value::Duration {
                months,
                days,
                milliseconds,
            } = duration
            {
                let sign = match op {
                    "+" => 1.0,
                    "-" => -1.0,
                    _ => bail!("date arithmetic requires + or -"),
                };
                let date = dates::add(date, months * sign, days * sign, milliseconds * sign)?;
                return Ok(if date_only && milliseconds == 0.0 {
                    Value::DateOnly(date)
                } else {
                    Value::Date(date)
                });
            }
            bail!("date arithmetic requires a duration or duration string");
        }
        if let Value::Duration {
            months,
            days,
            milliseconds,
        } = left
        {
            if let Value::Duration {
                months: other_months,
                days: other_days,
                milliseconds: other_ms,
            } = right
            {
                return match op {
                    "+" => Ok(Value::Duration {
                        months: months + other_months,
                        days: days + other_days,
                        milliseconds: milliseconds + other_ms,
                    }),
                    "-" => Ok(Value::Duration {
                        months: months - other_months,
                        days: days - other_days,
                        milliseconds: milliseconds - other_ms,
                    }),
                    _ => bail!("duration pairs require + or -"),
                };
            }
            let scalar = coerce_number(&right)?;
            return match op {
                "*" => Ok(Value::Duration {
                    months: months * scalar,
                    days: days * scalar,
                    milliseconds: milliseconds * scalar,
                }),
                "/" => Ok(Value::Duration {
                    months: months / scalar,
                    days: days / scalar,
                    milliseconds: milliseconds / scalar,
                }),
                _ => bail!("duration scalar arithmetic requires * or /"),
            };
        }
        if op == "+" && (matches!(left, Value::String(_)) || matches!(right, Value::String(_))) {
            return Ok(Value::String(format!(
                "{}{}",
                self.data.display(&left),
                self.data.display(&right)
            )));
        }
        let a = coerce_number(&left)?;
        let b = coerce_number(&right)?;
        Ok(Value::Number(match op {
            "+" => a + b,
            "-" => a - b,
            "*" => a * b,
            "/" => a / b,
            "%" => a % b,
            _ => unreachable!(),
        }))
    }
    fn path(&self, value: &Value) -> anyhow::Result<String> {
        match value {
            Value::String(s) => Ok(s.clone()),
            Value::File(i) | Value::This(i) => Ok(self.data.file(*i).relative_path),
            Value::Link { path, .. } => Ok(path.clone()),
            _ => bail!(
                "expected a file, link, or path string; got {}",
                value.type_name()
            ),
        }
    }
    fn global(&self, name: &str, args: Vec<Value>) -> anyhow::Result<Value> {
        Ok(match name {
            "escapeHTML" => Value::String(escape_html(args[0].string()?)),
            "date" => dates::value(args[0].string()?)?,
            "duration" => dates::duration(args[0].string()?)?,
            "file" => match &args[0] {
                Value::File(i) | Value::This(i) => Value::File(*i),
                Value::Link { resolved, .. } => resolved.map(Value::File).unwrap_or(Value::Null),
                value => self
                    .data
                    .resolve(&self.path(value)?, self.row)?
                    .map(Value::File)
                    .unwrap_or(Value::Null),
            },
            "html" => Value::Html(args[0].string()?.into()),
            "image" => Value::Image(self.path(&args[0])?),
            "icon" => Value::Icon(args[0].string()?.into()),
            "link" => self
                .data
                .link(&self.path(&args[0])?, args.get(1).cloned(), self.row)?,
            "list" => {
                if let Value::List(_) = args[0] {
                    args[0].clone()
                } else {
                    Value::List(vec![args[0].clone()])
                }
            }
            "number" => Value::Number(coerce_number(&args[0])?),
            "now" => Value::Date(self.now),
            "today" => Value::DateOnly(dates::local(
                self.now.date_naive().and_hms_opt(0, 0, 0).unwrap(),
            )?),
            "random" => Value::Number(fastrand::f64()),
            "min" | "max" => {
                let mut result = if name == "min" {
                    f64::INFINITY
                } else {
                    f64::NEG_INFINITY
                };
                for arg in &args {
                    let n = coerce_number(arg)?;
                    if n.is_nan() {
                        result = f64::NAN;
                        break;
                    }
                    result = if name == "min" {
                        result.min(n)
                    } else {
                        result.max(n)
                    };
                }
                Value::Number(result)
            }
            _ => bail!("unsupported function: {name}"),
        })
    }
    fn method(&self, base: Value, name: &str, args: Vec<Value>) -> anyhow::Result<Value> {
        if name == "isTruthy" {
            return Ok(Value::Bool(base.truthy()));
        }
        if name == "isEmpty" {
            return Ok(Value::Bool(base.empty()));
        }
        if name == "isType" {
            let kind = args[0].string()?;
            let kind = match kind {
                "array" => "list",
                "boolean" | "number" | "string" | "date" | "duration" | "file" | "link"
                | "image" | "icon" | "html" | "list" | "object" | "null" | "regexp" => kind,
                "regex" => "regexp",
                _ => bail!("unknown value type: {kind}"),
            };
            return Ok(Value::Bool(base.type_name() == kind));
        }
        if name == "toString" {
            return Ok(Value::String(self.data.display(&base)));
        }
        match base {
            Value::String(text) => self.string_method(text, name, &args),
            Value::Number(n) => Ok(match name {
                "abs" => Value::Number(n.abs()),
                "ceil" => Value::Number(n.ceil()),
                "floor" => Value::Number(n.floor()),
                "round" => {
                    let digits = if args.is_empty() {
                        0.0
                    } else {
                        args[0].number()?
                    };
                    if !digits.is_finite() || digits.abs() > 308.0 {
                        bail!("round digits out of range");
                    }
                    let factor = 10f64.powi(digits.trunc() as i32);
                    Value::Number((n * factor + 0.5).floor() / factor)
                }
                "toFixed" => {
                    let digits = args[0].number()?.trunc();
                    if !(0.0..=100.0).contains(&digits) {
                        bail!("toFixed precision must be 0..100");
                    }
                    Value::String(to_fixed(n, digits as usize))
                }
                _ => bail!("number has no method {name}"),
            }),
            Value::List(items) => self.list_method(items, name, &args),
            Value::Date(date) | Value::DateOnly(date) => Ok(match name {
                "date" => Value::DateOnly(dates::local(
                    date.date_naive().and_hms_opt(0, 0, 0).unwrap(),
                )?),
                "format" => Value::String(dates::format(date, args[0].string()?)?),
                "time" => Value::String(date.format("%H:%M:%S").to_string()),
                "relative" => Value::String(dates::relative(date, self.now)),
                _ => bail!("date has no method {name}"),
            }),
            Value::File(index) | Value::This(index) => {
                let file = self.data.file(index);
                Ok(match name {
                    "asLink" => {
                        self.data
                            .link(&file.relative_path, args.first().cloned(), index)?
                    }
                    "hasLink" => Value::Bool(self.data.has_link(index, &args[0])?),
                    "hasProperty" => {
                        Value::Bool(self.data.props(index)?.contains_key(args[0].string()?))
                    }
                    "hasTag" => {
                        let tags = self.data.tags(index)?;
                        let mut found = false;
                        for arg in args {
                            let tag = arg.string()?.trim_start_matches('#');
                            if tags.iter().any(|v| {
                                v.string().is_ok_and(|s| {
                                    let s = s.trim_start_matches('#');
                                    s == tag || s.starts_with(&format!("{tag}/"))
                                })
                            }) {
                                found = true;
                            }
                        }
                        Value::Bool(found)
                    }
                    "inFolder" => {
                        let folder = args[0].string()?.trim_matches('/');
                        let current = camino::Utf8Path::new(&file.relative_path)
                            .parent()
                            .map(|p| p.as_str())
                            .unwrap_or("");
                        Value::Bool(
                            folder.is_empty()
                                || current == folder
                                || current.starts_with(&format!("{folder}/")),
                        )
                    }
                    _ => bail!("file has no method {name}"),
                })
            }
            Value::Link { resolved, .. } => Ok(match name {
                "asFile" => resolved.map(Value::File).unwrap_or(Value::Null),
                "linksTo" => {
                    if let Some(index) = resolved {
                        Value::Bool(self.data.has_link(index, &args[0])?)
                    } else {
                        Value::Bool(false)
                    }
                }
                _ => bail!("link has no method {name}"),
            }),
            Value::Object(properties) => Ok(match name {
                "keys" => Value::List(properties.into_keys().map(Value::String).collect()),
                "values" => Value::List(properties.into_values().collect()),
                _ => bail!("object has no method {name}"),
            }),
            Value::Regex { pattern, flags } => {
                if name == "matches" {
                    let regex = regress::Regex::with_flags(&pattern, flags.as_str())
                        .map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;
                    let text = args[0].string()?;
                    Ok(Value::Bool(
                        regex
                            .find(text)
                            .is_some_and(|m| !flags.contains('y') || m.start() == 0),
                    ))
                } else {
                    bail!("regexp has no method {name}")
                }
            }
            other => bail!("{} has no method {name}", other.type_name()),
        }
    }
    fn string_method(&self, text: String, name: &str, args: &[Value]) -> anyhow::Result<Value> {
        Ok(match name {
            "contains" => Value::Bool(text.contains(args[0].string()?)),
            "containsAll" | "containsAny" => {
                let matches = args
                    .iter()
                    .map(|arg| Ok(text.contains(arg.string()?)))
                    .collect::<anyhow::Result<Vec<_>>>()?;
                Value::Bool(if name == "containsAll" {
                    matches.iter().all(|v| *v)
                } else {
                    matches.iter().any(|v| *v)
                })
            }
            "endsWith" => Value::Bool(text.ends_with(args[0].string()?)),
            "startsWith" => Value::Bool(text.starts_with(args[0].string()?)),
            "lower" => Value::String(text.to_lowercase()),
            "trim" => Value::String(text.trim().into()),
            "title" => {
                let mut start = true;
                Value::String(
                    text.chars()
                        .flat_map(|ch| {
                            let transformed = if !ch.is_alphabetic() {
                                ch.to_string()
                            } else if start {
                                ch.to_uppercase().collect::<String>()
                            } else {
                                ch.to_lowercase().collect::<String>()
                            };
                            start = !ch.is_alphabetic();
                            transformed.chars().collect::<Vec<_>>()
                        })
                        .collect(),
                )
            }
            "reverse" => Value::String(text.chars().rev().collect()),
            "repeat" => {
                let count = args[0].number()?.trunc();
                if !count.is_finite()
                    || count < 0.0
                    || count > 8_388_608.0
                    || text.len().saturating_mul(count as usize) > 8_388_608
                {
                    bail!("repeat result exceeds 8 MiB or count is invalid");
                }
                Value::String(text.repeat(count as usize))
            }
            "slice" => {
                let units: Vec<u16> = text.encode_utf16().collect();
                let (start, end) = slice_bounds(units.len(), args)?;
                Value::String(String::from_utf16(&units[start..end]).context("string.slice split a UTF-16 surrogate pair; result cannot be represented as Unicode text")?)
            }
            "replace" => {
                let replacement = args[1].string()?;
                Value::String(match &args[0] {
                    Value::String(pattern) => {
                        let mut out = String::new();
                        let mut previous = 0;
                        for (start, found) in text.match_indices(pattern) {
                            out.push_str(&text[previous..start]);
                            out.push_str(&expand_replacement(
                                replacement,
                                &text,
                                start,
                                start + found.len(),
                                None,
                            ));
                            previous = start + found.len();
                        }
                        out.push_str(&text[previous..]);
                        out
                    }
                    Value::Regex { pattern, flags } => {
                        let regex = regress::Regex::with_flags(pattern, flags.as_str())
                            .map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;
                        let mut out = String::new();
                        let mut previous = 0;
                        for m in regex.find_iter(&text) {
                            if flags.contains('y') && m.start() != previous {
                                break;
                            }
                            out.push_str(&text[previous..m.start()]);
                            out.push_str(&expand_replacement(
                                replacement,
                                &text,
                                m.start(),
                                m.end(),
                                Some(&m),
                            ));
                            previous = m.end();
                            if !flags.contains('g') {
                                break;
                            }
                        }
                        out.push_str(&text[previous..]);
                        out
                    }
                    _ => bail!("replace pattern must be a string or regular expression"),
                })
            }
            "split" => {
                let limit = if args.len() == 2 {
                    let n = coerce_number(&args[1])?;
                    if !n.is_finite() {
                        0
                    } else {
                        (n.trunc() as i64).rem_euclid(1i64 << 32) as usize
                    }
                } else {
                    usize::MAX
                };
                let parts = match &args[0] {
                    Value::String(separator) => {
                        if separator.is_empty() {
                            text.encode_utf16().map(|unit|String::from_utf16(&[unit]).context("string.split split a UTF-16 surrogate pair; result cannot be represented as Unicode text")).collect::<anyhow::Result<Vec<_>>>()?
                        } else {
                            text.split(separator).map(String::from).collect()
                        }
                    }
                    Value::Regex { pattern, flags } => {
                        let regex = regress::Regex::with_flags(pattern, flags.as_str())
                            .map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;
                        let mut parts = Vec::new();
                        let mut previous = 0;
                        for m in regex.find_iter(&text) {
                            if m.start() == m.end()
                                && (m.start() == previous || m.end() == text.len())
                            {
                                continue;
                            }
                            parts.push(text[previous..m.start()].into());
                            for group in &m.captures {
                                parts.push(
                                    group
                                        .as_ref()
                                        .map(|r| text[r.clone()].into())
                                        .unwrap_or_default(),
                                );
                            }
                            previous = m.end();
                        }
                        parts.push(text[previous..].into());
                        parts
                    }
                    _ => bail!("split separator must be a string or regular expression"),
                };
                Value::List(parts.into_iter().take(limit).map(Value::String).collect())
            }
            _ => bail!("string has no method {name}"),
        })
    }
    fn list_method(
        &self,
        mut items: Vec<Value>,
        name: &str,
        args: &[Value],
    ) -> anyhow::Result<Value> {
        Ok(match name {
            "contains" => Value::Bool(items.iter().any(|v| v.equal(&args[0]))),
            "containsAll" => Value::Bool(args.iter().all(|arg| items.iter().any(|v| v.equal(arg)))),
            "containsAny" => Value::Bool(args.iter().any(|arg| items.iter().any(|v| v.equal(arg)))),
            "flat" => Value::List(
                items
                    .into_iter()
                    .flat_map(|v| {
                        if let Value::List(items) = v {
                            items
                        } else {
                            vec![v]
                        }
                    })
                    .collect(),
            ),
            "join" => Value::String(
                items
                    .iter()
                    .map(|v| {
                        if matches!(v, Value::Null) {
                            String::new()
                        } else {
                            self.data.display(v)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(args[0].string()?),
            ),
            "reverse" => {
                items.reverse();
                Value::List(items)
            }
            "sort" => {
                items.sort_by(Value::compare);
                Value::List(items)
            }
            "slice" => {
                let (start, end) = slice_bounds(items.len(), args)?;
                Value::List(items[start..end].to_vec())
            }
            "unique" => {
                let mut unique: Vec<Value> = Vec::new();
                for item in items {
                    if !unique.iter().any(|v| v.equal(&item)) {
                        unique.push(item);
                    }
                }
                Value::List(unique)
            }
            "mean" | "sum" | "min" | "max" | "median" | "stddev" => aggregate(name, &items)?,
            _ => bail!("list has no method {name}"),
        })
    }
}
pub fn coerce_number(value: &Value) -> anyhow::Result<f64> {
    Ok(match value {
        Value::Number(v) => *v,
        Value::Bool(v) => {
            if *v {
                1.0
            } else {
                0.0
            }
        }
        Value::Null => 0.0,
        Value::Date(v) | Value::DateOnly(v) => v.timestamp_millis() as f64,
        Value::Duration {
            months,
            days,
            milliseconds,
        } => months * 146097.0 / 4800.0 * 86400000.0 + days * 86400000.0 + milliseconds,
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                0.0
            } else if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
                u64::from_str_radix(hex, 16)? as f64
            } else if let Some(bin) = text.strip_prefix("0b").or_else(|| text.strip_prefix("0B")) {
                u64::from_str_radix(bin, 2)? as f64
            } else if let Some(oct) = text.strip_prefix("0o").or_else(|| text.strip_prefix("0O")) {
                u64::from_str_radix(oct, 8)? as f64
            } else {
                let number = text
                    .parse::<f64>()
                    .with_context(|| format!("cannot convert {text:?} to number"))?;
                if number.is_nan() {
                    bail!("cannot convert {text:?} to number");
                }
                number
            }
        }
        _ => bail!("cannot convert {} to number", value.type_name()),
    })
}
fn slice_bounds(len: usize, args: &[Value]) -> anyhow::Result<(usize, usize)> {
    fn bound(n: f64, len: usize) -> usize {
        if n.is_nan() {
            0
        } else if n < 0.0 {
            (len as f64 + n.trunc()).clamp(0.0, len as f64) as usize
        } else {
            n.trunc().min(len as f64) as usize
        }
    }
    let start = bound(coerce_number(&args[0])?, len);
    let end = if args.len() == 2 {
        bound(coerce_number(&args[1])?, len)
    } else {
        len
    };
    Ok((start, end.max(start)))
}
pub fn aggregate(name: &str, values: &[Value]) -> anyhow::Result<Value> {
    let mut numbers: Vec<f64> = values
        .iter()
        .filter_map(|v| {
            if let Value::Number(n) = v {
                Some(*n)
            } else {
                None
            }
        })
        .collect();
    if numbers.is_empty() {
        return Ok(if name == "sum" {
            Value::Number(0.0)
        } else {
            Value::Null
        });
    }
    let sum: f64 = numbers.iter().sum();
    let mean = sum / numbers.len() as f64;
    Ok(Value::Number(match name {
        "sum" => sum,
        "mean" => mean,
        "min" => numbers.into_iter().fold(f64::INFINITY, f64::min),
        "max" => numbers.into_iter().fold(f64::NEG_INFINITY, f64::max),
        "median" => {
            numbers.sort_by(f64::total_cmp);
            let mid = numbers.len() / 2;
            if numbers.len().is_multiple_of(2) {
                (numbers[mid - 1] + numbers[mid]) / 2.0
            } else {
                numbers[mid]
            }
        }
        "stddev" => {
            (numbers.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / numbers.len() as f64).sqrt()
        }
        _ => bail!("unknown aggregate: {name}"),
    }))
}
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
/// ECMA-262 toFixed rounds the exact binary float, choosing the larger integer
/// on a tie. Multiplying f64 by 10^precision first would change cases like 2.675.
fn to_fixed(number: f64, digits: usize) -> String {
    use num_bigint::BigUint;
    if !number.is_finite() || number.abs() >= 1e21 {
        return Value::Number(number).text();
    }
    let bits = number.abs().to_bits();
    let exponent = ((bits >> 52) & 0x7ff) as i32;
    let mantissa = (bits & ((1u64 << 52) - 1)) | if exponent == 0 { 0 } else { 1u64 << 52 };
    let power = if exponent == 0 {
        -1074
    } else {
        exponent - 1023 - 52
    };
    let mut integer = BigUint::from(mantissa) * BigUint::from(10u8).pow(digits as u32);
    if power >= 0 {
        integer <<= power as usize;
    } else {
        let shift = (-power) as usize;
        let half = BigUint::from(1u8) << (shift - 1);
        integer = (integer + half) >> shift;
    }
    let mut value = integer.to_str_radix(10);
    if digits > 0 {
        if value.len() <= digits {
            value = format!("{}{}", "0".repeat(digits + 1 - value.len()), value);
        }
        value.insert(value.len() - digits, '.');
    }
    if number < 0.0 {
        value.insert(0, '-');
    }
    value
}
fn expand_replacement(
    replacement: &str,
    text: &str,
    start: usize,
    end: usize,
    matched: Option<&regress::Match>,
) -> String {
    let chars: Vec<char> = replacement.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        i += 1;
        if ch != '$' || i == chars.len() {
            out.push(ch);
            continue;
        }
        let next = chars[i];
        match next {
            '$' => {
                out.push('$');
                i += 1;
            }
            '&' => {
                out.push_str(&text[start..end]);
                i += 1;
            }
            '`' => {
                out.push_str(&text[..start]);
                i += 1;
            }
            '\'' => {
                out.push_str(&text[end..]);
                i += 1;
            }
            '1'..='9' if matched.is_some() => {
                let m = matched.unwrap();
                let first = next.to_digit(10).unwrap() as usize;
                let mut number = first;
                let mut consumed = 1;
                if let Some(second) = chars.get(i + 1).and_then(|c| c.to_digit(10)) {
                    let two = number * 10 + second as usize;
                    if two <= m.captures.len() {
                        number = two;
                        consumed = 2;
                    }
                }
                if number <= m.captures.len() {
                    if let Some(range) = m.group(number) {
                        out.push_str(&text[range]);
                    }
                    i += consumed;
                } else {
                    out.push('$');
                }
            }
            '<' if matched.is_some_and(|m| m.named_groups().next().is_some()) => {
                if let Some(offset) = chars[i + 1..].iter().position(|ch| *ch == '>') {
                    let name: String = chars[i + 1..i + 1 + offset].iter().collect();
                    if let Some(range) = matched.unwrap().named_group(&name) {
                        out.push_str(&text[range]);
                    }
                    i += offset + 2;
                } else {
                    out.push('$');
                }
            }
            _ => out.push('$'),
        }
    }
    out
}
