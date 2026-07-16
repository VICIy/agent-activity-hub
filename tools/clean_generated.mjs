#!/usr/bin/env node
import {
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readlinkSync,
  readdirSync,
  rmSync,
  statSync,
  symlinkSync,
} from "node:fs";
import { homedir } from "node:os";
import { dirname, isAbsolute, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const OWNER = "work.effective.agent-activity-hub/v1";
const workspaceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const targetDirectory = join(workspaceRoot, "target");
const frontendDist = join(workspaceRoot, "apps", "agent-activity-desktop", "dist");
const releaseDirectory = join(targetDirectory, "release");
const debugDirectory = join(targetDirectory, "debug");
const debugHelper = join(debugDirectory, process.platform === "win32" ? "agent-activity-hook.exe" : "agent-activity-hook");
const removable = [
  frontendDist,
  debugDirectory,
  join(releaseDirectory, "build"),
  join(releaseDirectory, "deps"),
  join(releaseDirectory, ".fingerprint"),
  join(releaseDirectory, "incremental"),
  join(releaseDirectory, "examples"),
];

if (existsSync(targetDirectory)) {
  for (const entry of readdirSync(targetDirectory, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.name === "release" || entry.name === "sidecars") continue;
    if (entry.name.includes("-apple-") || entry.name.includes("-windows-") || entry.name.includes("-linux-")) {
      removable.push(join(targetDirectory, entry.name));
    }
  }
}

const protectedReferences = managedHookExecutables()
  .filter((value) => isAbsolute(value))
  .map((value) => resolve(value))
  .filter((value) => removable.some((directory) => isInside(value, directory)));

if (protectedReferences.length > 0) {
  console.error("Cleanup refused because managed Hooks still reference generated files:");
  for (const reference of [...new Set(protectedReferences)]) console.error(`- ${reference}`);
  console.error("Repair those adapters from the production Tauri application, then retry.");
  process.exit(2);
}

const existing = removable.filter(existsSync);
const compatibilityLink = readCompatibilityLink();
if (process.argv.includes("--dry-run")) {
  if (existing.length === 0) console.log("No removable generated directories found.");
  else for (const directory of existing) {
    if (directory === debugDirectory && compatibilityLink) {
      console.log(`Would clean and preserve the active-session Helper link: ${directory}`);
    } else {
      console.log(`Would remove: ${directory}`);
    }
  }
  process.exit(0);
}

for (const directory of existing) {
  rmSync(directory, { recursive: true, force: true });
  if (directory === debugDirectory && compatibilityLink) {
    mkdirSync(debugDirectory, { recursive: true });
    symlinkSync(compatibilityLink, debugHelper);
    console.log(`Cleaned and preserved the active-session Helper link: ${directory}`);
  } else {
    console.log(`Removed: ${directory}`);
  }
}
for (const file of [
  ".cargo-artifact-lock",
  ".cargo-build-lock",
  ".cargo-lock",
  "agent-activity.d",
  "agent-activity-hook.d",
]) {
  rmSync(join(releaseDirectory, file), { force: true });
}
if (existing.length === 0) console.log("Generated build caches are already clean.");

function managedHookExecutables() {
  const configurations = [
    join(homedir(), ".codex", "hooks.json"),
    join(homedir(), ".claude", "settings.json"),
    join(homedir(), ".qoder", "settings.json"),
  ];
  const commands = [];
  for (const path of configurations) {
    if (!existsSync(path) || !statSync(path).isFile()) continue;
    let config;
    try {
      config = JSON.parse(readFileSync(path, "utf8"));
    } catch {
      continue;
    }
    const hooks = config?.hooks;
    if (!hooks || Array.isArray(hooks) || typeof hooks !== "object") continue;
    for (const entries of Object.values(hooks)) {
      if (!Array.isArray(entries)) continue;
      for (const entry of entries) {
        if (entry?.["x-agent-activity-owner"] !== OWNER || !Array.isArray(entry.hooks)) continue;
        for (const hook of entry.hooks) {
          if (typeof hook?.command === "string") commands.push(commandExecutable(hook.command));
        }
      }
    }
  }
  return commands.filter(Boolean);
}

function readCompatibilityLink() {
  if (!existsSync(debugHelper) || !lstatSync(debugHelper).isSymbolicLink()) return null;
  const target = readlinkSync(debugHelper);
  return existsSync(resolve(dirname(debugHelper), target)) ? target : null;
}

function commandExecutable(command) {
  const trimmed = command.trim();
  if (trimmed.startsWith('"')) return trimmed.slice(1).split('"', 1)[0];
  if (trimmed.startsWith("'")) return trimmed.slice(1).split("'", 1)[0];
  return trimmed.split(/\s+/, 1)[0];
}

function isInside(path, directory) {
  const value = relative(resolve(directory), path);
  return value === "" || (!value.startsWith("..") && !isAbsolute(value));
}
