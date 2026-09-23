#!/usr/bin/env node

const { spawn } = require("node:child_process");

const binaries = {
  "linux-x64": "obsidian-vault-mcp",
  "linux-arm64": "obsidian-vault-mcp",
  "darwin-arm64": "obsidian-vault-mcp",
  "win32-x64": "obsidian-vault-mcp.exe",
};

const platform = `${process.platform}-${process.arch}`;
const name = binaries[platform];
if (!name) {
  console.error(`obsidian-vault-mcp: unsupported platform ${platform}`);
  process.exit(1);
}

let binary;
try {
  binary = require.resolve(`@ptdgrp/obsidian-vault-mcp-${platform}/bin/${name}`);
} catch (error) {
  if (error.code !== "MODULE_NOT_FOUND") throw error;
  console.error(`obsidian-vault-mcp: missing binary package for ${platform}; reinstall with optional dependencies enabled`);
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
