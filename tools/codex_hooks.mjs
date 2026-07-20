#!/usr/bin/env node
import { closeSync, copyFileSync, existsSync, fsyncSync, mkdirSync, openSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";

const OWNER = "work.effective.agent-activity-hub/v1";
const EVENTS = [
  "SessionStart",
  "UserPromptSubmit",
  "PreToolUse",
  "PermissionRequest",
  "PostToolUse",
  "PostToolUseFailure",
  "Stop",
  "SessionEnd",
];

const [command = "doctor", ...args] = process.argv.slice(2);
const configPath = resolve(value(args, "--config") ?? join(homedir(), ".codex", "hooks.json"));
const helperPath = resolve(value(args, "--helper") ?? "agent-activity-hook");
const shouldApply = args.includes("--apply");

try {
  if (command === "doctor") doctor(configPath);
  else if (command === "install") install(configPath, helperPath, shouldApply);
  else if (command === "uninstall") uninstall(configPath, shouldApply);
  else throw new Error(`Unknown command: ${command}`);
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exitCode = 1;
}

function value(items, flag) {
  const index = items.indexOf(flag);
  return index >= 0 ? items[index + 1] : undefined;
}

function load(path) {
  if (!existsSync(path)) return {};
  const parsed = JSON.parse(readFileSync(path, "utf8"));
  if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new Error(`Expected a JSON object in ${path}`);
  }
  return parsed;
}

function ownedEntry(helper, event) {
  return {
    hooks: [{
      type: "command",
      command: `${quoteCommandPath(helper)} --provider codex --event ${event}`,
      timeout: 2,
    }],
    "x-agent-activity-owner": OWNER,
    "x-agent-activity-event": event,
  };
}

function quoteCommandPath(path) {
  if (process.platform === "win32") return `& '${path.replaceAll("'", "''")}'`;
  return `"${path}"`;
}

function withHooks(config, helper) {
  const next = structuredClone(config);
  next.hooks ??= {};
  if (!next.hooks || Array.isArray(next.hooks) || typeof next.hooks !== "object") {
    throw new Error("The existing hooks field is not an object; no changes were made.");
  }
  for (const event of EVENTS) {
    const entries = Array.isArray(next.hooks[event]) ? next.hooks[event] : [];
    next.hooks[event] = [
      ...entries.filter((entry) => entry?.["x-agent-activity-owner"] !== OWNER),
      ownedEntry(helper, event),
    ];
  }
  return next;
}

function withoutHooks(config) {
  const next = structuredClone(config);
  if (!next.hooks || Array.isArray(next.hooks) || typeof next.hooks !== "object") return next;
  for (const event of Object.keys(next.hooks)) {
    if (!Array.isArray(next.hooks[event])) continue;
    next.hooks[event] = next.hooks[event].filter(
      (entry) => entry?.["x-agent-activity-owner"] !== OWNER,
    );
    if (next.hooks[event].length === 0) delete next.hooks[event];
  }
  if (Object.keys(next.hooks).length === 0) delete next.hooks;
  return next;
}

function install(path, helper, apply) {
  const current = load(path);
  const next = withHooks(current, helper);
  preview(path, current, next);
  if (apply) atomicWrite(path, next);
  else console.log("Preview only. Re-run with --apply to write the configuration.");
}

function uninstall(path, apply) {
  const current = load(path);
  const next = withoutHooks(current);
  preview(path, current, next);
  if (apply) atomicWrite(path, next);
  else console.log("Preview only. Re-run with --apply to remove owned entries.");
}

function doctor(path) {
  const config = load(path);
  const installed = EVENTS.filter((event) =>
    Array.isArray(config.hooks?.[event])
      && config.hooks[event].some((entry) =>
        entry?.["x-agent-activity-owner"] === OWNER
        && entry.hooks?.some((hook) =>
          typeof hook?.command === "string" && commandIsUsable(hook.command, event),
        ),
      ),
  );
  const missing = EVENTS.filter((event) => !installed.includes(event));
  console.log(JSON.stringify({ config: path, owner: OWNER, installed, missing, healthy: missing.length === 0 }, null, 2));
  if (missing.length) process.exitCode = 2;
}

function commandIsUsable(command, event) {
  if (!command.includes(`--event ${event}`)) return false;
  const trimmed = command.trim().replace(/^&\s*/, "");
  const executable = trimmed.startsWith('"')
    ? trimmed.slice(1).split('"')[0]
    : trimmed.startsWith("'")
      ? trimmed.slice(1).split("'")[0]
      : trimmed.split(/\s+/, 1)[0];
  if (!executable) return false;
  if (executable.includes("/") || executable.includes("\\")) {
    return existsSync(executable);
  }
  return (process.env.PATH ?? "")
    .split(delimiter)
    .filter(Boolean)
    .some((directory) =>
      existsSync(join(directory, executable))
      || (process.platform === "win32" && existsSync(join(directory, `${executable}.exe`))),
    );
}

function preview(path, current, next) {
  const currentOwned = countOwned(current);
  const nextOwned = countOwned(next);
  console.log(JSON.stringify({ config: path, ownedEntries: { before: currentOwned, after: nextOwned }, preservedTopLevelKeys: Object.keys(current).filter((key) => key !== "hooks") }, null, 2));
}

function countOwned(config) {
  if (!config.hooks || typeof config.hooks !== "object") return 0;
  return Object.values(config.hooks).flatMap((entries) => Array.isArray(entries) ? entries : [])
    .filter((entry) => entry?.["x-agent-activity-owner"] === OWNER).length;
}

function atomicWrite(path, config) {
  mkdirSync(dirname(path), { recursive: true });
  if (existsSync(path)) {
    const backup = `${path}.agent-activity.${new Date().toISOString().replaceAll(":", "-")}.bak`;
    copyFileSync(path, backup);
    console.log(`Backup: ${backup}`);
  }
  const temporary = `${path}.agent-activity.tmp`;
  writeFileSync(temporary, `${JSON.stringify(config, null, 2)}\n`, { mode: 0o600 });
  const descriptor = openSync(temporary, "r+");
  fsyncSync(descriptor);
  closeSync(descriptor);
  renameSync(temporary, path);
  console.log(`Updated: ${path}`);
}
