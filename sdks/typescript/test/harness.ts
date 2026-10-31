// A real oml-server-lead on a throwaway database, for the SDK's tests.
//
// docs/engineering/sdks.md, "Tests": create a uniquely named database, boot the
// server on it with a keyring in a scratch directory, read the enrollment
// token from its stdout, claim the bootstrap and issue the keys with the `oml`
// CLI, and drop the database afterwards.
//
// Environment (all optional):
//   OML_SERVER_BIN              the lead binary (default: oml-server-lead on PATH)
//   OML_CLI_BIN                 the CLI binary (default: oml on PATH)
//   OML_SDK_TEST_DATABASE_URL   a role with CREATEDB
//                               (default: postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev)
//   OML_SDK_TEST_PORT           the port to listen on (default: one the OS hands out)
//   OML_SDK_TEST_SCRATCH        where the scratch directory goes (default: the OS temp dir)
//
// The server and the CLI get an environment built from an allow-list, so a key
// in the caller's environment never reaches either.

import { type ChildProcess, execFileSync, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { closeSync, mkdtempSync, openSync, readFileSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";

const ADMIN_URL =
  process.env.OML_SDK_TEST_DATABASE_URL ?? "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev";
const SERVER_BIN = process.env.OML_SERVER_BIN ?? "oml-server-lead";
const CLI_BIN = process.env.OML_CLI_BIN ?? "oml";
const ALLOWED_ENV = ["PATH", "HOME", "LANG", "LC_ALL", "TZ", "TMPDIR"];
const TOKEN_RE = /enr_[0-9a-f]{48}/;

export interface OmlServer {
  url: string;
  agentKey: string;
  dreamerKey: string;
  workarea: string;
  reviewWorkarea: string;
  stop(): Promise<void>;
}

function cleanEnv(extra: Record<string, string> = {}): Record<string, string> {
  const env: Record<string, string> = {};
  for (const name of ALLOWED_ENV) {
    const value = process.env[name];
    if (value !== undefined) env[name] = value;
  }
  return { ...env, ...extra };
}

function psql(sql: string): void {
  execFileSync("psql", [ADMIN_URL, "-v", "ON_ERROR_STOP=1", "-q", "-c", sql], {
    env: cleanEnv(),
    stdio: ["ignore", "ignore", "pipe"],
  });
}

function databaseUrl(name: string): string {
  const url = new URL(ADMIN_URL);
  url.pathname = `/${name}`;
  return url.toString();
}

async function freePort(): Promise<number> {
  if (process.env.OML_SDK_TEST_PORT) return Number(process.env.OML_SDK_TEST_PORT);
  return await new Promise((resolve, reject) => {
    const probe = createServer();
    probe.once("error", reject);
    probe.listen(0, "127.0.0.1", () => {
      const address = probe.address();
      const port = typeof address === "object" && address ? address.port : 0;
      probe.close(() => resolve(port));
    });
  });
}

function tail(path: string): string {
  try {
    return readFileSync(path, "utf8").split("\n").slice(-20).join("\n");
  } catch {
    return "";
  }
}

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

export async function startServer(): Promise<OmlServer> {
  const scratch = mkdtempSync(join(process.env.OML_SDK_TEST_SCRATCH ?? tmpdir(), "oml-sdk-ts-"));
  const database = `oml_sdk_ts_${randomBytes(6).toString("hex")}`;
  const port = await freePort();
  const url = `http://127.0.0.1:${port}`;
  const stdoutPath = join(scratch, "server.stdout");
  const stderrPath = join(scratch, "server.stderr");

  psql(`CREATE DATABASE ${database}`);
  let server: ChildProcess | undefined;
  const stop = async () => {
    if (server && server.exitCode === null && server.signalCode === null) {
      const exited = new Promise((resolve) => server?.once("exit", resolve));
      server.kill("SIGTERM");
      const timer = setTimeout(() => server?.kill("SIGKILL"), 10_000);
      await exited;
      clearTimeout(timer);
    }
    psql(`DROP DATABASE IF EXISTS ${database} WITH (FORCE)`);
    rmSync(scratch, { recursive: true, force: true });
  };

  try {
    const out = openSync(stdoutPath, "w");
    const err = openSync(stderrPath, "w");
    server = spawn(SERVER_BIN, [], {
      stdio: ["ignore", out, err],
      env: cleanEnv({
        OML_POSTGRES__URL: databaseUrl(database),
        OML_SERVER__LISTEN: `127.0.0.1:${port}`,
        OML_KMS__KEYRING_PATH: join(scratch, "keyring"),
      }),
    });
    closeSync(out);
    closeSync(err);
    let spawnError: Error | undefined;
    server.once("error", (error) => {
      spawnError = error;
    });

    let token: string | undefined;
    let ready = false;
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      if (spawnError) throw new Error(`could not start ${SERVER_BIN}: ${spawnError.message}`);
      if (server.exitCode !== null) {
        throw new Error(`oml-server-lead exited during boot:\n${tail(stderrPath)}`);
      }
      token ??= TOKEN_RE.exec(readFileSync(stdoutPath, "utf8"))?.[0];
      if (token) {
        try {
          if ((await fetch(`${url}/readyz`)).status === 200) {
            ready = true;
            break;
          }
        } catch {
          // not listening yet
        }
      }
      await sleep(100);
    }
    if (!token || !ready) {
      throw new Error(`oml-server-lead never became ready on ${url}:\n${tail(stderrPath)}`);
    }

    const cliEnv = cleanEnv({ OML_CLI_CREDENTIALS: join(scratch, "credentials") });
    const oml = (...args: string[]): Record<string, unknown> => {
      try {
        const stdout = execFileSync(CLI_BIN, ["--server", url, "--json", ...args], {
          env: cliEnv,
          encoding: "utf8",
          stdio: ["ignore", "pipe", "pipe"],
        });
        return JSON.parse(stdout) as Record<string, unknown>;
      } catch (error) {
        // stderr only: stdout may hold a key value
        const stderr = (error as { stderr?: string }).stderr ?? "";
        throw new Error(`oml ${args[0]} ${args[1]} failed:\n${stderr}`);
      }
    };

    const boot = oml(
      "bootstrap", "claim", "sdk-admin", "--token", token,
      "--tenant-name", "sdk-tests", "--principal", "platform/admin",
    );
    const root = String(boot.root_workarea_id);
    const workarea = String(oml("workareas", "create", root, "sdk").id);
    const reviewWorkarea = String(oml("workareas", "create", root, "sdk-review").id);
    const agentKey = String(
      oml(
        "keys", "issue", "sdk-agent", "--role", "writer", "--held-by", "agent",
        "--principal", "sdk/agent", "--default-workarea", workarea,
        "--grant", `${workarea}:writer`, "--grant", `${workarea}:reader`,
        "--grant", `${reviewWorkarea}:writer`, "--grant", `${reviewWorkarea}:reader`,
      ).value,
    );
    const dreamerKey = String(
      oml(
        "keys", "issue", "sdk-reviewer", "--role", "dreamer", "--held-by", "human",
        "--principal", "sdk/reviewer", "--default-workarea", reviewWorkarea,
        "--grant", `${reviewWorkarea}:dreamer`,
      ).value,
    );
    return { url, agentKey, dreamerKey, workarea, reviewWorkarea, stop };
  } catch (error) {
    await stop();
    throw error;
  }
}

/** A word no other test writes, for recall queries. */
export function marker(): string {
  const letters = "abcdefghijklmnopqrstuvwxyz";
  return `sdkmark${Array.from(randomBytes(10), (b) => letters[b % 26]).join("")}`;
}
