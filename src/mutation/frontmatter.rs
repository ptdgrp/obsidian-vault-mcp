//! Single-field YAML edits. The CST supplies a local edit; semantic and byte
//! checks reject changes that would affect unrelated metadata or Markdown.
use std::{collections::BTreeMap, str::FromStr};

use anyhow::{Context, bail};
use serde_json::Value as JsonValue;
use serde_yaml_ng::{Mapping, Value as YamlValue};
use yaml_edit::{AsYaml, YamlFile};

use super::{SetFrontmatterFieldResult, VaultMutations, text::PlannedEdit};

impl VaultMutations {
    pub fn set_frontmatter_field(
        &self,
        note: &str,
        field: &str,
        value: JsonValue,
        dry_run: bool,
    ) -> anyhow::Result<SetFrontmatterFieldResult> {
        let (path, relative_path, original) = self.read_editable_note(note)?;
        let header = Header::find(&original)?;
        let yaml = header
            .as_ref()
            .map(|header| &original[header.start..header.end])
            .unwrap_or("");
        let before = parse_mapping(yaml).context("invalid frontmatter")?;
        let effective_before = merged(&before)?;
        let key = YamlValue::String(field.into());
        let previous = effective_before.get(&key).cloned();
        let previous_value = previous
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .context("previous field value cannot be represented as JSON")?
            .unwrap_or(JsonValue::Null);
        let new_value = serde_yaml_ng::to_value(&value)?;
        let changed = previous.as_ref() != Some(&new_value);
        let result = SetFrontmatterFieldResult {
            path: relative_path.clone(),
            field: field.into(),
            dry_run,
            changed,
            previous_exists: previous.is_some(),
            previous_value,
            result_value: value.clone(),
        };
        if !changed {
            return Ok(result);
        }

        let newline = header
            .as_ref()
            .map(|header| header.newline)
            .unwrap_or_else(|| newline_for(&original));
        let updated_yaml = edit_yaml(yaml, field, &value, newline)?;
        let after = parse_mapping(&updated_yaml)
            .context("frontmatter edit produced invalid YAML; no changes written")?;
        let effective_after = merged(&after)?;
        if effective_after.get(&key) != Some(&new_value) {
            bail!(
                "frontmatter editor could not set the requested JSON value safely; no changes written"
            );
        }
        let mut expected = effective_before;
        expected.insert(key.clone(), new_value);
        if effective_after != expected {
            bail!(
                "frontmatter edit would change other metadata through aliases or YAML merges; no changes written"
            );
        }
        // Also preserve the explicit mapping, including merge/anchor content,
        // rather than relying only on the resolved view of the properties.
        let mut other_before = before;
        let mut other_after = after;
        other_before.remove(&key);
        other_after.remove(&key);
        if other_before != other_after {
            bail!("frontmatter edit would change other metadata; no changes written");
        }
        let updated = if let Some(header) = header {
            let mut updated = original.clone();
            updated.replace_range(header.start..header.end, &updated_yaml);
            updated
        } else {
            let bom = usize::from(original.starts_with('\u{feff}')) * '\u{feff}'.len_utf8();
            format!(
                "{}---{newline}{updated_yaml}---{newline}{}",
                &original[..bom],
                &original[bom..]
            )
        };
        let mut edits = BTreeMap::new();
        edits.insert(
            relative_path,
            PlannedEdit {
                path,
                original,
                updated,
            },
        );
        self.apply_planned_edits("set_frontmatter_field", edits, dry_run)?;
        Ok(result)
    }
}

