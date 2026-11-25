#!/usr/bin/env node
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
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

function run(root, extra = []) {
  const p = spawnSync("node", [script, root, ...extra], { encoding: "utf8" });
  return { code: p.status ?? 1, out: `${p.stdout ?? ""}${p.stderr ?? ""}` };
}

function tables(md) {
  const lines = md.split("\n");
  const out = [];
  let cur = null;
  for (const line of lines) {
    if (/^\|/.test(line)) {
      if (!cur) cur = [];
      cur.push(line);
    } else {
      if (cur) {
        out.push(cur);
        cur = null;
      }
    }
  }
  if (cur) out.push(cur);
  return out;
}

function splitCols(row) {
  return row.split("|").slice(1, -1).map((c) => c.trim());
}

function docOrphans(md) {
  const marker = "## Orphans (not reachable from a global rule or CI workflow)";
  const i = md.indexOf(marker);
  if (i < 0) return new Set();
  const rest = md.slice(i);
  const next = rest.indexOf("## ", marker.length);
  const section = next >= 0 ? rest.slice(0, next) : rest;
  if (/\bNone\./.test(section)) return new Set();
  const set = new Set();
  for (const m of section.matchAll(/`([^`]+)`/g)) set.add(m[1]);
  return set;
}

function stdoutOrphans(out) {
  const lines = out.split("\n");
  const idx = lines.findIndex((l) => /Orphan candidates/.test(l));
  if (idx < 0) return new Set();
  if (lines[idx].includes("none")) return new Set();
  const set = new Set();
  for (let i = idx + 1; i < lines.length; i += 1) {
    const m = lines[i].match(/^  (\S+)\s*$/);
    if (!m) break;
    if (lines[i].includes("gates-check")) break;
    set.add(m[1]);
  }
  return set;
}

function mermaidBlocks(md) {
  return [...md.matchAll(/```mermaid\n([\s\S]*?)```/g)].map((m) => m[1]);
}

function parseBlock(block) {
  const nodes = new Set();
  const implicit = new Set();
  const edges = [];
  for (const raw of block.split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith("graph")) continue;
    const edge = line.match(/^(\S+)\s*(-->|~~~)\s*(.+)$/);
    if (edge) {
      const src = edge[1];
      implicit.add(src);
      for (const p of edge[3].split("&")) {
        const dst = p.trim();
        edges.push({ src, dst });
        implicit.add(dst);
      }
      continue;
    }
    const node = line.match(/^([A-Za-z0-9_-]+)(?:\[".*"\])?$/);
    if (node) nodes.add(node[1]);
  }
  return { nodes: nodes.size ? nodes : implicit, edges };
}

function findRecipeRow(md, name) {
  for (const t of tables(md)) {
    if (t.length < 2) continue;
    const header = splitCols(t[0]);
    if (!header.includes("Rule (justfile)")) continue;
    for (const row of t.slice(2)) {
      const cols = splitCols(row);
      if (cols[0] === "`" + name + "`") return { header, cols };
    }
  }
  return null;
}

function recipeNames(dir) {
  const out = execFileSync("just", ["--json"], { cwd: dir, encoding: "utf8" });
  const json = JSON.parse(out);
  const pub = [];
  const priv = [];
  function walk(m) {
    for (const r of Object.values(m.recipes || {})) {
      if (r.private) priv.push(r.namepath);
      else pub.push(r.namepath);
    }
    for (const sub of Object.values(m.modules || {})) walk(sub);
  }
  walk(json);
  return { pub, priv };
}

const ALLOWED_TRIGGERS = new Set(["local", "PR", "main", "tags", "scheduled"]);

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
  writeFileSync(join(dir, "justfile"), "# check\ncheck: foo\n\n# foo\nfoo:\n    echo foo\n");
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  const a = join(dir, "a.md");
  const b = join(dir, "b.md");
  const r1 = run(dir, ["--generate", a]);
  assert.equal(r1.code, 0, r1.out);
  const r2 = run(dir, ["--generate", b]);
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

test("unwired foo-lint is tracked; wired default-param recipe passes", (t) => {
  const dir1 = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir1, { recursive: true, force: true }));
  writeFileSync(join(dir1, "justfile"), "# foo\nfoo-lint:\n    echo foo\n");
  mkdirSync(join(dir1, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir1, "docs"), { recursive: true });
  const g1 = run(dir1, ["--generate", join(dir1, "docs", "quality-gates.md")]);
  assert.equal(g1.code, 0, g1.out);
  const md1 = readFileSync(join(dir1, "docs", "quality-gates.md"), "utf8");
  assert.ok(docOrphans(md1).has("foo-lint"), "foo-lint should be orphan");
  const r1 = run(dir1);
  assert.deepEqual(stdoutOrphans(r1.out), new Set(["foo-lint"]));

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
  assert.ok(!docOrphans(readFileSync(join(dir2, "docs", "quality-gates.md"), "utf8")).has("bar-playwright"));
});

test("recipe with release workflow appears with release/tags vocabulary", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(join(dir, "justfile"), "# foo\nfoo-release:\n    echo foo\n");
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  writeFileSync(
    join(dir, ".github", "workflows", "release.yml"),
    "on:\n  push:\n    tags: ['v*']\njobs:\n  release:\n    runs-on: ubuntu-latest\n    steps:\n      - run: just foo-release\n",
  );
  const g = run(dir, ["--generate", join(dir, "docs", "quality-gates.md")]);
  assert.equal(g.code, 0, g.out);
  const md = readFileSync(join(dir, "docs", "quality-gates.md"), "utf8");
  const found = findRecipeRow(md, "foo-release");
  assert.ok(found, "foo-release row should exist");
  assert.match(found.cols[2], /release/);
  const triggers = found.cols[3] === "-" ? [] : found.cols[3].split(",").map((s) => s.trim());
  assert.ok(triggers.includes("tags"), "trigger should include tags");
  for (const tok of triggers) assert.ok(ALLOWED_TRIGGERS.has(tok), `unknown trigger ${tok}`);
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
  assert.ok(docOrphans(md).has("foo-check"));
});

test("orphan candidates use reachability, not just parent callers", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  writeFileSync(
    join(dir, "justfile"),
    "# check\ncheck: covered\n\n# covered\ncovered:\n    echo covered\n\n# detached\ndetached: detached-child\n    echo detached\n\n# child\ndetached-child:\n    echo child\n",
  );
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  const g = run(dir, ["--generate", join(dir, "docs", "quality-gates.md")]);
  assert.equal(g.code, 0, g.out);
  const r = run(dir);
  assert.equal(r.code, 0, r.out);
  assert.deepEqual(stdoutOrphans(r.out), new Set(["detached", "detached-child"]));

  writeFileSync(
    join(dir, ".github", "workflows", "ci.yml"),
    "on:\n  pull_request:\njobs:\n  check:\n    runs-on: ubuntu-latest\n    steps:\n      - run: just detached\n",
  );
  const g2 = run(dir, ["--generate", join(dir, "docs", "quality-gates.md")]);
  assert.equal(g2.code, 0, g2.out);
  const r2 = run(dir);
  assert.equal(r2.code, 0, r2.out);
  assert.deepEqual(stdoutOrphans(r2.out), new Set());
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

test("CI-only recipe note is rendered", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  // The name matters: the script carries a built-in note for dylint-check.
  writeFileSync(join(dir, "justfile"), "# dylint\ndylint-check:\n    echo x\n");
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  const g = run(dir, ["--generate", join(dir, "docs", "quality-gates.md")]);
  assert.equal(g.code, 0, g.out);
  const md = readFileSync(join(dir, "docs", "quality-gates.md"), "utf8");
  assert.match(md, /Note: `dylint-check`/);
  assert.match(md, /CI-only/);
});

test("module recipe appears and private module recipe is absent", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, "bench"), { recursive: true });
  writeFileSync(join(dir, "justfile"), "# root\ncheck:\n    echo check\n\nmod bench \"bench/justfile\"\n");
  writeFileSync(
    join(dir, "bench", "justfile"),
    "# bench test\ntest:\n    echo test\n\n[private]\n_venv:\n    echo venv\n",
  );
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  const g = run(dir, ["--generate", join(dir, "docs", "quality-gates.md")]);
  assert.equal(g.code, 0, g.out);
  const md = readFileSync(join(dir, "docs", "quality-gates.md"), "utf8");
  assert.match(md, /bench::test/);
  assert.doesNotMatch(md, /_venv/);
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

// Invariant tests over a synthetic generated doc.
test("structural invariants hold for a generated doc", (t) => {
  const dir = mkdtempSync(join(tmpdir(), "pipelint-"));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, "bench"), { recursive: true });
  writeFileSync(
    join(dir, "justfile"),
    "# check\ncheck: covered\n\n# covered\ncovered:\n    echo covered\n\n# detached\ndetached: detached-child\n    echo detached\n\n# child\ndetached-child:\n    echo child\n\n# foo\nfoo-check:\n    echo foo\n\n[private]\n_secret:\n    echo secret\n\nmod bench \"bench/justfile\"\n",
  );
  writeFileSync(
    join(dir, "bench", "justfile"),
    "# bench test\ntest:\n    echo test\n\n[private]\n_venv:\n    echo venv\n",
  );
  mkdirSync(join(dir, ".github", "workflows"), { recursive: true });
  mkdirSync(join(dir, "docs"), { recursive: true });
  const out = join(dir, "docs", "quality-gates.md");
  const g = run(dir, ["--generate", out]);
  assert.equal(g.code, 0, g.out);
  const md = readFileSync(out, "utf8");

  const { pub, priv } = recipeNames(dir);
  for (const name of pub) {
    assert.ok(md.includes("`" + name + "`"), `public recipe ${name} should appear`);
  }
  for (const name of priv) {
    assert.ok(!md.includes(name), `private recipe ${name} should not appear`);
  }

  assert.doesNotMatch(md, /#L/);
  assert.doesNotMatch(md, /:\d+-\d+/);

  for (const block of mermaidBlocks(md)) {
    const { nodes, edges } = parseBlock(block);
    for (const e of edges) {
      assert.ok(nodes.has(e.src), `edge src ${e.src} must be a node`);
      assert.ok(nodes.has(e.dst), `edge dst ${e.dst} must be a node`);
    }
  }

  for (const t of tables(md)) {
    const headerCols = splitCols(t[0]).length;
    const sepCols = splitCols(t[1]).length;
    assert.equal(sepCols, headerCols, "separator column count should match header");
    for (const row of t.slice(2)) {
      assert.equal(splitCols(row).length, headerCols, "row column count should match header");
    }
    for (const cell of splitCols(t[1])) {
      assert.match(cell, /^-+$/, "separator cells should be dashes");
    }
  }

  for (const t of tables(md)) {
    const header = splitCols(t[0]);
    const ti = header.indexOf("Trigger");
    if (ti < 0) continue;
    for (const row of t.slice(2)) {
      const cols = splitCols(row);
      const cell = cols[ti];
      const toks = cell === "-" ? [] : cell.split(",").map((s) => s.trim()).filter(Boolean);
      for (const tok of toks) assert.ok(ALLOWED_TRIGGERS.has(tok), `unknown trigger token ${tok}`);
    }
  }

  const expectedOrphans = new Set(["detached", "detached-child", "foo-check", "bench::test"]);
  assert.deepEqual(docOrphans(md), expectedOrphans);
});
