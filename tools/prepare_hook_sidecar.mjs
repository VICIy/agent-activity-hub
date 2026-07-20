#!/usr/bin/env node
import { copyFileSync, chmodSync, existsSync, mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const toolsDirectory = dirname(fileURLToPath(import.meta.url));
const workspaceRoot = resolve(toolsDirectory, "..");
const frontendDirectory = join(workspaceRoot, "apps", "agent-activity-desktop");
const typescriptScript = join(frontendDirectory, "node_modules", "typescript", "bin", "tsc");
const viteScript = join(frontendDirectory, "node_modules", "vite", "bin", "vite.js");
const args = process.argv.slice(2);
const hostTarget = rustHostTriple();
const target = argumentValue(args, "--target")
  ?? process.env.AGENT_ACTIVITY_TARGET_TRIPLE
  ?? process.env.TAURI_ENV_TARGET_TRIPLE
  ?? process.env.CARGO_BUILD_TARGET
  ?? hostTarget;
const executableSuffix = target.includes("windows") ? ".exe" : "";
const targetDirectory = join(workspaceRoot, "target");

run("cargo", ["build", "--release", "-p", "activity-hook", "--target", target], {
  cwd: workspaceRoot,
  env: { ...process.env, CARGO_TARGET_DIR: targetDirectory },
});

const source = join(targetDirectory, target, "release", `agent-activity-hook${executableSuffix}`);
const sidecarDirectory = join(targetDirectory, "sidecars");
const destination = join(sidecarDirectory, `agent-activity-hook-${target}${executableSuffix}`);
if (!existsSync(source)) throw new Error(`Hook Helper build output is missing: ${source}`);
mkdirSync(sidecarDirectory, { recursive: true });
copyFileSync(source, destination);
if (!executableSuffix) chmodSync(destination, 0o755);
console.log(`Prepared Hook Helper sidecar: ${destination}`);
if (target === hostTarget) {
  const developmentCopy = join(targetDirectory, "release", `agent-activity-hook${executableSuffix}`);
  mkdirSync(join(targetDirectory, "release"), { recursive: true });
  copyFileSync(source, developmentCopy);
  if (!executableSuffix) chmodSync(developmentCopy, 0o755);
}

if (args.includes("--with-frontend")) {
  run(process.execPath, [typescriptScript], {
    cwd: frontendDirectory,
    env: process.env,
  });
  run(process.execPath, [viteScript, "build"], {
    cwd: frontendDirectory,
    env: process.env,
  });
}

function argumentValue(items, flag) {
  const index = items.indexOf(flag);
  return index >= 0 ? items[index + 1] : undefined;
}

function rustHostTriple() {
  const result = spawnSync("rustc", ["-vV"], { encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(result.stderr || "rustc -vV failed");
  const host = result.stdout.split(/\r?\n/).find((line) => line.startsWith("host: "))?.slice(6).trim();
  if (!host) throw new Error("Unable to determine the Rust host target triple");
  return host;
}

function run(command, commandArgs, options) {
  const result = spawnSync(command, commandArgs, { ...options, stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
