#!/usr/bin/env node
import { spawn, spawnSync } from "node:child_process";
import { createConnection, createServer } from "node:net";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const workspaceRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const frontendDirectory = join(workspaceRoot, "apps", "agent-activity-desktop");
const tauriBinary = join(
  frontendDirectory,
  "node_modules",
  ".bin",
  process.platform === "win32" ? "tauri.cmd" : "tauri",
);
const tauriScript = join(frontendDirectory, "node_modules", "@tauri-apps", "cli", "tauri.js");
const viteScript = join(frontendDirectory, "node_modules", "vite", "bin", "vite.js");
const args = process.argv.slice(2);
const command = args[0];
const target = argumentValue(args, "--target");
const childEnvironment = {
  ...process.env,
  ...(target ? { AGENT_ACTIVITY_TARGET_TRIPLE: target } : {}),
};

if (command !== "dev") {
  const result = spawnSync(
    process.platform === "win32" ? process.execPath : tauriBinary,
    process.platform === "win32" ? [tauriScript, ...args] : args,
    {
      cwd: frontendDirectory,
      env: childEnvironment,
      stdio: "inherit",
    },
  );
  if (result.error) throw result.error;
  process.exit(result.status ?? 1);
}

const sidecarArgs = [join(workspaceRoot, "tools", "prepare_hook_sidecar.mjs")];
if (target) sidecarArgs.push("--target", target);
const sidecar = spawnSync(process.execPath, sidecarArgs, {
  cwd: workspaceRoot,
  env: childEnvironment,
  stdio: "inherit",
});
if (sidecar.error) throw sidecar.error;
if (sidecar.status !== 0) process.exit(sidecar.status ?? 1);

const host = "127.0.0.1";
const port = await findAvailablePort(host, 1420);
const devUrl = `http://${host}:${port}`;
console.log(`Starting Tauri development frontend at ${devUrl}`);

const vite = spawn(
  process.platform === "win32" ? process.execPath : "npm",
  process.platform === "win32" ? [viteScript] : ["run", "dev"],
  {
    cwd: frontendDirectory,
    env: { ...childEnvironment, AGENT_ACTIVITY_DEV_PORT: String(port) },
    stdio: "inherit",
  },
);
let viteExit = null;
vite.once("exit", (code) => { viteExit = code ?? 1; });

try {
  await waitForServer(host, port, () => viteExit);
} catch (error) {
  stop(vite);
  throw error;
}

const override = JSON.stringify({ build: { beforeDevCommand: "", devUrl } });
const tauri = spawn(
  process.platform === "win32" ? process.execPath : tauriBinary,
  process.platform === "win32"
    ? [tauriScript, "dev", "--config", override, ...args.slice(1)]
    : ["dev", "--config", override, ...args.slice(1)],
  {
    cwd: frontendDirectory,
    env: childEnvironment,
    stdio: "inherit",
  },
);

let stopping = false;
function stopChildren(signal = "SIGTERM") {
  if (stopping) return;
  stopping = true;
  stop(tauri, signal);
  stop(vite, signal);
}
for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
  process.on(signal, () => stopChildren(signal));
}

const exitCode = await new Promise((resolveExit) => {
  tauri.once("error", (error) => {
    console.error(error);
    resolveExit(1);
  });
  tauri.once("exit", (code, signal) => resolveExit(code ?? (signal ? 1 : 0)));
});
stop(vite);
await waitForExit(vite, 2_000);
process.exitCode = exitCode;

function argumentValue(items, flag) {
  const index = items.indexOf(flag);
  return index >= 0 ? items[index + 1] : undefined;
}

async function findAvailablePort(hostname, firstPort) {
  for (let portNumber = firstPort; portNumber < firstPort + 100; portNumber += 1) {
    if (await canListen(hostname, portNumber)) return portNumber;
  }
  throw new Error(`No development port is available from ${firstPort} to ${firstPort + 99}`);
}

function canListen(hostname, portNumber) {
  return new Promise((resolvePort) => {
    const server = createServer();
    server.unref();
    server.once("error", () => resolvePort(false));
    server.listen({ host: hostname, port: portNumber, exclusive: true }, () => {
      server.close(() => resolvePort(true));
    });
  });
}

async function waitForServer(hostname, portNumber, exitStatus) {
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    if (exitStatus() !== null) throw new Error(`Vite exited before opening port ${portNumber}`);
    if (await canConnect(hostname, portNumber)) return;
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  }
  throw new Error(`Timed out waiting for Vite on ${hostname}:${portNumber}`);
}

function canConnect(hostname, portNumber) {
  return new Promise((resolveConnection) => {
    const socket = createConnection({ host: hostname, port: portNumber });
    socket.setTimeout(250);
    socket.once("connect", () => {
      socket.destroy();
      resolveConnection(true);
    });
    const fail = () => {
      socket.destroy();
      resolveConnection(false);
    };
    socket.once("error", fail);
    socket.once("timeout", fail);
  });
}

function stop(child, signal = "SIGTERM") {
  if (child && child.exitCode === null && !child.killed) child.kill(signal);
}

function waitForExit(child, timeout) {
  if (!child || child.exitCode !== null) return Promise.resolve();
  return Promise.race([
    new Promise((resolveExit) => child.once("exit", resolveExit)),
    new Promise((resolveWait) => setTimeout(resolveWait, timeout)),
  ]);
}
