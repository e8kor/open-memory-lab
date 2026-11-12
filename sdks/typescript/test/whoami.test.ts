// A whoami without `shared`, as a server older than ADR-0076 answers, is a
// Whoami, and a client reads its missing `shared` as not shared (ADR-0076 d9,
// the operator's clarification of 2026-11-11). Needs no server: `npm run
// check` type-checks this file, and a required `shared` fails it.

import assert from "node:assert/strict";
import { test } from "node:test";

import type { components } from "../src/index.ts";

type Whoami = components["schemas"]["Whoami"];

const older: Whoami = JSON.parse(
  JSON.stringify({
    key_id: "key_w",
    name: "writer",
    role: "writer",
    principal: "p",
    held_by: "agent",
    tenant: { id: "t1", name: "oml" },
    default_workarea: null,
    crew: null,
    user: null,
    grants: [],
  }),
);

// Assigned without a cast: compiles only while `shared` is optional.
const typed: Whoami = {
  key_id: "key_w",
  name: "writer",
  role: "writer",
  principal: "p",
  held_by: "agent",
  tenant: { id: "t1", name: "oml" },
  default_workarea: null,
  crew: null,
  user: null,
  grants: [],
};

test("a whoami without shared parses and reads as not shared", () => {
  assert.equal(older.shared ?? false, false);
  assert.equal(typed.shared ?? false, false);
  const shared: Whoami = { ...typed, shared: true };
  assert.equal(shared.shared ?? false, true);
});
