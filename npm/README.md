# @ptdgrp/obsidian-vault-mcp

Prebuilt [obsidian-vault-mcp](https://github.com/ptdgrp/obsidian-vault-mcp) CLI and MCP server.

```sh
npm install -g @ptdgrp/obsidian-vault-mcp
obsidian-vault-mcp serve
```

Or run it directly from an MCP client:

```json
{
  "mcpServers": {
    "obsidian-vault": {
      "command": "npx",
      "args": ["-y", "@ptdgrp/obsidian-vault-mcp", "serve"]
    }
  }
}
```

Use `--vault /absolute/path/to/vault` before `serve` when the client does not launch in the vault directory. The package supports Linux x64/arm64, macOS arm64, and Windows x64. npm installs only the matching optional platform package. Node.js is needed only to launch its Rust binary.

The npm build contains the default Markdown feature set. See the [project README](https://github.com/ptdgrp/obsidian-vault-mcp#readme) for CLI options and source builds with attachment support.
