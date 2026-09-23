import { execFileSync } from "node:child_process";
import { chmodSync, copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";

const assetsDir = path.resolve(process.argv[2] ?? "release");
const rootPackage = JSON.parse(readFileSync("npm/package.json", "utf8"));
const platforms = [
  { id: "linux-x64", os: "linux", cpu: "x64", binary: "obsidian-vault-mcp" },
  { id: "linux-arm64", os: "linux", cpu: "arm64", binary: "obsidian-vault-mcp" },
  { id: "darwin-arm64", os: "darwin", cpu: "arm64", binary: "obsidian-vault-mcp" },
  { id: "win32-x64", os: "win32", cpu: "x64", binary: "obsidian-vault-mcp.exe" },
];

mkdirSync(assetsDir, { recursive: true });
for (const platform of platforms) {
  const name = `@ptdgrp/obsidian-vault-mcp-${platform.id}`;
  if (rootPackage.optionalDependencies?.[name] !== rootPackage.version) {
    throw new Error(`Missing matching optional dependency: ${name}`);
  }

  const packageDir = path.resolve("npm/platforms", platform.id);
  const binDir = path.join(packageDir, "bin");
  mkdirSync(binDir, { recursive: true });
  execFileSync("tar", [
    "-xzf",
    path.join(assetsDir, `obsidian-vault-mcp-${platform.id}.tar.gz`),
    "-C",
    binDir,
  ], { stdio: "inherit" });
  if (platform.os !== "win32") chmodSync(path.join(binDir, platform.binary), 0o755);
  copyFileSync("LICENSE", path.join(packageDir, "LICENSE"));
  writeFileSync(path.join(packageDir, "package.json"), JSON.stringify({
    name,
    version: rootPackage.version,
    description: `Prebuilt obsidian-vault-mcp binary for ${platform.id}`,
    repository: rootPackage.repository,
    license: rootPackage.license,
    os: [platform.os],
    cpu: [platform.cpu],
    files: ["bin/"],
    publishConfig: { access: "public" },
  }, null, 2) + "\n");
  execFileSync("npm", ["pack", packageDir, "--pack-destination", assetsDir], { stdio: "inherit" });
}

copyFileSync("LICENSE", "npm/LICENSE");
execFileSync("npm", ["pack", "./npm", "--pack-destination", assetsDir], { stdio: "inherit" });
