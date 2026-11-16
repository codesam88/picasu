// Checker self-tests: fixture catalogs prove the quint gates fail when
// they should. Hermetic — every case builds its own fixture tree under a
// tempdir and invokes the checker script against it; no repo state is
// touched. The fixture shape mirrors the layout the scripts accept:
//   <fixture>/utils/quint/pipeline/{events,operations}.json [+.qnt]
//   <fixture>/backend/src/**/*.rs
import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const quintDir = dirname(here);
const realPipeline = join(quintDir, "pipeline");

function fixtureRoot() {
  return mkdtempSync(join(tmpdir(), "quint-selftest-"));
}

// Minimal catalogs in the shape the checkers require.
function minimalOperations() {
  const operations = Array.from({ length: 12 }, (_, i) => ({
    id: `OP-${i + 1}`,
    name: `FIXTURE_${i + 1}`,
    events: [],
    reads: [],
    writes: [],
    guards: [],
    status: "open",
    nodes: [],
    notes: "",
  }));
  return {
    version: 1,
    description: "Fixture operations catalog for checker self-tests.",
    operations,
  };
}

function seedFixture(root, { qnt = false } = {}) {
  const pipeline = join(root, "utils", "quint", "pipeline");
  mkdirSync(pipeline, { recursive: true });
  writeFileSync(
    join(pipeline, "events.json"),
    JSON.stringify(
      { version: 1, description: "Fixture event catalog.", events: { version: 1, description: "Fixture event catalog.", events: [{"name": "API_READ_COMPRESSED", "status": "held", "description": "Fixture exempt event.", "actor": "environment", "target": "none", "preconditions": [], "affected_state": [], "code_ref": "TODO", "effects": ["fixture"], "faults": [], "notes": ""}, {"name": "API_READ_ORIGINAL", "status": "held", "description": "Fixture exempt event.", "actor": "environment", "target": "none", "preconditions": [], "affected_state": [], "code_ref": "TODO", "effects": ["fixture"], "faults": [], "notes": ""}, {"name": "API_READ_MISSING", "status": "held", "description": "Fixture exempt event.", "actor": "environment", "target": "none", "preconditions": [], "affected_state": [], "code_ref": "TODO", "effects": ["fixture"], "faults": [], "notes": ""}, {"name": "API_READ_MOVED", "status": "held", "description": "Fixture exempt event.", "actor": "environment", "target": "none", "preconditions": [], "affected_state": [], "code_ref": "TODO", "effects": ["fixture"], "faults": [], "notes": ""}, {"name": "PROCESS_CRASH", "status": "held", "description": "Fixture exempt event.", "actor": "environment", "target": "none", "preconditions": [], "affected_state": [], "code_ref": "TODO", "effects": ["fixture"], "faults": [], "notes": ""}, {"name": "DERIVED_STORE_LOSS", "status": "held", "description": "Fixture exempt event.", "actor": "environment", "target": "none", "preconditions": [], "affected_state": [], "code_ref": "TODO", "effects": ["fixture"], "faults": [], "notes": ""}, {"name": "DB_LOSS", "status": "held", "description": "Fixture exempt event.", "actor": "environment", "target": "none", "preconditions": [], "affected_state": [], "code_ref": "TODO", "effects": ["fixture"], "faults": [], "notes": ""}] } },
      null,
      2,
    ),
  );
  writeFileSync(
    join(pipeline, "operations.json"),
    JSON.stringify(minimalOperations(), null, 2),
  );
  if (qnt) {
    writeFileSync(
      join(pipeline, "fixture.qnt"),
      "module fixture {\n  val FixtureDeclaration = true\n}\n",
    );
  }
  mkdirSync(join(root, "backend", "src"), { recursive: true });
  return pipeline;
}

// Copy the committed catalogs wholesale and override select files — the
// positive fixtures need the real exempt-event names, schema enums and
// cross-references to resolve.
function seedRealPipeline(root) {
  const pipeline = join(root, "utils", "quint", "pipeline");
  mkdirSync(pipeline, { recursive: true });
  cpSync(realPipeline, pipeline, { recursive: true });
  mkdirSync(join(root, "backend", "src"), { recursive: true });
  return pipeline;
}

