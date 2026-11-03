// The harness's minting check, read through psql as it runs it (ADR-0034 d8).
//
// The admin role here is rarely a superuser, so the check's two answers are
// proved by putting a literal boolean where `rolsuper` stands: the same query,
// the same psql flags, the same parse.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { test } from "node:test";

import { ADMIN_ROLE_QUERY, adminRole } from "./harness.ts";

test("a superuser admin role is read as one that can mint", () => {
  assert.ok(ADMIN_ROLE_QUERY.includes("rolsuper"));
  assert.equal(adminRole(ADMIN_ROLE_QUERY.replace("rolsuper", "true")).superuser, true);
});

test("any other admin role is read as one that cannot mint", () => {
  assert.equal(adminRole(ADMIN_ROLE_QUERY.replace("rolsuper", "false")).superuser, false);
});

test("the admin role is read by its name", () => {
  const url =
    process.env.OML_SDK_TEST_DATABASE_URL ?? "postgres://oml:oml_dev_only@127.0.0.1:5432/oml_dev";
  const name = execFileSync("psql", [url, "-q", "-t", "-A", "-c", "SELECT current_user"], {
    env: { PATH: process.env.PATH ?? "" },
    encoding: "utf8",
  }).trim();
  assert.equal(adminRole(ADMIN_ROLE_QUERY.replace("rolsuper", "false")).role, name);
});
