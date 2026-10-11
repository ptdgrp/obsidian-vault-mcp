//! A local, strictly diagnosed evaluator for the public Obsidian Bases syntax.
mod data;
mod dates;
mod eval;
mod expression;
mod value;

use crate::{
    query::{McpNonNegativeInteger, VaultQueries},
    rendered::{RenderPagination, default_page, paginate, read_source},
};
use anyhow::{Context, bail};
use chrono::Local;
use data::Dataset;
use eval::Eval;
use expression::Expr;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use value::{Value, escape_cell};

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct ReadBaseRequest {
    /// Exact vault-relative .base path. Rendered output never uses source-line references.
    pub path: String,
    /// Exact unique view name from query_base_views; omit to use the first view in file order.
    pub view: Option<String>,
    /// Exact embedding/active file path for this; defaults to the .base itself.
    pub context_note: Option<String>,
    #[serde(default = "default_page")]
    #[schemars(with = "McpNonNegativeInteger", range(min = 1))]
    pub page: usize,
    /// Unicode-character page budget. Complete rows plus headers and summaries may exceed it.
    #[schemars(with = "Option<McpNonNegativeInteger>", range(min = 0))]
    pub max_chars: Option<usize>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema)]
pub struct QueryBaseViewsRequest {
    /// Exact vault-relative .base path. Only view configuration is inspected.
    pub path: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct QueryBaseViewsResult {
    pub path: String,
    /// Views in file order. Names are returned exactly, including null and duplicates.
    pub views: Vec<BaseViewSummary>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct BaseViewSummary {
    /// One-based position for discovery, not a read_base selector.
    #[schemars(with = "McpNonNegativeInteger", range(min = 1))]
    pub position: usize,
    /// Original name, or null for an unnamed view; no selectable name is invented.
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    /// True only for the first view, which read_base uses when view is omitted.
    pub is_default: bool,
}

impl VaultQueries {
    pub fn query_base_views(
        &self,
        request: QueryBaseViewsRequest,
    ) -> anyhow::Result<QueryBaseViewsResult> {
        let (path, source) = read_source(&self.vault, &request.path, "base")?;
        // Parse a value first so duplicate keys are rejected even in fields that
        // discovery does not interpret. No expression or dataset is evaluated.
        let yaml: serde_yaml_ng::Value = serde_yaml_ng::from_str(&source)
            .with_context(|| format!("invalid Base YAML: {path}"))?;
        #[derive(Deserialize)]
        struct Config {
            views: Vec<ConfiguredView>,
        }
        #[derive(Deserialize)]
        struct ConfiguredView {
            name: Option<String>,
            #[serde(rename = "type")]
            kind: String,
        }
        if !yaml.is_mapping() {
            bail!("invalid Base YAML: {path}: expected a mapping");
        }
        let config: Config = serde_yaml_ng::from_value(yaml)
            .with_context(|| format!("invalid Base view configuration: {path}"))?;
        Ok(QueryBaseViewsResult {
            path,
            views: config
                .views
                .into_iter()
                .enumerate()
                .map(|(index, view)| BaseViewSummary {
                    position: index + 1,
                    name: view.name,
                    kind: view.kind,
                    is_default: index == 0,
                })
                .collect(),
        })
    }
}
#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct ReadBaseResult {
    pub path: String,
    pub view: String,
    pub view_type: String,
    pub context: String,
    /// Always markdown. All supported layouts use a semantic Markdown table.
    pub format: String,
    pub content: String,
    /// Rows after filters, group visibility, sorting and the view's limit, before pagination.
    #[schemars(with = "McpNonNegativeInteger")]
    pub total_rows: usize,
    /// Exact file paths for data rows on this page, in rendered order; empty groups are excluded.
    pub row_paths: Vec<String>,
    pub pagination: RenderPagination,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Base {
    filters: Option<Filter>,
    #[serde(default)]
    formulas: BTreeMap<String, String>,
    #[serde(default)]
    properties: BTreeMap<String, Property>,
    #[serde(default)]
    summaries: BTreeMap<String, String>,
    views: Vec<View>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Property {
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Filter {
    Text(String),
    Group(BTreeMap<String, Vec<Filter>>),
}
enum CompiledFilter {
    Expr(Expr),
    And(Vec<CompiledFilter>),
    Or(Vec<CompiledFilter>),
    Not(Vec<CompiledFilter>),
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct View {
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
    filters: Option<Filter>,
    limit: Option<usize>,
    #[serde(default)]
    order: Vec<String>,
    #[serde(default)]
    sort: Vec<Sort>,
    group_by: Option<Sort>,
    group_order: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    summaries: BTreeMap<String, String>,
    image: Option<String>,
    coordinates: Option<String>,
    marker_icon: Option<String>,
    marker_color: Option<String>,
    #[serde(flatten)]
    settings: BTreeMap<String, serde_json::Value>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sort {
    property: String,
    #[serde(default)]
    direction: Direction,
}
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
enum Direction {
    #[default]
    Asc,
    Desc,
}
struct Row {
    values: Vec<Value>,
    sort: Vec<Value>,
    group: Value,
    path: String,
    summary_values: BTreeMap<String, Value>,
}
struct Group {
    value: Value,
    rows: Vec<Row>,
    summaries: Vec<Summary>,
}
struct Summary {
    property: String,
    function: String,
    value: Value,
}
#[derive(Clone, Copy)]
struct Unit {
    group: usize,
    row: Option<usize>,
}

impl VaultQueries {
    pub fn read_base(&self, request: ReadBaseRequest) -> anyhow::Result<ReadBaseResult> {
        let (path, source) = read_source(&self.vault, &request.path, "base")?;
        let base: Base = serde_yaml_ng::from_str(&source)
            .with_context(|| format!("invalid Base YAML: {path}"))?;
        let run = || -> anyhow::Result<ReadBaseResult> {
            if base.views.is_empty() {
                bail!("base has no views");
            }
            let selected = if let Some(name) = &request.view {
                let found: Vec<_> = base
                    .views
                    .iter()
                    .filter(|view| view.name.as_ref() == Some(name))
                    .collect();
                if found.len() != 1 {
                    bail!(
                        "view {name:?} must identify exactly one view (found {})",
                        found.len()
                    );
                }
                found[0]
            } else {
                &base.views[0]
            };
            validate_view(selected)?;
            let view = selected.name.clone().unwrap_or_else(|| "(unnamed)".into());
            let formulas = base
                .formulas
                .iter()
                .map(|(name, source)| {
                    Ok((
                        name.clone(),
                        expression::parse(source).with_context(|| format!("formula.{name}"))?,
                    ))
                })
                .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
            validate_dependencies(&formulas)?;
            let custom = base
                .summaries
                .iter()
                .map(|(name, source)| {
                    Ok((
                        name.clone(),
                        expression::parse(source).with_context(|| format!("summary {name}"))?,
                    ))
                })
                .collect::<anyhow::Result<BTreeMap<_, _>>>()?;
            for name in selected.summaries.values() {
                if !custom.contains_key(name) && !SUMMARY_NAMES.contains(&name.as_str()) {
                    bail!("unsupported summary: {name}");
                }
            }
            let global = base.filters.as_ref().map(compile_filter).transpose()?;
            let local = selected.filters.as_ref().map(compile_filter).transpose()?;
            let mut columns = if selected.order.is_empty() {
                vec!["file.name".into()]
            } else {
                selected.order.clone()
            };
            if let Some(image) = &selected.image
                && !columns.contains(image)
            {
                columns.push(image.clone());
            }
            if selected.kind == "map" {
                for property in [
                    &selected.coordinates,
                    &selected.marker_icon,
                    &selected.marker_color,
                ]
                .into_iter()
                .flatten()
                {
                    if !columns.contains(property) {
                        columns.push(property.clone());
                    }
                }
            }
            for property in columns
                .iter()
                .chain(selected.sort.iter().map(|sort| &sort.property))
                .chain(selected.group_by.iter().map(|sort| &sort.property))
                .chain(selected.summaries.keys())
            {
                validate_property(property, &formulas)?;
            }
            let data = Dataset::new(self)?;
            let context = data.exact(request.context_note.as_deref().unwrap_or(&path))?;
            let now = Local::now();
            let mut map_metadata = String::new();
            if selected.kind == "map" {
                if let Some(center) = selected.settings.get("center") {
                    let value = if let Some(source) = center.as_str() {
                        if source.split(',').count() == 2
                            && source
                                .split(',')
                                .all(|part| part.trim().parse::<f64>().is_ok())
                        {
                            Value::String(source.into())
                        } else {
                            Eval::new(&data, context, context, now, &formulas)
                                .eval(&expression::parse(source).context("map center formula")?)?
                        }
                    } else {
                        data.convert(center, context, None)?
                    };
                    map_metadata.push_str(&format!("Map center: {}\n\n", cell(&data, &value)));
                }
                for key in ["mapTiles", "mapTilesDark"] {
                    if let Some(value) = selected.settings.get(key) {
                        map_metadata
                            .push_str(&format!("{key}: {}\n\n", escape_cell(&value.to_string())));
                    }
                }
            }
            let mut rows = Vec::new();
            for index in 0..data.candidates {
                let mut eval = Eval::new(&data, index, context, now, &formulas);
                if global
                    .as_ref()
                    .map(|filter| test_filter(filter, &mut eval))
                    .transpose()?
                    .is_some_and(|matched| !matched)
                {
                    continue;
                }
                if local
                    .as_ref()
                    .map(|filter| test_filter(filter, &mut eval))
                    .transpose()?
                    .is_some_and(|matched| !matched)
                {
                    continue;
                }
                let group = if let Some(sort) = &selected.group_by {
                    eval.property(&sort.property)?
                } else {
                    Value::Null
                };
                let allowed = selected
                    .group_order
                    .as_ref()
                    .map(|values| {
                        values
                            .iter()
                            .map(|value| data.convert(value, context, None))
                            .collect::<anyhow::Result<Vec<_>>>()
                    })
                    .transpose()?;
                if allowed
                    .as_ref()
                    .is_some_and(|allowed| !allowed.iter().any(|value| value.equal(&group)))
                {
                    continue;
                }
                let values = columns
                    .iter()
                    .map(|name| eval.property(name))
                    .collect::<anyhow::Result<_>>()?;
                let sort = selected
                    .sort
                    .iter()
                    .map(|sort| eval.property(&sort.property))
                    .collect::<anyhow::Result<_>>()?;
                let summary_values = selected
                    .summaries
                    .keys()
                    .map(|name| Ok((name.clone(), eval.property(name)?)))
                    .collect::<anyhow::Result<_>>()?;
                rows.push(Row {
                    values,
                    sort,
                    group,
                    path: data.file(index).relative_path,
                    summary_values,
                });
            }
            rows.sort_by(|a, b| {
                for (i, sort) in selected.sort.iter().enumerate() {
                    let order = a.sort[i].compare(&b.sort[i]);
                    let order = if matches!(sort.direction, Direction::Desc) {
                        order.reverse()
                    } else {
                        order
                    };
                    if !order.is_eq() {
                        return order;
                    }
                }
                natord::compare(&a.path, &b.path)
            });
            if let Some(limit) = selected.limit {
                rows.truncate(limit);
            }
            let total_rows = rows.len();
            let overall = summarize(&data, &rows, selected, &custom, context, now, &formulas)?;
            let mut groups: Vec<Group> = Vec::new();
            if let Some(order) = &selected.group_order {
                for value in order {
                    let value = data.convert(value, context, None)?;
                    if groups.iter().any(|group| group.value.equal(&value)) {
                        bail!("duplicate groupOrder value: {}", value.text());
                    }
                    groups.push(Group {
                        value,
                        rows: Vec::new(),
                        summaries: Vec::new(),
                    });
                }
            }
            for row in rows {
                let index = groups
                    .iter()
                    .position(|group| group.value.equal(&row.group))
                    .unwrap_or_else(|| {
                        groups.push(Group {
                            value: row.group.clone(),
                            rows: Vec::new(),
                            summaries: Vec::new(),
                        });
                        groups.len() - 1
                    });
                groups[index].rows.push(row);
            }
            if selected.group_order.is_none()
                && let Some(sort) = &selected.group_by
            {
                groups.sort_by(|a, b| {
                    let order = a.value.compare(&b.value);
                    if matches!(sort.direction, Direction::Desc) {
                        order.reverse()
                    } else {
                        order
                    }
                });
            }
            for group in &mut groups {
                group.summaries = summarize(
                    &data,
                    &group.rows,
                    selected,
                    &custom,
                    context,
                    now,
                    &formulas,
                )?;
            }
            if selected.kind == "kanban"
                && selected
                    .settings
                    .get("hideEmptyColumns")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            {
                groups.retain(|group| !group.rows.is_empty());
            }
            let units: Vec<Unit> = groups
                .iter()
                .enumerate()
                .flat_map(|(i, group)| {
                    if group.rows.is_empty() {
                        vec![Unit {
                            group: i,
                            row: None,
                        }]
                    } else {
                        (0..group.rows.len())
                            .map(|row| Unit {
                                group: i,
                                row: Some(row),
                            })
                            .collect()
                    }
                })
                .collect();
            let budget = request
                .max_chars
                .unwrap_or(self.vault.config().max_read_note_chars);
            let (content, pagination, span) = paginate(&units, request.page, budget, |units| {
                format!(
                    "{map_metadata}{}",
                    render(
                        &data, &base, selected, &view, &columns, &groups, units, &overall
                    )
                )
            })?;
            let row_paths = units[span]
                .iter()
                .filter_map(|unit| {
                    unit.row
                        .map(|row| groups[unit.group].rows[row].path.clone())
                })
                .collect();
            Ok(ReadBaseResult {
                path: path.clone(),
                view,
                view_type: selected.kind.clone(),
                context: data.file(context).relative_path,
                format: "markdown".into(),
                content,
                total_rows,
                row_paths,
                pagination,
            })
        };
        run().map_err(|error| anyhow::anyhow!("reading Base {path}: {error:#}"))
    }
}
fn validate_view(view: &View) -> anyhow::Result<()> {
    if !["table", "cards", "list", "kanban", "map"].contains(&view.kind.as_str()) {
        bail!(
            "unsupported Base view type {:?}; select a table/cards/list/kanban/map view",
            view.kind
        );
    }
    // These are exclusively visual settings in built-in layouts.
    for key in view.settings.keys() {
        if ![
            "columnSize",
            "columnWidths",
            "columnWidth",
            "hideEmptyColumns",
            "rowHeight",
            "cardSize",
            "imageFit",
            "imageAspectRatio",
            "imageAspect",
            "fontSize",
            "markers",
            "indent",
            "separator",
            "nestedProperties",
            "showPropertyLabels",
            "center",
            "defaultZoom",
            "minZoom",
            "maxZoom",
            "mapHeight",
            "mapTiles",
            "mapTilesDark",
        ]
        .contains(&key.as_str())
        {
            bail!("unsupported Base view setting {key:?}; its semantics cannot be safely ignored");
        }
    }
    if view.group_order.is_some() && view.group_by.is_none() {
        bail!("groupOrder requires groupBy");
    }
    Ok(())
}
fn validate_property(name: &str, formulas: &BTreeMap<String, Expr>) -> anyhow::Result<()> {
    if name.is_empty() {
        bail!("empty property name");
    }
    if let Some(name) = name.strip_prefix("formula.")
        && !formulas.contains_key(name)
    {
        bail!("unknown formula: {name}");
    }
    if let Some(name) = name.strip_prefix("file.")
        && ![
            "file",
            "name",
            "basename",
            "path",
            "folder",
            "ext",
            "size",
            "properties",
            "tags",
            "links",
            "embeds",
            "backlinks",
            "ctime",
            "mtime",
        ]
        .contains(&name)
    {
        bail!("unknown file field: {name}");
    }
    Ok(())
}
fn compile_filter(filter: &Filter) -> anyhow::Result<CompiledFilter> {
    Ok(match filter {
        Filter::Text(source) => CompiledFilter::Expr(expression::parse(source)?),
        Filter::Group(map) => {
            if map.len() != 1 {
                bail!("filter objects require exactly one of and/or/not");
            }
            let (name, filters) = map.iter().next().unwrap();
            let filters = filters
                .iter()
                .map(compile_filter)
                .collect::<anyhow::Result<_>>()?;
            match name.as_str() {
                "and" => CompiledFilter::And(filters),
                "or" => CompiledFilter::Or(filters),
                "not" => CompiledFilter::Not(filters),
                _ => bail!("unknown filter conjunction: {name}"),
            }
        }
    })
}
fn test_filter(filter: &CompiledFilter, eval: &mut Eval<'_, '_>) -> anyhow::Result<bool> {
    match filter {
        CompiledFilter::Expr(expr) => Ok(eval.eval(expr)?.truthy()),
        CompiledFilter::And(filters) => {
            for filter in filters {
                if !test_filter(filter, eval)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        CompiledFilter::Or(filters) => {
            for filter in filters {
                if test_filter(filter, eval)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        CompiledFilter::Not(filters) => {
            for filter in filters {
                if test_filter(filter, eval)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}
fn validate_dependencies(formulas: &BTreeMap<String, Expr>) -> anyhow::Result<()> {
    fn references(expr: &Expr, out: &mut Vec<String>) {
        match expr {
            Expr::Member(base, key) => {
                if matches!(base.as_ref(),Expr::Ident(name) if name=="formula")
                    && let Expr::Literal(Value::String(name)) = key.as_ref()
                {
                    out.push(name.clone());
                }
                references(base, out);
                references(key, out);
            }
            Expr::Unary(_, expr) => references(expr, out),
            Expr::Binary(_, a, b) => {
                references(a, out);
                references(b, out);
            }
            Expr::Call(callee, args) => {
                references(callee, out);
                for arg in args {
                    references(arg, out);
                }
            }
            Expr::List(items) => {
                for item in items {
                    references(item, out);
                }
            }
            Expr::Object(items) => {
                for (_, item) in items {
                    references(item, out);
                }
            }
            _ => {}
        }
    }
    fn visit(
        name: &str,
        formulas: &BTreeMap<String, Expr>,
        done: &mut HashSet<String>,
        stack: &mut Vec<String>,
    ) -> anyhow::Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        if stack.len() >= 128 {
            bail!("formula dependency depth exceeds 128 at {name}");
        }
        if stack.iter().any(|key| key == name) {
            bail!(
                "circular formula dependency: {} -> {name}",
                stack.join(" -> ")
            );
        }
        let expr = formulas
            .get(name)
            .with_context(|| format!("unknown formula: {name}"))?;
        stack.push(name.into());
        let mut refs = Vec::new();
        references(expr, &mut refs);
        for reference in refs {
            visit(&reference, formulas, done, stack)?;
        }
        stack.pop();
        done.insert(name.into());
        Ok(())
    }
    let mut done = HashSet::new();
    for name in formulas.keys() {
        visit(name, formulas, &mut done, &mut Vec::new())?;
    }
    Ok(())
}
const SUMMARY_NAMES: &[&str] = &[
    "Average",
    "Min",
    "Max",
    "Sum",
    "Range",
    "Median",
    "Stddev",
    "Earliest",
    "Latest",
    "Checked",
    "Unchecked",
    "Empty",
    "Filled",
    "Unique",
];
fn summarize(
    data: &Dataset<'_>,
    rows: &[Row],
    view: &View,
    custom: &BTreeMap<String, Expr>,
    context: usize,
    now: chrono::DateTime<Local>,
    formulas: &BTreeMap<String, Expr>,
) -> anyhow::Result<Vec<Summary>> {
    view.summaries
        .iter()
        .map(|(property, function)| {
            let values: Vec<_> = rows
                .iter()
                .map(|row| row.summary_values[property].clone())
                .collect();
            let value = if let Some(expr) = custom.get(function) {
                let mut eval = Eval::new(data, context, context, now, formulas);
                eval.vars.insert("values".into(), Value::List(values));
                eval.eval(expr)
                    .with_context(|| format!("custom summary {function} for {property}"))?
            } else {
                default_summary(function, &values)?
            };
            Ok(Summary {
                property: property.clone(),
                function: function.clone(),
                value,
            })
        })
        .collect()
}
fn default_summary(name: &str, values: &[Value]) -> anyhow::Result<Value> {
    Ok(match name {
        "Average" | "Min" | "Max" | "Sum" | "Median" | "Stddev" => eval::aggregate(
            match name {
                "Average" => "mean",
                "Min" => "min",
                "Max" => "max",
                "Sum" => "sum",
                "Median" => "median",
                _ => "stddev",
            },
            values,
        )?,
        "Earliest" | "Latest" => {
            let dates: Vec<_> = values
                .iter()
                .filter(|v| matches!(v, Value::Date(_) | Value::DateOnly(_)))
                .cloned()
                .collect();
            if name == "Earliest" {
                dates.into_iter().min_by(Value::compare)
            } else {
                dates.into_iter().max_by(Value::compare)
            }
            .unwrap_or(Value::Null)
        }
        "Range" => {
            let dates: Vec<_> = values
                .iter()
                .filter_map(|v| {
                    if let Value::Date(date) | Value::DateOnly(date) = v {
                        Some(date.timestamp_millis())
                    } else {
                        None
                    }
                })
                .collect();
            if !dates.is_empty() {
                Value::Number((dates.iter().max().unwrap() - dates.iter().min().unwrap()) as f64)
            } else {
                match (
                    eval::aggregate("min", values)?,
                    eval::aggregate("max", values)?,
                ) {
                    (Value::Number(min), Value::Number(max)) => Value::Number(max - min),
                    _ => Value::Null,
                }
            }
        }
        "Checked" => Value::Number(
            values
                .iter()
                .filter(|v| matches!(v, Value::Bool(true)))
                .count() as f64,
        ),
        "Unchecked" => Value::Number(
            values
                .iter()
                .filter(|v| matches!(v, Value::Bool(false)))
                .count() as f64,
        ),
        "Empty" => Value::Number(values.iter().filter(|v| v.empty()).count() as f64),
        "Filled" => Value::Number(values.iter().filter(|v| !v.empty()).count() as f64),
        "Unique" => {
            let mut unique: Vec<&Value> = Vec::new();
            for value in values {
                if !unique.iter().any(|v| v.equal(value)) {
                    unique.push(value);
                }
            }
            Value::Number(unique.len() as f64)
        }
        _ => bail!("unsupported summary: {name}"),
    })
}
#[allow(clippy::too_many_arguments)]
fn render(
    data: &Dataset<'_>,
    base: &Base,
    view: &View,
    name: &str,
    columns: &[String],
    groups: &[Group],
    units: &[Unit],
    overall: &[Summary],
) -> String {
    let mut out = format!("# {}\n\n", escape_cell(name));
    let mut current = None;
    for unit in units {
        let group = &groups[unit.group];
        if current != Some(unit.group) {
            current = Some(unit.group);
            if let Some(group_by) = &view.group_by {
                out.push_str(&format!(
                    "\n## {}: {}\n\n",
                    escape_cell(display_name(base, &group_by.property)),
                    if matches!(group.value, Value::Null) {
                        "(empty)".into()
                    } else {
                        cell(data, &group.value)
                    }
                ));
                render_summaries(&mut out, data, base, &group.summaries, "Group summaries");
            }
            out.push_str("| ");
            out.push_str(
                &columns
                    .iter()
                    .map(|name| escape_cell(display_name(base, name)))
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
            out.push_str(" |\n| ");
            out.push_str(&vec!["---"; columns.len()].join(" | "));
            out.push_str(" |\n");
        }
        if let Some(row) = unit.row {
            out.push_str("| ");
            out.push_str(
                &group.rows[row]
                    .values
                    .iter()
                    .map(|value| cell(data, value))
                    .collect::<Vec<_>>()
                    .join(" | "),
            );
            out.push_str(" |\n");
        } else {
            out.push_str("\n_No rows in this group._\n");
        }
    }
    if units.is_empty() {
        out.push_str("_No rows on this page._\n");
    }
    render_summaries(
        &mut out,
        data,
        base,
        overall,
        "Summaries (all result rows after limit, before pagination)",
    );
    out
}
fn display_name<'a>(base: &'a Base, name: &'a str) -> &'a str {
    let config = base.properties.get(name).or_else(|| {
        if let Some(bare) = name.strip_prefix("note.") {
            base.properties.get(bare)
        } else {
            base.properties.get(&format!("note.{name}"))
        }
    });
    config
        .and_then(|p| p.display_name.as_deref())
        .unwrap_or(name)
}
fn render_summaries(
    out: &mut String,
    data: &Dataset<'_>,
    base: &Base,
    summaries: &[Summary],
    label: &str,
) {
    if summaries.is_empty() {
        return;
    }
    out.push_str(&format!(
        "\n### {label}\n\n| Property | Function | Value |\n| --- | --- | --- |\n"
    ));
    for summary in summaries {
        out.push_str(&format!(
            "| {} | {} | {} |\n",
            escape_cell(display_name(base, &summary.property)),
            escape_cell(&summary.function),
            cell(data, &summary.value)
        ));
    }
    out.push('\n');
}
fn cell(data: &Dataset<'_>, value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::File(index) | Value::This(index) => {
            let path = data.file(*index).relative_path;
            format!("[{}]({})", escape_cell(&path), url(&path))
        }
        Value::Link { path, display, .. } => format!(
            "[{}]({})",
            display
                .as_ref()
                .map(|value| escape_cell(&data.display(value)))
                .unwrap_or_else(|| escape_cell(path)),
            url(path)
        ),
        Value::List(values) => values
            .iter()
            .map(|value| cell(data, value))
            .collect::<Vec<_>>()
            .join(", "),
        Value::Image(path) => format!("Image: [{}]({})", escape_cell(path), url(path)),
        _ => escape_cell(&data.display(value)),
    }
}
fn url(path: &str) -> String {
    let mut out = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:?#=&+@!$,;".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests;
