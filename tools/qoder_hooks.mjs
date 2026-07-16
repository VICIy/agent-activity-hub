#!/usr/bin/env node
import { closeSync, copyFileSync, existsSync, fsyncSync, mkdirSync, openSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join, resolve } from "node:path";

const OWNER = "work.effective.agent-activity-hub/v1";
const EVENTS = [
  "SessionStart",
  "UserPromptSubmit",
  "PreToolUse",
  "PermissionRequest",
  "Notification",
  "PostToolUse",
  "PostToolUseFailure",
  "Stop",
  "StopFailure",
  "PreCompact",
  "PostCompact",
  "SubagentStart",
  "SubagentStop",
  "SessionEnd",
];

const [command = "doctor", ...args] = process.argv.slice(2);
const provider = value(args, "--provider") ?? "qoder";
if (!/^[a-z0-9][a-z0-9._-]*$/.test(provider)) {
  throw new Error(`Invalid provider: ${provider}`);
}
const defaultDirectory = provider === "claude" ? ".claude" : ".qoder";
const configPath = resolve(value(args, "--config") ?? join(homedir(), defaultDirectory, "settings.json"));
const helperPath = resolve(value(args, "--helper") ?? "agent-activity-hook");
const shouldApply = args.includes("--apply");

try {
  if (command === "doctor") doctor(configPath, provider);
  else if (command === "install") install(configPath, helperPath, provider, shouldApply);
  else if (command === "uninstall") uninstall(configPath, provider, shouldApply);
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

function ownedEntry(helper, event, provider) {
  const hook = provider === "qoder"
    ? {
        type: "command",
        command: helper,
        args: ["--provider", provider, "--event", event],
        name: "agent-activity",
        async: true,
        timeout: 2,
      }
    : {
        type: "command",
        command: `"${helper}" --provider ${provider}`,
        timeout: 5,
      };
  return {
    matcher: ".*",
    hooks: [hook],
    "x-agent-activity-owner": OWNER,
    "x-agent-activity-event": event,
  };
}

function withHooks(config, helper, provider) {
  const next = structuredClone(config);
  next.hooks ??= {};
  if (!next.hooks || Array.isArray(next.hooks) || typeof next.hooks !== "object") {
    throw new Error("The existing hooks field is not an object; no changes were made.");
  }
  if (provider === "qoder") {
    for (const event of Object.keys(next.hooks)) {
      if (!Array.isArray(next.hooks[event])) continue;
      next.hooks[event] = next.hooks[event].filter((entry) => !isLegacyQoderEntry(entry));
      if (next.hooks[event].length === 0) delete next.hooks[event];
    }
  }
  for (const event of EVENTS) {
    const entries = Array.isArray(next.hooks[event]) ? next.hooks[event] : [];
    next.hooks[event] = [
      ...entries.filter((entry) => entry?.["x-agent-activity-owner"] !== OWNER),
      ownedEntry(helper, event, provider),
    ];
  }
  return next;
}

function withoutHooks(config, provider) {
  const next = structuredClone(config);
  if (!next.hooks || Array.isArray(next.hooks) || typeof next.hooks !== "object") return next;
  for (const event of Object.keys(next.hooks)) {
    if (!Array.isArray(next.hooks[event])) continue;
    next.hooks[event] = next.hooks[event].filter(
      (entry) => entry?.["x-agent-activity-owner"] !== OWNER
        && !(provider === "qoder" && isLegacyQoderEntry(entry)),
    );
    if (next.hooks[event].length === 0) delete next.hooks[event];
  }
  if (Object.keys(next.hooks).length === 0) delete next.hooks;
  return next;
}

function install(path, helper, provider, apply) {
  const current = load(path);
  const next = withHooks(current, helper, provider);
  preview(path, current, next);
  if (apply) atomicWrite(path, next);
  else console.log("Preview only. Re-run with --apply to write the configuration.");
}

function uninstall(path, provider, apply) {
  const current = load(path);
  const next = withoutHooks(current, provider);
  preview(path, current, next);
  if (apply) atomicWrite(path, next);
  else console.log("Preview only. Re-run with --apply to remove owned entries.");
}

function doctor(path, provider) {
  const config = load(path);
  const installed = EVENTS.filter((event) =>
    Array.isArray(config.hooks?.[event])
      && config.hooks[event].some((entry) => entry?.["x-agent-activity-owner"] === OWNER),
  );
  const missing = EVENTS.filter((event) => !installed.includes(event));
  console.log(JSON.stringify({ config: path, provider, owner: OWNER, installed, missing, healthy: missing.length === 0 }, null, 2));
  if (missing.length) process.exitCode = 2;
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

function isLegacyQoderEntry(entry) {
  return Array.isArray(entry?.hooks) && entry.hooks.some((hook) =>
    hook?.name === "flash4-light"
      || (typeof hook?.command === "string" && hook.command.endsWith("/flash4-light.sh")),
  );
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
  const descriptor = openSync(temporary, "r");
  fsyncSync(descriptor);
  closeSync(descriptor);
  renameSync(temporary, path);
  console.log(`Updated: ${path}`);
}
