use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// External, body-shaped content that is isolated from Blueprint protocol Markdown.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
pub struct ExternalBody {
    pub lines: Vec<String>,
}

impl ExternalBody {
    pub(crate) fn from_text(text: &str) -> Self {
        Self {
            lines: text.split('\n').map(ToOwned::to_owned).collect(),
        }
    }

    pub(crate) fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn render(&self) -> anyhow::Result<String> {
        self.render_with_required(true)
    }

    pub(crate) fn render_optional(&self) -> anyhow::Result<String> {
        self.render_with_required(false)
    }

    fn render_with_required(&self, required: bool) -> anyhow::Result<String> {
        if required && !self.lines.iter().any(|line| !line.trim().is_empty()) {
            anyhow::bail!("external body must contain non-whitespace content");
        }
        let longest = self
            .lines
            .iter()
            .map(|line| {
                line.trim_start()
                    .chars()
                    .take_while(|character| *character == '~')
                    .count()
            })
            .max()
            .unwrap_or(0);
        let fence = "~".repeat(longest.max(2) + 1);
        Ok(format!("{fence}\n{}\n{fence}", self.lines.join("\n")))
    }

    pub fn parse(field: &str, source: &str) -> anyhow::Result<Self> {
        let (body, remainder) = Self::parse_leading(field, source, true)?;
        if !remainder.trim().is_empty() {
            anyhow::bail!("{field} contains protocol content after its external body");
        }
        Ok(body)
    }

    pub(crate) fn parse_optional(field: &str, source: &str) -> anyhow::Result<Self> {
        let (body, remainder) = Self::parse_leading(field, source, false)?;
        if !remainder.trim().is_empty() {
            anyhow::bail!("{field} contains protocol content after its external body");
        }
        Ok(body)
    }

    pub(crate) fn parse_leading<'a>(
        field: &str,
        source: &'a str,
        required: bool,
    ) -> anyhow::Result<(Self, &'a str)> {
        let lines = source.split('\n').collect::<Vec<_>>();
        let Some(opening_raw) = lines.first().copied() else {
            anyhow::bail!("{field} must contain one tilde-fenced external body");
        };
        let opening = opening_raw.trim_end_matches('\r');
        if opening.len() < 3 || !opening.bytes().all(|byte| byte == b'~') {
            anyhow::bail!("{field} must contain one tilde-fenced external body");
        }
        let Some(closing) = lines
            .iter()
            .skip(1)
            .position(|line| line.trim_end_matches('\r') == opening)
        else {
            anyhow::bail!("{field} external body fence is not closed");
        };
        let closing = closing + 1;
        let body = Self {
            lines: lines[1..closing]
                .iter()
                .map(|line| line.trim_end_matches('\r').to_string())
                .collect(),
        };
        if required && !body.lines.iter().any(|line| !line.trim().is_empty()) {
            anyhow::bail!("{field} external body must contain non-whitespace content");
        }
        let mut offset = lines[..=closing]
            .iter()
            .map(|line| line.len())
            .sum::<usize>()
            + closing;
        if source.as_bytes().get(offset) == Some(&b'\n') {
            offset += 1;
        }
        Ok((body, &source[offset..]))
    }
}