function seedMarkers(root, ops = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]) {
  const body = ops
    .map(
      (n) =>
        `/// Fixture operation site OP-${n}.\n#[quint::operation("OP-${n}")]\npub fn fixture_site_${n}() {}\n`,
    )
    .join("\n");
  writeFileSync(join(root, "backend", "src", "fixture.rs"), body);
}

function runScript(script, args) {
  const sub = script === "events-check.mjs" ? "events"
    : script === "catalog-check.mjs" ? "catalog"
    : script === "operations-check.mjs" ? "ops"
    : script === "specs-check.mjs" ? "specs"
    : script;
  return spawnSync("node", [join(quintDir, "coverage", "bin", "check.ts"), sub, ...args], { encoding: "utf8" });
}

// A step module importing one actions module: `init` and the step action
// live outside the branch list, `Beta` is the branch the step may omit.
function seedStepFixture(root, { complete = false, withStep = true } = {}) {
  const pipeline = join(root, "utils", "quint", "pipeline");
  mkdirSync(pipeline, { recursive: true });
  writeFileSync(
    join(pipeline, "actions.qnt"),
    [
      "module actions {",
      "  var n: int",
      "  action init: bool = all { n' = 0 }",
      "  action Alpha: bool = all { n' = n }",
      "  action Beta: bool = all { n' = n + 1 }",
      "}",
      "",
    ].join("\n"),
  );
  if (withStep) {
    writeFileSync(
      join(pipeline, "step.qnt"),
      [
        "module step_fixture {",
        '  import actions.* from "./actions"',
        "  action step: bool = any {",
        "    Alpha,",
        ...(complete ? ["    Beta,"] : []),
        "  }",
        "}",
        "",
      ].join("\n"),
    );
  }
  return pipeline;
}

// --- operations-check ---

test("operations-check accepts a fixture with every operation marked", () => {
  const root = fixtureRoot();
  seedFixture(root);
  seedMarkers(root);
  const r = runScript("operations-check.mjs", [root]);
  assert.equal(r.status, 0, r.stderr);
});

test("operations-check fails on a cataloged operation without a marker", () => {
  const root = fixtureRoot();
  seedFixture(root);
  seedMarkers(root, [1]);
  const r = runScript("operations-check.mjs", [root]);
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /has no .*marker on a real item/);
});

test("operations-check does not count a comment mention as a marker", () => {
  const root = fixtureRoot();
  seedFixture(root);
  writeFileSync(
    join(root, "backend", "src", "fixture.rs"),
    `// Markers in a comment: #[quint::operation("OP-1")] .. #[quint::operation("OP-12")].\n`,
  );
  const r = runScript("operations-check.mjs", [root]);
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /has no .*marker on a real item/);
});

test("operations-check fails on an unknown property model declaration", () => {
  const root = fixtureRoot();
  const pipeline = seedFixture(root);
  seedMarkers(root);
  const ops = JSON.parse(readFileSync(join(pipeline, "operations.json"), "utf8"));
  ops.operations[0].properties = [{ model: "NoSuchDeclaration" }];
  writeFileSync(join(pipeline, "operations.json"), JSON.stringify(ops, null, 2));
  const r = runScript("operations-check.mjs", [root]);
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /unknown model declaration/);
});

test("operations-check accepts a property whose model declaration exists", () => {
  const root = fixtureRoot();
  const pipeline = seedFixture(root, { qnt: true });
  seedMarkers(root);
  const ops = JSON.parse(readFileSync(join(pipeline, "operations.json"), "utf8"));
  ops.operations[0].properties = [{ model: "FixtureDeclaration" }];
  writeFileSync(join(pipeline, "operations.json"), JSON.stringify(ops, null, 2));
  const r = runScript("operations-check.mjs", [root]);
  assert.equal(r.status, 0, r.stderr);
});

// --- docs / doc-check subcommands (explicit file args are required) ---

test("docs subcommand requires an output file argument", () => {
  const root = fixtureRoot();
  const pipeline = seedRealPipeline(root);
  const r = runScript("docs", [pipeline]);
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /missing required output file/);
});

