#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  existsSync,
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

const script = join(dirname(fileURLToPath(import.meta.url)), "quality-gates.mjs");
const repoRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

function run(root, extra = []) {
  try {
    const out = execFileSync("node", [script, root, ...extra], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    });
    return { code: 0, out };
  } catch (error) {
    return { code: error.status ?? 1, out: `${error.stdout ?? ""}${error.stderr ?? ""}` };
  }
}

test("an unwired check recipe fails the gate", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "justfile"), "# foo\nfoo-check:\n    echo foo\n");
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  const r = run(dir);
  assert.notEqual(r.code, 0, r.out);
  assert.match(r.out, /not invoked by any lane|foo-check/);
});

test("regeneration is deterministic", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const a = join(dir, "a.md");
  const b = join(dir, "b.md");
  const r1 = run(repoRoot, ["--generate", a]);
  assert.equal(r1.code, 0, r1.out);
  const r2 = run(repoRoot, ["--generate", b]);
  assert.equal(r2.code, 0, r2.out);
  assert.deepEqual(readFileSync(a), readFileSync(b));
});

test("check mode writes nothing", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "justfile"), "# foo\nfoo-check:\n    echo foo\n");
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  run(dir);
  assert.equal(
    existsSync(join(dir, "docs", "quality-gates.md")),
    false,
    "check mode must not create output files",
  );
});

test("blob-link format for a known recipe", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  assert.match(
    md,
    /https:\/\/github\.com\/codesam88\/picasu\/blob\/main\/justfile#L\d+-L\d+/,
  );
  assert.match(md, /justfile:\d+-\d+/);
  assert.match(md, /backend-check/);
});

test("widened gate rule: unwired foo-lint fails, wired default-param recipe passes", (t) => {
  // An unwired foo-lint recipe fails the gate.
  const dir1 = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir1, { recursive: true, force: true }));
  writeFileSync(join(dir1, "justfile"), "# foo\nfoo-lint:\n    echo foo\n");
  mkdirSync(join(dir1, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir1, "docs"), { recursive: true });
  const r1 = run(dir1);
  assert.notEqual(r1.code, 0, r1.out);
  assert.match(r1.out, /foo-lint/);

  // A wired default-param recipe (bar-playwright baz="") passes.
  const dir2 = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir2, { recursive: true, force: true }));
  writeFileSync(
    join(dir2, "justfile"),
    '# bar\nbar-playwright baz="":\n    echo bar\n',
  );
  mkdirSync(join(dir2, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir2, "docs"), { recursive: true });
  writeFileSync(
    join(dir2, ".github", "workflows", "ci.yml"),
    "jobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: just bar-playwright\n",
  );
  const g = run(dir2, ["--generate", join(dir2, "docs", "quality-gates.md")]);
  assert.equal(g.code, 0, g.out);
  const r2 = run(dir2);
  assert.equal(r2.code, 0, r2.out);
});

test("regenerated doc lists openapi-routes-release in the release lane", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const md = join(dir, "quality-gates.md");
  const r = run(repoRoot, ["--generate", md]);
  assert.equal(r.code, 0, r.out);
  const content = readFileSync(md, "utf8");
  assert.match(content, /### `openapi-routes-release`/);
  assert.match(content, /### `openapi-routes-release`[\s\S]*?- Lanes: [^\n]*release/);
});


test("precommit lane present on backend-check", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  assert.match(md, /### `backend-check`[\s\S]*?- Lanes: [^\n]*precommit/);
});

test("every emitted lane node has at least one edge and no self-loops", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  const m = md.match(/```mermaid\n([\s\S]*?)```/);
  assert.ok(m, "mermaid block present");
  const block = m[1];
  const laneNodes = [...block.matchAll(/^\s*(lane_[A-Za-z0-9_]+)\[/gm)].map((x) => x[1]);
  const edges = [...block.matchAll(/^\s*(\S+?)\s*-->\s*([A-Za-z0-9_]+)/gm)].map((x) => ({
    from: x[1],
    to: x[2],
  }));
  for (const node of laneNodes) {
    assert.ok(
      edges.some((e) => e.from === node),
      `lane node ${node} has no outgoing edge`,
    );
  }
  for (const e of edges) {
    assert.notEqual(e.from, e.to, `self-loop edge ${e.from} --> ${e.to}`);
  }
});

test("openapi-sanity has no direct lane edge", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  const m = md.match(/```mermaid\n([\s\S]*?)```/);
  assert.ok(m);
  const block = m[1];
  const directLaneEdges = [...block.matchAll(/^\s*lane_[A-Za-z0-9_]+\s*-->\s*openapi_sanity\b/gm)];
  assert.equal(directLaneEdges.length, 0, "openapi-sanity should not be a direct lane target");
});

test("checks sections carry a Required by line", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  const backend = md.match(/### `backend-check`([\s\S]*?)(?=\n### `|\s*$)/);
  assert.ok(backend, "backend-check section present");
  assert.match(backend[1], /- Required by: `check`\n/);
  const sanity = md.match(/### `openapi-sanity`([\s\S]*?)(?=\n### `|\s*$)/);
  assert.ok(sanity, "openapi-sanity section present");
  assert.match(sanity[1], /- Required by: .*`openapi-check`/);
});
