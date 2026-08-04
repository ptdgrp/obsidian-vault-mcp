use tracing_subscriber::{EnvFilter, fmt};

pub(crate) fn init(level: &str) -> anyhow::Result<()> {
    fmt()
        .with_env_filter(log_filter(level)?)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .with_target(true)
        .try_init()
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    Ok(())
}

pub(crate) fn input_preview(value: &impl std::fmt::Debug) -> (String, bool) {
    const MAX_INPUT_BYTES: usize = 1024;
    const TRUNCATION_MARKER: &str = "…";

    let mut preview = format!("{value:?}");
    if preview.len() <= MAX_INPUT_BYTES {
        return (preview, false);
    }

    let mut cutoff = MAX_INPUT_BYTES - TRUNCATION_MARKER.len();
    while !preview.is_char_boundary(cutoff) {
        cutoff -= 1;
    }
    preview.truncate(cutoff);
    preview.push_str(TRUNCATION_MARKER);
    (preview, true)
}

fn log_filter(input: &str) -> anyhow::Result<EnvFilter> {
    if input.contains('=') || input.contains(',') {
        return Ok(EnvFilter::try_new(input)?);
    }
    Ok(EnvFilter::try_new(format!(
        "obsidian_vault_mcp={input},warn"
    ))?)
}

#[cfg(test)]
mod tests {
    use super::input_preview;

    #[test]
    fn truncates_preview_at_a_utf8_boundary() {
        let input = "界".repeat(400);
        let (preview, truncated) = input_preview(&input);
        assert!(truncated);
        assert!(preview.ends_with('…'));
        assert!(preview.len() <= 1024);
    }
}
