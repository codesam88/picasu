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
