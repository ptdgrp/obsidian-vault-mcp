#!/usr/bin/env node

const { spawn } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const binaries = {
  "linux-x64": "obsidian-vault-mcp",
  "linux-arm64": "obsidian-vault-mcp",
  "darwin-x64": "obsidian-vault-mcp",
  "darwin-arm64": "obsidian-vault-mcp",
  "win32-x64": "obsidian-vault-mcp.exe",
};

const platform = `${process.platform}-${process.arch}`;
const name = binaries[platform];
if (!name) {
  console.error(`obsidian-vault-mcp: unsupported platform ${platform}`);
  process.exit(1);
}

const binary = path.join(__dirname, "..", "vendor", platform, name);
if (!fs.existsSync(binary)) {
  console.error(`obsidian-vault-mcp: missing binary for ${platform}; reinstall the package`);
  process.exit(1);
}

const child = spawn(binary, process.argv.slice(2), { stdio: "inherit", windowsHide: true });
child.on("error", (error) => {
  console.error(`obsidian-vault-mcp: ${error.message}`);
  process.exitCode = 1;
});
child.on("close", (code, signal) => {
  process.exitCode = code ?? (signal ? 128 : 1);
});
for (const signal of ["SIGINT", "SIGTERM"]) {
  process.on(signal, () => child.kill(signal));
}
