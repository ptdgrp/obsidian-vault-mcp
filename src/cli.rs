pub(crate) mod commands;

use crate::vault::DEFAULT_MAX_READ_NOTE_CHARS;
use camino::Utf8PathBuf;

#[derive(Debug, clap::Parser)]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Vault root directory
    #[arg(long, env = "OBSIDIAN_VAULT_MCP_ROOT")]
    pub(crate) vault: Option<Utf8PathBuf>,

    /// Optional config file
    #[arg(long, env = "OBSIDIAN_VAULT_MCP_CONFIG")]
    pub(crate) config: Option<Utf8PathBuf>,

    /// Include glob patterns
    #[arg(long)]
    pub(crate) include: Vec<String>,

    /// Exclude glob patterns
    #[arg(long)]
    pub(crate) exclude: Vec<String>,

    /// Follow symlinks inside vault
    #[arg(long, default_value_t = false)]
    pub(crate) follow_symlinks: bool,

    /// Max Unicode characters returned by read_note
    #[arg(long, default_value_t = DEFAULT_MAX_READ_NOTE_CHARS)]
    pub(crate) max_read_note_chars: usize,

    /// Max search results
    #[arg(long, default_value_t = 50)]
    pub(crate) max_results: usize,

    /// Seconds before an unused parsed Markdown cache entry expires
    #[arg(long, default_value_t = 600)]
    pub(crate) parse_cache_ttl_secs: u64,

    /// Max parsed Markdown cache entries kept in memory
    #[arg(long, default_value_t = 1024)]
    pub(crate) parse_cache_max_entries: usize,

    /// Log level for stderr.
    #[arg(long, default_value = "debug")]
    pub(crate) log_level: String,

    #[command(subcommand)]
    command: Option<commands::Command>,
}

impl Cli {
    pub(crate) fn command_ref(&self) -> Option<&commands::Command> {
        self.command.as_ref()
    }

    pub(crate) fn command(self) -> commands::Command {
        self.command.unwrap_or(commands::Command::Serve)
    }
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;

    #[test]
    fn edit_commands_reject_line_selectors_while_read_note_accepts_them() {
        for command in ["append-section", "replace-section", "delete-section"] {
            let mut args = vec!["obsidian-vault-mcp", command, "note.md", "--line", "#L1"];
            if command != "delete-section" {
                args.push("content");
            }
            assert!(Cli::try_parse_from(args).is_err(), "{command}");
        }

        assert!(
            Cli::try_parse_from([
                "obsidian-vault-mcp",
                "read-note",
                "note.md",
                "--line",
                "#L1"
            ])
            .is_ok()
        );
    }

    #[test]
    fn install_ocr_models_requires_a_target_directory() {
        assert!(Cli::try_parse_from(["obsidian-vault-mcp", "install-ocr-models"]).is_err());
        assert!(
            Cli::try_parse_from([
                "obsidian-vault-mcp",
                "install-ocr-models",
                "--model-dir",
                "./models",
            ])
            .is_ok()
        );
    }
}
