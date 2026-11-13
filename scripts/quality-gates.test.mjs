#!/usr/bin/env node
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
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
  const p = spawnSync("node", [script, root, ...extra], { encoding: "utf8" });
  return { code: p.status ?? 1, out: `${p.stdout ?? ""}${p.stderr ?? ""}` };
}

test("stale committed doc is flagged", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "justfile"), "# foo\nfoo-check:\n    echo foo\n");
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  const g = run(dir, ["--generate", join(dir, "docs", "quality-gates.md")]);
  assert.equal(g.code, 0, g.out);
  writeFileSync(join(dir, "docs", "quality-gates.md"), "# stale\n");
  const r = run(dir);
  assert.match(r.out, /differs from regenerated output/);
  assert.match(r.out, /just gates-docs/);
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

test("widened gate rule: unwired foo-lint is tracked, wired default-param recipe passes", (t) => {
  const dir1 = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir1, { recursive: true, force: true }));
  writeFileSync(join(dir1, "justfile"), "# foo\nfoo-lint:\n    echo foo\n");
  mkdirSync(join(dir1, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir1, "docs"), { recursive: true });
  const g1 = run(dir1, ["--generate", join(dir1, "docs", "quality-gates.md")]);
  assert.equal(g1.code, 0, g1.out);
  // foo-lint appears in Orphans (no CI lane, no parent).
  const md1 = readFileSync(join(dir1, "docs", "quality-gates.md"), "utf8");
  assert.match(md1, /`foo-lint`/);

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

test("regenerated doc lists openapi-routes-release with its release trigger", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const md = join(dir, "quality-gates.md");
  const r = run(repoRoot, ["--generate", md]);
  assert.equal(r.code, 0, r.out);
  const content = readFileSync(md, "utf8");
  assert.match(
    content,
    /`openapi-routes-release`[^\n]*\| release\s*\| tags/,
  );
});

test("rule graph has separate check and test trees", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  const section = md.slice(0, md.indexOf("## Orphans"));
  const blocks = [...section.matchAll(/```mermaid\n([\s\S]*?)```/g)].map((m) => m[1]);
  assert.equal(blocks.length, 2);
  const [checkBlock, testBlock] = blocks;
  assert.match(checkBlock, /^graph LR$/m);
  assert.match(checkBlock, /^  check --> backend-check$/m);
  assert.match(checkBlock, /^  openapi-check --> openapi-sanity$/m);
  assert.match(testBlock, /^  test --> frontend-test$/m);
  assert.match(testBlock, /^  frontend-test --> frontend-vitest$/m);
});

test("trigger column reflects the CI workflow files and callers", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  assert.match(
    md,
    /`backend-check`[^\n]*\| ci\s*\| local, PR, main/,
  );
  assert.match(
    md,
    /`dylint-check`[^\n]*\| ci\s*\| PR, main/,
  );
  assert.match(md, /`quint-check`[^\n]*\| ❌️\s*\| local/);
  assert.match(md, /`check-tools`[^\n]*\| ❌️\s*\| -\s*\|/);
  assert.match(
    md,
    /`msrv-check`[^\n]*\| msrv\s*\| scheduled/,
  );
  assert.match(
    md,
    /`openapi-routes-release`[^\n]*\| release\s*\| tags/,
  );
  assert.match(
    md,
    /`zizmor-check`[^\n]*\| zizmor, \(utils-check\)\s*\| local, PR, main/,
  );
  // An indirectly-run recipe names the CI caller instead of repeating the file.
  assert.match(md, /`auth-matrix-check`[^\n]*\| \(openapi-check\)\s*\|/);
});