test("docs subcommand writes to the given output file", () => {
  const root = fixtureRoot();
  const pipeline = seedRealPipeline(root);
  const out = join(root, "custom", "out.md");
  const r = runScript("docs", [pipeline, out]);
  assert.equal(r.status, 0, r.stderr);
  assert.ok(readFileSync(out, "utf8").includes("Requirements:"));
});

test("doc-check reports drift against the given committed file", () => {
  const root = fixtureRoot();
  const pipeline = seedRealPipeline(root);
  const committed = join(root, "custom", "committed.md");
  mkdirSync(dirname(committed), { recursive: true });
  writeFileSync(committed, "stale content\n");
  const r = runScript("doc-check", [pipeline, committed]);
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /out of date with the generator/);
});

// --- events-check (real catalog as the positive fixture) ---

test("events-check accepts the committed event catalog", () => {
  const root = fixtureRoot();
  const pipeline = join(root, "utils", "quint", "pipeline");
  mkdirSync(pipeline, { recursive: true });
  cpSync(join(realPipeline, "events.json"), join(pipeline, "events.json"));
  cpSync(join(realPipeline, "catalog.schema.json"), join(pipeline, "catalog.schema.json"));
  const r = runScript("events-check.mjs", [pipeline]);
  assert.equal(r.status, 0, r.stderr);
});

test("events-check fails when an event loses a required field", () => {
  const root = fixtureRoot();
  const pipeline = join(root, "utils", "quint", "pipeline");
  mkdirSync(pipeline, { recursive: true });
  const events = JSON.parse(readFileSync(join(realPipeline, "events.json"), "utf8"));
  delete events.events[0].actor;
  writeFileSync(join(pipeline, "events.json"), JSON.stringify(events, null, 2));
  const r = runScript("events-check.mjs", [pipeline]);
  assert.notEqual(r.status, 0);
});

// --- catalog-check (real catalogs as the positive fixture) ---

test("catalog-check accepts the committed catalogs", () => {
  const root = fixtureRoot();
  const pipeline = seedRealPipeline(root);
  const r = runScript("catalog-check.mjs", [pipeline]);
  assert.equal(r.status, 0, r.stderr);
});

test("catalog-check fails when a committed goal derives no requirements", () => {
  const root = fixtureRoot();
  const pipeline = seedRealPipeline(root);
  const goals = JSON.parse(readFileSync(join(pipeline, "goals.json"), "utf8"));
  goals.goals.push({
    id: "G9",
    decision: "committed",
    title: "Fixture goal",
    description: "A committed goal with no derived requirement must fail.",
    anchors: [],
    assumptions: [],
    notes: "",
  });
  writeFileSync(join(pipeline, "goals.json"), JSON.stringify(goals, null, 2));
  const r = runScript("catalog-check.mjs", [pipeline]);
  assert.notEqual(r.status, 0);
  assert.match(r.stderr, /G9/);
});

// --- steps (branch completeness) ---

test("steps fails when the branch list omits a declared action", () => {
  const root = fixtureRoot();
  const pipeline = seedStepFixture(root);
  const r = runScript("steps", [pipeline]);
  assert.equal(r.status, 1, r.stderr);
  assert.match(
    r.stderr,
    /step 'step' in step\.qnt is missing declared action 'Beta'/,
  );
});

test("steps accepts a complete branch list and skips init and step itself", () => {
  const root = fixtureRoot();
  const pipeline = seedStepFixture(root, { complete: true });
  const r = runScript("steps", [pipeline]);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /2 branches/);
});

test("steps passes trivially when no step module exists", () => {
  const root = fixtureRoot();
  const pipeline = seedStepFixture(root, { withStep: false });
  const r = runScript("steps", [pipeline]);
  assert.equal(r.status, 0, r.stderr);
  assert.match(r.stdout, /no step actions/);
});

test("steps requires a pipeline-dir argument", () => {
  const r = runScript("steps", []);
  assert.equal(r.status, 2);
  assert.match(r.stderr, /missing required pipeline-dir argument/);
});
