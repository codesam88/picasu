#!/usr/bin/env node
// Behavior pins for auth-matrix.mjs. Mirrors env-var-lint.test.mjs style:
// node:test, throwaway fixtures where useful, assertions against derived output.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const script = join(dirname(fileURLToPath(import.meta.url)), "auth-matrix.mjs");
const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const spec = JSON.parse(
  readFileSync(join(repoRoot, "backend", "openapi.json"), "utf8"),
);

const AUTH_FIELDS = [
  "admin",
  "effects",
  "guards",
  "invariants",
  "public",
  "shareScope",
  "tokenTyp",
];

function countOps(s) {
  let n = 0;
  for (const p of Object.keys(s.paths))
    for (const m of Object.keys(s.paths[p])) n++;
  return n;
}

function run(root) {
  try {
    const out = execFileSync("node", [script, root], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
    return { code: 0, out };
  } catch (error) {
    return {
      code: error.status ?? 1,
      out: `${error.stdout ?? ""}${error.stderr ?? ""}`,
    };
  }
}

test("generator runs against the repo root", (t) => {
  const r = run(repoRoot);
  assert.equal(r.code, 0, r.out);
});

test("machine file row count equals spec operation count", (t) => {
  const records = JSON.parse(
    readFileSync(join(repoRoot, "backend", "auth-matrix.json"), "utf8"),
  );
  assert.equal(records.length, countOps(spec));
  assert.equal(records.length, 74);
});

test("every record has sorted keys", (t) => {
  const records = JSON.parse(
    readFileSync(join(repoRoot, "backend", "auth-matrix.json"), "utf8"),
  );
  for (const r of records) {
    const keys = Object.keys(r);
    assert.deepEqual(keys, [...keys].sort(), r.operationId);
    if (r["x-picasu-auth"]) {
      const akeys = Object.keys(r["x-picasu-auth"]);
      assert.deepEqual(akeys, [...akeys].sort(), r.operationId);
    }
  }
});

test("every annotated op has a row with all 7 x-picasu-auth fields", (t) => {
  const records = JSON.parse(
    readFileSync(join(repoRoot, "backend", "auth-matrix.json"), "utf8"),
  );
  const byId = new Map(records.map((r) => [r.operationId, r]));
  for (const p of Object.keys(spec.paths)) {
    for (const m of Object.keys(spec.paths[p])) {
      const op = spec.paths[p][m];
      if (!op["x-picasu-auth"]) continue;
      const r = byId.get(op.operationId);
      assert.ok(r, op.operationId);
      assert.ok(r["x-picasu-auth"], op.operationId);
      const fields = Object.keys(r["x-picasu-auth"]).sort();
      assert.deepEqual(fields, [...AUTH_FIELDS].sort(), op.operationId);
    }
  }
});

test("public:true iff security is [{}] (S7) for modeled rows", (t) => {
  const records = JSON.parse(
    readFileSync(join(repoRoot, "backend", "auth-matrix.json"), "utf8"),
  );
  for (const r of records) {
    if (!r.modeled) continue;
    const a = r["x-picasu-auth"];
    const pub = a.public === true;
    const secEmpty = JSON.stringify(r.security) === "[{}]";
    assert.equal(
      pub,
      secEmpty,
      `${r.operationId}: public=${pub} security=${JSON.stringify(r.security)}`,
    );
  }
});

test("page shells are listed as unmodeled", (t) => {
  const records = JSON.parse(
    readFileSync(join(repoRoot, "backend", "auth-matrix.json"), "utf8"),
  );
  const unmodeled = records.filter((r) => !r.modeled);
  const shells = [];
  for (const p of Object.keys(spec.paths)) {
    for (const m of Object.keys(spec.paths[p])) {
      const op = spec.paths[p][m];
      if (!op["x-picasu-auth"]) shells.push(op.operationId);
    }
  }
  assert.equal(unmodeled.length, shells.length);
  assert.equal(unmodeled.length, 23);
  assert.deepEqual(unmodeled.map((r) => r.operationId).sort(), shells.sort());
  for (const r of unmodeled) {
    assert.equal(r.security, null, r.operationId);
    assert.equal(r["x-picasu-auth"], null, r.operationId);
  }
});

test("human doc has modeled, public, and unmodeled sections", (t) => {
  const md = readFileSync(join(repoRoot, "docs", "auth-matrix.md"), "utf8");
  assert.match(md, /## Columns/);
  assert.match(md, /## Modeled operations/);
  assert.match(md, /## Public operations/);
  assert.match(md, /## Unmodeled operations \(page shells\)/);
  // sample rows
  assert.match(md, /delete_data/);
  assert.match(md, /redirect_to_photo/);
});

test("human table has name, role, tokens, and guards columns", (t) => {
  const md = readFileSync(join(repoRoot, "docs", "auth-matrix.md"), "utf8");
  const norm = md.replace(/ +/g, " ");
  assert.ok(
    norm.includes("| Op | Name | Role | Tokens | Scope | Guards | Effects |"),
    "modeled table header",
  );
  assert.ok(!norm.includes("| Typ |"), "no abbreviated Typ header");
  assert.ok(
    norm.includes(
      "`PUT /put/set_album_title` | `set_album_title` | any(admin, user) | session | — | GuardUser, GuardMutate |",
    ),
    "share-scoped editor row",
  );
  assert.ok(
    norm.includes(
      "`GET /get/get-data` | `get_data` | none(snapshot) | snapshot |",
    ),
    "token-only row names its token basis",
  );
  assert.ok(
    norm.includes(
      "`DELETE /delete/delete-data` | `delete_data` | any(admin, user) | session |",
    ),
    "user-guarded row names the role",
  );
  assert.ok(
    norm.includes(
      "`GET /get/server-info` | `server_info_handler` | none(public) |",
    ),
    "public row names its basis",
  );
  assert.ok(
    norm.includes(
      "`GET /object/compressed/{file_path}` | `compressed_file` | any(admin, user, share) | session, share, asset | — |",
    ),
    "serving row carries no enforced album scope (bearer authorizes the asset)",
  );
});

test("fixture: generator derives artifacts in a throwaway tree", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "authmatrix-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, "backend"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  // minimal openapi with one annotated and one unmodeled op
  const mini = {
    paths: {
      "/x": {
        get: {
          operationId: "get_x",
          tags: ["t"],
          security: [{}],
          "x-picasu-auth": {
            admin: "none",
            effects: [],
            guards: [],
            invariants: [],
            public: true,
            shareScope: "n-a",
            tokenTyp: [],
          },
        },
      },
      "/y": { get: { operationId: "get_y", tags: ["pages"] } },
    },
  };
  writeFileSync(join(dir, "backend", "openapi.json"), JSON.stringify(mini));
  const r = run(dir);
  assert.equal(r.code, 0, r.out);
  const records = JSON.parse(
    readFileSync(join(dir, "backend", "auth-matrix.json"), "utf8"),
  );
  assert.equal(records.length, 2);
  assert.equal(records.filter((x) => x.modeled).length, 1);
  assert.equal(records.filter((x) => !x.modeled).length, 1);
});

test("every effect is one class and only public operations are unclassified", (t) => {
  const records = JSON.parse(
    readFileSync(join(repoRoot, "backend", "auth-matrix.json"), "utf8"),
  );
  const vocabulary = new Set(["read", "write", "manage", "control"]);
  for (const record of records) {
    if (!record.modeled) continue;
    const auth = record["x-picasu-auth"];
    if (auth.public) {
      assert.deepEqual(
        auth.effects,
        [],
        `${record.operationId}: public operations carry no effect`,
      );
      continue;
    }
    assert.equal(
      auth.effects.length,
      1,
      `${record.operationId}: exactly one effect class`,
    );
    assert.ok(
      vocabulary.has(auth.effects[0]),
      `${record.operationId}: ${auth.effects[0]}`,
    );
  }
});

test("every referenced invariant is declared in the authz model", (t) => {
  const records = JSON.parse(
    readFileSync(join(repoRoot, "backend", "auth-matrix.json"), "utf8"),
  );
  const model = readFileSync(join(repoRoot, "docs", "authz-model.md"), "utf8");
  const declared = new Set(model.match(/AUTH-\d+/g) ?? []);
  const referenced = new Set();
  for (const record of records) {
    const auth = record["x-picasu-auth"];
    if (!auth) continue;
    for (const invariant of auth.invariants) referenced.add(invariant);
  }
  const unknown = [...referenced].filter(
    (invariant) => !declared.has(invariant),
  );
  assert.deepEqual(
    unknown,
    [],
    `invariants not declared in docs/authz-model.md: ${unknown}`,
  );
  assert.ok(
    referenced.size >= 8,
    `expected the model to reference at least 8 invariants, saw ${referenced.size}`,
  );
});