test("family tables share the full-width style and five-column header", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  assert.match(md, /h3 \+ \.table-wrapper > table \{\n\s+width: 100%;\n\s+table-layout: fixed;/);
  const headers = md.match(
    /^\| Rule \(justfile\)\s+\| Description\s+\| CI Workflow\s+\| Trigger\s+\|$/gm,
  );
  const families = md.slice(md.indexOf("## Rules by group")).match(/^### /gm);
  assert.ok(families.length > 0, "group sections present");
  assert.equal(headers.length, families.length, "every group has the header");
});

test("each group has a local mermaid diagram", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  const section = md.slice(md.indexOf("## Rules by group"));
  const groups = section.match(/^### .*$/gm) ?? [];
  const blocks = section.match(/```mermaid\n/g) ?? [];
  assert.equal(blocks.length, groups.length, "one diagram per group");
  assert.match(section, /openapi-check --> auth-matrix-check/);
  assert.match(section, /utils-check --> zizmor-check/);
  assert.match(section, /~~~ /);
  assert.match(section, /bench__test\["bench::test"\]/);
});

test("orphans section lists branches-prune and check-tools", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  assert.match(md, /## Orphans \(Called Manually\)/);
  assert.match(md, /branches-prune/);
  assert.match(md, /check-tools/);
  // check/test/precommit/audit must not appear as orphan bullets
  const section = md.split("## Orphans (Called Manually)")[1].split("## Rules by group")[0];
  assert.doesNotMatch(section, /- `check`/);
  assert.doesNotMatch(section, /- `test`/);
  assert.doesNotMatch(section, /- `precommit`/);
  assert.doesNotMatch(section, /- `audit`/);
});

test("doc contains no #L anchors and no file:line references", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  assert.doesNotMatch(md, /#L/);
  assert.doesNotMatch(md, /:\d+-\d+/);
});

test("synthetic foo-check generates Other group and Orphans entry", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "justfile"), "# foo\nfoo-check:\n    echo foo\n");
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  const g = run(dir, ["--generate", join(dir, "docs", "quality-gates.md")]);
  assert.equal(g.code, 0, g.out);
  const md = readFileSync(join(dir, "docs", "quality-gates.md"), "utf8");
  assert.match(md, /### Other/);
  assert.match(md, /`foo-check`/);
  assert.match(md, /## Orphans \(Called Manually\)[\s\S]*?- `foo-check`/);
});

test("inline check tooling without a just recipe is flagged", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "justfile"), "# helper\nh does x:\n    echo hi\n");
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  writeFileSync(
    join(dir, ".github", "workflows", "ci.yml"),
    "on:\n  pull_request:\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo clippy --all-targets -- -D warnings\n",
  );
  const g = run(dir, ["--generate", join(dir, "docs", "quality-gates.md")]);
  assert.equal(g.code, 0, g.out);
  const r = run(dir);
  assert.match(r.out, /without a just recipe/);
});

test("allowlisted CI-only recipe passes with its reason rendered", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  assert.match(md, /dylint-check/);
  assert.match(
    md,
    /Note: `dylint-check` - needs nightly-2026-08-01 toolchain\+rDylint; CI-only by decision 2026-11-05/,
  );
  const r = run(repoRoot);
  assert.equal(r.code, 0, r.out);
});

test("--check-only behaves like the default check", () => {
  const r = run(repoRoot, ["--check-only"]);
  assert.equal(r.code, 0, r.out);
  assert.match(r.out, /gates-check: OK/);
});

test("--check <path> diffs against the named file", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "justfile"), "# foo\nfoo-check:\n    echo foo\n");
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  const custom = join(dir, "custom.md");
  const g = run(dir, ["--generate", custom]);
  assert.equal(g.code, 0, g.out);
  const r = run(dir, ["--check", custom]);
  assert.equal(r.code, 0, r.out);
  writeFileSync(custom, "# stale\n");
  const r2 = run(dir, ["--check", custom]);
  assert.match(r2.out, /differs from regenerated output/);
  assert.match(r2.out, /custom\.md/);
});

test("module recipe appears and private module recipe is absent", () => {
  const md = readFileSync(join(repoRoot, "docs", "quality-gates.md"), "utf8");
  assert.match(md, /### Bench/);
  assert.match(md, /`bench::test`/);
  assert.doesNotMatch(md, /`_venv`/);
});
