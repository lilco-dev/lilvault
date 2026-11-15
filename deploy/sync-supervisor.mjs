import { execFile, spawn } from "node:child_process";
import { mkdir, readdir, rename, unlink, writeFile } from "node:fs/promises";
import process from "node:process";
import { promisify } from "node:util";

const exec = promisify(execFile);
const ob = process.argv[2];
const registry = process.env.LILVAULT_VAULT_REGISTRY ?? "/registry";
const children = new Map();
let stopping = false;
let reconciling = false;
let announced = false;

if (!ob) throw new Error("usage: sync-supervisor.mjs /path/to/ob");

async function registrations() {
  const { stdout } = await exec(ob, ["sync-list-local", "--json"], {
    maxBuffer: 1024 * 1024,
  });
  const parsed = JSON.parse(stdout);
  if (!Array.isArray(parsed.vaults)) throw new Error("invalid sync-list-local response");
  const result = new Map();
  for (const vault of parsed.vaults) {
    if (typeof vault.path !== "string") continue;
    const match = /^\/vaults\/([A-Za-z0-9_-]{1,64})$/.exec(vault.path);
    if (!match) {
      console.error(`Ignoring registration outside /vaults/<id>: ${vault.path}`);
      continue;
    }
    result.set(match[1], vault.path);
  }
  return result;
}

async function markRegistered(id) {
  await mkdir(registry, { recursive: true, mode: 0o700 });
  const temporary = `${registry}/.${id}.${process.pid}.tmp`;
  await writeFile(temporary, `${id}\n`, { mode: 0o600 });
  await rename(temporary, `${registry}/${id}`);
}

function start(id, path, state = { failures: 0, next: 0 }) {
  if (stopping || Date.now() < state.next) return;
  console.error(`Starting continuous sync for ${id} at ${path}`);
  const started = Date.now();
  const child = spawn(ob, ["sync", "--path", path, "--continuous"], {
    stdio: "inherit",
  });
  state.child = child;
  children.set(id, state);
  child.on("error", (error) => console.error(`Could not start sync for ${id}: ${error.message}`));
  child.on("close", (code, signal) => {
    delete state.child;
    if (stopping || state.removed) return;
    const stable = Date.now() - started >= 60_000;
    state.failures = stable ? 1 : Math.min(state.failures + 1, 6);
    const delay = Math.min(60, 2 ** state.failures);
    state.next = Date.now() + delay * 1000;
    console.error(`Sync for ${id} exited (${signal ?? code}); retrying in ${delay}s`);
  });
}

async function reconcile() {
  if (stopping || reconciling) return;
  reconciling = true;
  try {
    const desired = await registrations();
    if (desired.size === 0 && !announced) {
      console.error("Waiting for setup. In this container run: lilvault-add-vault");
      announced = true;
    }
    for (const [id, path] of desired) {
      await markRegistered(id);
      const state = children.get(id);
      if (!state?.child) start(id, path, state);
    }
    for (const [id, state] of children) {
      if (!desired.has(id)) {
        state.removed = true;
        if (state.child) {
          console.error(`Stopping sync for removed registration ${id}`);
          state.child.kill("SIGTERM");
        }
        children.delete(id);
      }
    }
    for (const marker of await readdir(registry)) {
      if (/^[A-Za-z0-9_-]{1,64}$/.test(marker) && !desired.has(marker)) {
        await unlink(`${registry}/${marker}`);
      }
    }
  } catch (error) {
    console.error(`Sync supervisor reconcile failed: ${error.message}`);
  } finally {
    reconciling = false;
  }
}

function shutdown(signal) {
  if (stopping) return;
  stopping = true;
  console.error(`Stopping sync supervisor (${signal})`);
  for (const state of children.values()) state.child?.kill("SIGTERM");
  setTimeout(() => {
    for (const state of children.values()) state.child?.kill("SIGKILL");
    process.exit(0);
  }, 10_000).unref();
  const active = [...children.values()].flatMap((state) => state.child ? [state.child] : []);
  if (active.length === 0) process.exit(0);
  Promise.all(active.map((child) => new Promise((resolve) => child.once("close", resolve))))
    .then(() => process.exit(0));
}

process.on("SIGTERM", () => shutdown("SIGTERM"));
process.on("SIGINT", () => shutdown("SIGINT"));
await reconcile();
setInterval(reconcile, 5_000);