struct Header {
    start: usize,
    end: usize,
    newline: &'static str,
}
impl Header {
    fn find(source: &str) -> anyhow::Result<Option<Self>> {
        let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
        let mut lines = source[bom..].split_inclusive('\n');
        let Some(first) = lines.next() else {
            return Ok(None);
        };
        // Only column-zero delimiters count. An indented --- may be literal
        // block-scalar content and must never terminate the header.
        if first.trim_end() != "---" {
            return Ok(None);
        }
        let start = bom + first.len();
        let mut offset = start;
        for line in lines {
            if line.trim_end() == "---" {
                return Ok(Some(Self {
                    start,
                    end: offset,
                    newline: newline_for(first),
                }));
            }
            offset += line.len();
        }
        bail!("frontmatter opening delimiter has no closing --- delimiter")
    }
}
fn newline_for(source: &str) -> &'static str {
    source
        .find('\n')
        .map(|index| {
            if index > 0 && source.as_bytes()[index - 1] == b'\r' {
                "\r\n"
            } else {
                "\n"
            }
        })
        .unwrap_or("\n")
}
fn parse_mapping(yaml: &str) -> anyhow::Result<Mapping> {
    // Empty/comment-only frontmatter is an empty mapping. An explicit null,
    // scalar or sequence is not an empty header and must be rejected.
    if yaml
        .lines()
        .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
    {
        return Ok(Mapping::new());
    }
    let value: YamlValue = serde_yaml_ng::from_str(yaml)?;
    let YamlValue::Mapping(mapping) = value else {
        bail!("frontmatter must be a YAML mapping");
    };
    Ok(mapping)
}
fn merged(mapping: &Mapping) -> anyhow::Result<Mapping> {
    let mut value = YamlValue::Mapping(mapping.clone());
    value
        .apply_merge()
        .context("cannot safely evaluate frontmatter YAML merges")?;
    match value {
        YamlValue::Mapping(mapping) => Ok(mapping),
        _ => unreachable!(),
    }
}
fn edit_yaml(yaml: &str, field: &str, value: &JsonValue, newline: &str) -> anyhow::Result<String> {
    let json = serde_json::to_string(value)?;
    let key = serde_json::to_string(field)?;
    if yaml
        .lines()
        .all(|line| line.trim().is_empty() || line.trim_start().starts_with('#'))
    {
        let separator = if !yaml.is_empty() && !yaml.ends_with('\n') {
            newline
        } else {
            ""
        };
        return Ok(format!("{yaml}{separator}{key}: {json}{newline}"));
    }
    let file =
        YamlFile::from_str(yaml).context("frontmatter syntax cannot be edited losslessly")?;
    if file.to_string() != yaml {
        bail!("frontmatter syntax cannot be round-tripped losslessly");
    }
    let mapping = file
        .document()
        .and_then(|document| document.as_mapping())
        .context("frontmatter CST is not a mapping")?;
    // Inserting into a flow mapping only needs a separator and a new pair.
    // The CST's generic insertion may attach a comma to the preceding entry,
    // which would otherwise fail our unchanged-entry byte check.
    if mapping.is_flow_style() && mapping.find_entry_by_key(field).is_none() {
        let node = mapping
            .as_node()
            .context("flow mapping has no syntax node")?;
        let closing = node
            .children_with_tokens()
            .filter_map(|item| item.into_token())
            .filter(|token| token.text() == "}")
            .last()
            .context("flow mapping has no closing brace")?;
        let offset = u32::from(closing.text_range().start()) as usize;
        let trailing_comma = node
            .descendants_with_tokens()
            .filter_map(|item| item.into_token())
            .filter(|token| token.text_range().end() <= closing.text_range().start())
            .filter(|token| !token.text().trim().is_empty() && !token.text().starts_with('#'))
            .last()
            .is_some_and(|token| token.text() == ",");
        let comma = if mapping.entries().next().is_some() && !trailing_comma {
            ", "
        } else {
            ""
        };
        let mut updated = yaml.to_owned();
        updated.insert_str(offset, &format!("{comma}{key}: {json}"));
        return Ok(updated);
    }
    let untouched: Vec<_> = mapping
        .entries()
        .filter(|entry| !entry.key_matches(field))
        .map(|entry| entry.to_string())
        .collect();
    let had_terminator = mapping
        .find_entry_by_key(field)
        .is_some_and(|entry| entry.to_string().ends_with('\n'));
    let value_file = YamlFile::from_str(&json).context("JSON value cannot be converted to YAML")?;
    let value = value_file
        .document()
        .and_then(|document| document.as_node().cloned())
        .context("JSON value has no YAML node")?;
    // Document nodes themselves aren't values; retrieve their root value.
    let value = value
        .children()
        .find_map(yaml_edit::YamlNode::from_syntax)
        .context("JSON value has no YAML value node")?;
    mapping.set(field, value);
    let after_untouched: Vec<_> = mapping
        .entries()
        .filter(|entry| !entry.key_matches(field))
        .map(|entry| entry.to_string())
        .collect();
    if untouched != after_untouched {
        bail!("frontmatter editor would reformat other fields; no changes written");
    }
    let mut edited = file.to_string();
    if !mapping.is_flow_style()
        && had_terminator
        && let Some(entry) = mapping.find_entry_by_key(field)
        && !entry.to_string().ends_with('\n')
    {
        let text = entry.to_string();
        let node = mapping
            .as_node()
            .context("mapping has no syntax node")?
            .children()
            .find(|node| node.to_string() == text)
            .context("edited field has no syntax node")?;
        let end = u32::from(node.text_range().end()) as usize;
        if !edited[end..].starts_with(['\r', '\n']) {
            edited.insert_str(end, newline);
        }
    }
    // Apply only the changed byte span, normalizing newly generated line breaks
    // to this header's style without touching unchanged bytes or mixed endings.
    let prefix = yaml
        .chars()
        .zip(edited.chars())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    let suffix = yaml[prefix..]
        .chars()
        .rev()
        .zip(edited[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    let replacement = edited[prefix..edited.len() - suffix]
        .replace("\r\n", "\n")
        .replace('\n', newline);
    let mut updated = yaml.to_owned();
    updated.replace_range(prefix..yaml.len() - suffix, &replacement);
    if !updated.ends_with('\n') {
        updated.push_str(newline);
    }
    Ok(updated)
}
