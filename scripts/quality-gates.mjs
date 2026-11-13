#!/usr/bin/env node
// Verify the quality gates: every check/test recipe in the justfile is
// scheduled in a pipeline lane, CI runs no check/test tooling outside
// of a `just` recipe, and docs/quality-gates.md matches the generated
// output. Also (re)generates docs/quality-gates.md.
//
// Tracked universe (step 9): U = transitive closure of justfile deps from
// roots {precommit, test, check, audit} UNION the same closure from every
// `just X` invoked in any enabled workflow file. Display set D = every
// isGateRecipe-incl-exacts recipe ∪ {precommit, test, check, audit}. Helpers
// stay in U for reachability but are never displayed.
//
// Lane mapping rule: a recipe's lane comes from (1) the workflows that
// invoke it directly and (2) the aggregates that reach it.
//
// Trigger labels: triggerTokens(wfText) scans the workflow's `on:` region
// for pull_request/PR, push+branches:main/main, tags:/tags, schedule/
// scheduled. laneTriggers[lane] unions tokens over its backing workflow
// files; tokens ordered PR, main, tags, scheduled. workflow_dispatch is not
// labeled: any workflow that declares it can be run by hand.
//
// Usage: quality-gates.mjs [repo-root] [--generate <path>] [--check <path>] [--check-only]
//
// Check-only by default: verifies lane scheduling and doc freshness
// without writing anything. --generate <path> (relative to the repo
// root, or absolute) explicitly writes the regenerated document,
// formatted with prettier on a best-effort basis. --check <path>
// diffs against that file instead of the committed doc. --check-only
// is the explicit form of the default (check, write nothing).
import { execFileSync } from "node:child_process";
import {
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { basename, dirname, join, relative } from "node:path";

const args = process.argv.slice(2);
function flagValue(flag) {
  const idx = args.indexOf(flag);
  if (idx < 0) return null;
  const val = args[idx + 1];
  if (!val || val.startsWith("--")) {
    console.error(`Error: gates-check ${flag} needs a <path>`);
    process.exit(1);
  }
  return val;
}
const generateArg = flagValue("--generate");
const checkArg = flagValue("--check");
const positional = args.filter(
  (a, i) =>
    !a.startsWith("--") &&
    args[i - 1] !== "--generate" &&
    args[i - 1] !== "--check",
);
const root = positional[0] ?? ".";

const justfilePath = join(root, "justfile");
const workflowsDir = join(root, ".github", "workflows");
const docsDir = join(root, "docs");
const outPath = join(docsDir, "quality-gates.md");

const failures = [];
const warnings = [];

// ---------- justfile parsing ----------
function parseDeps(s) {
  if (!s.trim()) return [];
  const tokens = [];
  let depth = 0;
  let cur = "";
  for (const ch of s) {
    if (ch === "(") {
      depth += 1;
      cur += ch;
    } else if (ch === ")") {
      depth -= 1;
      cur += ch;
    } else if (ch === " " && depth === 0) {
      if (cur.trim()) tokens.push(cur.trim());
      cur = "";
    } else {
      cur += ch;
    }
  }
  if (cur.trim()) tokens.push(cur.trim());
  return tokens.map((t) => {
    const m = t.match(/^\(([^)]*)\)/);
    const inner = m ? m[1].trim() : t;
    const nm = inner.match(/^([A-Za-z_][\w-]*)/);
    return nm ? nm[1] : t;
  });
}

function parseJustfile() {
  const content = readFileSync(justfilePath, "utf8");
  const lines = content.split("\n");
  const recipes = [];
  let comments = [];
  let attrs = [];
  let idx = 0;
  while (idx < lines.length) {
    const line = lines[idx];
    const trimmed = line.trim();
    if (trimmed === "") {
      comments = [];
      attrs = [];
      idx += 1;
      continue;
    }
    if (trimmed.startsWith("#")) {
      comments.push(trimmed);
      idx += 1;
      continue;
    }
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      attrs.push(trimmed);
      idx += 1;
      continue;
    }
    const colonIdx = trimmed.indexOf(":");
    if (colonIdx > 0 && /^[A-Za-z_][\w-]*/.test(trimmed)) {
      const before = trimmed.slice(0, colonIdx).trim();
      const nameMatch = before.match(/^([A-Za-z_][\w-]*)/);
      if (nameMatch) {
        const name = nameMatch[1];
        const params = before.slice(name.length).trim();
        const after = trimmed.slice(colonIdx + 1).trim();
        const deps = parseDeps(after);
        const annotation =
          comments.length > 0
            ? comments[comments.length - 1].replace(/^#\s?/, "")
            : "";
        const isPrivate = attrs.some((a) => a.includes("private"));
        const groupMatch = attrs
          .map((a) => a.match(/group\('([^']+)'\)/))
          .find(Boolean);
        const group = groupMatch ? groupMatch[1] : "";
        const startLine = idx + 1;
        let lastBody = idx;
        let j = idx + 1;
        while (j < lines.length) {
          const l = lines[j];
          if (l.trim() === "") {
            j += 1;
            continue;
          }
          if (!/^\s/.test(l)) break;
          lastBody = j;
          j += 1;
        }
        let end = lastBody + 1;
        if (end < startLine) end = startLine;
        const bodyLines = lines.slice(idx + 1, end).map((l, k) => ({
          text: l,
          lineno: idx + 2 + k,
        }));
        recipes.push({
          name,
          params,
          deps,
          annotation,
          startLine,
          endLine: end,
          bodyLines,
          private: isPrivate,
          group,
        });
        comments = [];
        attrs = [];
        idx = end;
        continue;
      }
    }
    comments = [];
    attrs = [];
    idx += 1;
  }
  return { content, lines, recipes };
}

const { recipes } = parseJustfile();
const recipeByName = new Map(recipes.map((r) => [r.name, r]));

function paramsAllowed(r) {
  const s = r.params.trim();
  if (s === "") return true;
  const tokens = s.split(/\s+/);
  for (const t of tokens) {
    if (t.startsWith("*")) return false;
    if (!t.includes("=")) return false;
  }
  return true;
}

function isGateRecipe(r) {
  if (!paramsAllowed(r)) return false;
  const name = r.name;
  // names matching -check/-test as substring (covers -test- infix)
  if (name.includes("-check")) return true;
  if (name.includes("-test")) return true;
  // suffixes
  if (name.endsWith("-lint")) return true;
  if (name.endsWith("-sanity")) return true;
  if (name.endsWith("-match")) return true;
  if (name.endsWith("-breakcheck")) return true;
  if (name.endsWith("-matrix")) return true;
  if (name.endsWith("-audit")) return true;
  // prefixes
  if (name.startsWith("fuzz-")) return true;
  if (name.startsWith("dylint-")) return true;
  // exacts
  if (name === "audit") return true;
  if (name === "frontend-vitest") return true;
  if (name === "frontend-playwright") return true;
  if (name === "openapi-routes-release") return true;
  return false;
}

// ---------- trigger parsing (step 9) ----------
function onRegionLines(lines) {
  const out = [];
  let inOn = false;
  for (const line of lines) {
    if (/^on:\s*$/.test(line.trim()) || /^"on":\s*$/.test(line.trim())) {
      inOn = true;
      continue;
    }
    if (!inOn) continue;
    if (line.trim() === "") {
      out.push(line);
      continue;
    }
    if (!/^\s/.test(line)) break;
    out.push(line);
  }
  return out;
}
function triggerTokens(text) {
  const region = onRegionLines(text.split("\n"));
  const regionText = region.join("\n");
  const tokens = [];
  if (/pull_request/.test(regionText)) tokens.push("PR");
  if (/push/.test(regionText)) {
    if (/branches:\s*\[?[^\]]*main/.test(regionText)) tokens.push("main");
  }
  if (/tags:/.test(regionText)) tokens.push("tags");
  if (/schedule/.test(regionText)) tokens.push("scheduled");
  const order = ["PR", "main", "tags", "scheduled"];
  return [...new Set(tokens)].sort((a, b) => order.indexOf(a) - order.indexOf(b));
}

// ---------- workflow parsing ----------
function laneForWorkflow(file, job) {
  const base = basename(file);
  if (base === "ci.yml") {
    // Open world: a new ci.yml job is its own lane, never silently dropped.
    return job ? `ci-${String(job).toLowerCase()}` : null;
  }
  if (base === "docs-check.yml") return "docs";
  if (base === "audit.yml") return "audit";
  if (base === "fuzz-advisory.yml") return "nightly";
  if (base === "msrv.yml") return "nightly";
  if (base === "release.yml") return "release";
  if (base === "zizmor.yml") {
    return job ? `ci-${String(job).toLowerCase()}` : null;
  }
  // Open world: any other workflow file is its own lane, named for the
  // file. New files are tracked instead of silently dropped.
  const m = base.match(/^(.+?)\.ya?ml$/);
  return m ? m[1] : null;
}

const suspiciousPatterns = [
  /clippy/i,
  /prettier/i,
  /eslint/i,
  /vue-tsc/i,
  /spectral/i,
  /oasdiff/i,
  /nextest/i,
  /playwright/i,
  /mdbook/i,
  /widdershins/i,
  /cargo\s+test/i,
  /cargo\s+deny/i,
  /cargo\s+dylint/i,
  /npm\s+audit/i,
  /fmt\s*.*--check/i,
];

function isSuspicious(line) {
  return suspiciousPatterns.some((re) => re.test(line));
}

function isBootstrap(line) {
  const l = line.toLowerCase();
  if (/\bdylint-lints\b/.test(l)) return true;
  if (/\bjust\b/.test(l)) return true;
  if (/\brustup\b/.test(l)) return true;
  if (/\bnpm\s+ci\b/.test(l)) return true;
  if (/\bcurl\b/.test(l)) return true;
  if (/\becho\b/.test(l)) return true;
  if (/\binstall\b/.test(l)) return true;
  if (/--version\b/.test(l)) return true;
  if (/\bcargo\s+build\b/.test(l)) return true;
  if (/\bnpm\s+run\s+build\b/.test(l)) return true;
  if (/\bnpm\s+install\b/.test(l)) return true;
  return false;
}

const directInvocations = []; // { recipe, lane, file, startLine, endLine }
const laneTriggers = new Map(); // lane -> Set<string>
const prTriggeredFiles = new Set();
const workflowFiles = readdirSync(workflowsDir)
  .filter((n) => n.endsWith(".yml") || n.endsWith(".yaml"))
  .sort();

for (const wf of workflowFiles) {
  const file = `.github/workflows/${wf}`;
  const fullPath = join(workflowsDir, wf);
  const content = readFileSync(fullPath, "utf8");
  const lines = content.split("\n");
  const fileTriggers = triggerTokens(content);
  if (fileTriggers.includes("PR")) prTriggeredFiles.add(file);
  let inJobs = false;
  let currentJob = null;
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (/^jobs:\s*$/.test(line.trim())) {
      inJobs = true;
      i += 1;
      continue;
    }
    if (/^[A-Za-z]/.test(line) && line.trim() !== "" && !line.startsWith(" ")) {
      if (line.trim() !== "jobs:") inJobs = false;
    }
    if (inJobs) {
      const jm = line.match(/^  ([A-Za-z_][\w-]*):\s*$/);
      if (jm) {
        currentJob = jm[1];
        i += 1;
        continue;
      }
    }
    const runMatch = line.match(/^(\s*)(?:-\s*)?run:\s*(.*)$/);
    if (runMatch) {
      const indent = runMatch[1].length;
      const rest = runMatch[2].trim();
      let collected = [];
      let startLine = i + 1;
      let endLine = i + 1;
      let j = i;
      if (
        rest === "" ||
        rest === "|" ||
        rest.startsWith("|") ||
        rest === ">" ||
        rest.startsWith(">")
      ) {
        j = i + 1;
        const tmp = [];
        while (j < lines.length) {
          const l = lines[j];
          if (l.trim() === "") {
            tmp.push({ text: l, lineno: j + 1 });
            j += 1;
            continue;
          }
          const lIndent = (l.match(/^ */)[0] || "").length;
          if (lIndent <= indent) break;
          tmp.push({ text: l, lineno: j + 1 });
          j += 1;
        }
        while (tmp.length && tmp[tmp.length - 1].text.trim() === "") tmp.pop();
        collected = tmp;
        endLine = collected.length
          ? collected[collected.length - 1].lineno
          : startLine;
        i = j;
      } else {
        collected = [{ text: rest, lineno: i + 1 }];
        endLine = i + 1;
        i += 1;
      }
      // (b) check tooling (warning only)
      for (const cl of collected) {
        const txt = cl.text.trim();
        if (!txt) continue;
        if (/\bjust\b/.test(txt)) continue;
        if (isSuspicious(txt) && !isBootstrap(txt)) {
          warnings.push(
            `${file}:${cl.lineno}: CI run block executes check/test tooling without a just recipe: ${txt}`,
          );
        }
      }
      // invocations
      for (const cl of collected) {
        const txt = cl.text;
        const re = /\bjust\s+([A-Za-z_][\w-]*)/g;
        let m;
        while ((m = re.exec(txt)) !== null) {
          const recipe = m[1];
          const lane = laneForWorkflow(file, currentJob);
          if (!lane) continue;
          if (!laneTriggers.has(lane)) laneTriggers.set(lane, new Set());
          for (const t of fileTriggers) laneTriggers.get(lane).add(t);
          directInvocations.push({
            recipe,
            lane,
            file,
            startLine,
            endLine,
          });
        }
      }
      continue;
    }
    i += 1;
  }
}

// precommit recipe invocations
const precommitRecipe = recipes.find((r) => r.name === "precommit");
if (precommitRecipe) {
  for (const bl of precommitRecipe.bodyLines) {
    const re = /\bjust\s+([A-Za-z_][\w-]*)/g;
    let m;
    while ((m = re.exec(bl.text)) !== null) {
      directInvocations.push({
        recipe: m[1],
        lane: "precommit",
        file: "justfile",
        startLine: bl.lineno,
        endLine: bl.lineno,
      });
    }
  }
}

// ---------- lane propagation ----------
const recipeLanes = new Map(); // name -> Set<string>
function addLane(recipe, lane) {
  if (!recipeLanes.has(recipe)) recipeLanes.set(recipe, new Set());
  recipeLanes.get(recipe).add(lane);
}
for (const inv of directInvocations) {
  addLane(inv.recipe, inv.lane);
}
// Forward propagation: a scheduled recipe's deps are scheduled in the same lanes.
let changed = true;
while (changed) {
  changed = false;
  for (const r of recipes) {
    if (!recipeLanes.has(r.name)) continue;
    const lanes = recipeLanes.get(r.name);
    for (const dep of r.deps) {
      if (!recipeByName.has(dep)) continue;
      if (!recipeLanes.has(dep)) {
        recipeLanes.set(dep, new Set(lanes));
        changed = true;
      } else {
        const before = recipeLanes.get(dep).size;
        for (const l of lanes) recipeLanes.get(dep).add(l);
        if (recipeLanes.get(dep).size !== before) changed = true;
      }
    }
  }
}
// Reverse propagation (single pass): an aggregate picks up the lanes of its
// wired deps, even if already scheduled (so frontend-test gains ci-frontend).
// Single pass avoids spreading lanes across siblings.
for (const r of recipes) {
  const wiredDeps = r.deps.filter(
    (d) => recipeLanes.has(d) && isGateRecipe(recipeByName.get(d)),
  );
  if (wiredDeps.length === 0) continue;
  if (!recipeLanes.has(r.name)) recipeLanes.set(r.name, new Set());
  for (const d of wiredDeps) {
    for (const l of recipeLanes.get(d)) recipeLanes.get(r.name).add(l);
  }
}


// ---------- tracked universe U (step 9) ----------
const aggregateRoots = ["precommit", "test", "check", "audit"];
function closureFromRoots(rootNames) {
  const visited = new Set();
  const stack = rootNames.filter((n) => recipeByName.has(n));
  for (const r of stack) visited.add(r);
  while (stack.length) {
    const cur = stack.pop();
    const rec = recipeByName.get(cur);
    if (!rec) continue;
    for (const dep of rec.deps) {
      if (!recipeByName.has(dep)) continue;
      if (visited.has(dep)) continue;
      visited.add(dep);
      stack.push(dep);
    }
  }
  return visited;
}
const workflowRoots = [
  ...new Set(
    directInvocations
      .filter((inv) => inv.file !== "justfile")
      .map((inv) => inv.recipe)
      .filter((n) => recipeByName.has(n)),
  ),
];
const U = closureFromRoots([...aggregateRoots, ...workflowRoots]);
const closureFromAggregates = closureFromRoots(aggregateRoots);
const reachableFromTest = closureFromRoots(["test"]);

const WORKFLOW_LANES = (() => {
  // Workflow-backed lanes: lanes of `just` invocations from workflow
  // files (the local precommit dispatch lane excluded). Dynamic so new
  // jobs/files count as CI automatically.
  return new Set(
    directInvocations
      .filter((inv) => inv.file !== "justfile")
      .map((inv) => inv.lane),
  );
})();

const locallyUnexercised = [
  {
    name: "dylint-check",
    reason:
      "needs nightly-2026-08-01 toolchain+rDylint; CI-only by decision 2026-11-05",
  },
  {
    name: "fuzz-check",
    reason:
      "CI-only tier-1 fuzz check not promoted into local `test` aggregate (step 3 pending)",
  },
];
const allowlistByName = new Map(locallyUnexercised.map((e) => [e.name, e.reason]));

// ---------- check (a): orphan (warning only) ----------
for (const r of recipes) {
  if (!isGateRecipe(r)) continue;
  if (!U.has(r.name)) {
    warnings.push(
      `justfile: recipe '${r.name}' (${r.startLine}) is an orphan (not in tracked universe U)`,
    );
  }
}

// ---------- check (b): tests not run in CI (warning only) ----------
for (const r of recipes) {
  if (!isGateRecipe(r)) continue;
  if (!reachableFromTest.has(r.name)) continue;
  const lanes = recipeLanes.get(r.name) || new Set();
  const hasWorkflowLane = [...lanes].some((l) => WORKFLOW_LANES.has(l));
  if (!hasWorkflowLane) {
    warnings.push(
      `justfile: recipe '${r.name}' (${r.startLine}) is a test reachable from 'test' but not run in CI (no workflow-backed lane)`,
    );
  }
}

// ---------- check (c): PR-triggered workflow recipes unreachable from aggregates (warning only) ----------
const prRecipes = new Set();
for (const inv of directInvocations) {
  if (!prTriggeredFiles.has(inv.file)) continue;
  if (!recipeByName.has(inv.recipe)) continue;
  prRecipes.add(inv.recipe);
}
for (const name of prRecipes) {
  if (!isGateRecipe(recipeByName.get(name))) continue;
  if (closureFromAggregates.has(name)) continue;
  if (allowlistByName.has(name)) continue;
  warnings.push(
    `justfile: recipe '${name}' is invoked by a PR-triggered workflow but unreachable from {precommit, test, check, audit}`,
  );
}

// ---------- generate doc ----------
const gateRecipes = recipes.filter(isGateRecipe).sort((a, b) =>
  a.name < b.name ? -1 : 1,
);
const displaySet = new Set([
  ...gateRecipes.map((r) => r.name),
  "precommit",
  "test",
  "check",
  "audit",
].filter((n) => recipeByName.has(n)));

const preferredLaneOrder = [
  "ci-backend",
  "ci-frontend",
  "ci-dylint",
  "ci-zizmor",
  "docs",
  "audit",
  "nightly",
  "release",
];
// Lanes table order: precommit first, then preferred CI order, then others.
const allLanes = new Set(directInvocations.map((inv) => inv.lane));
if (precommitRecipe) allLanes.add("precommit");
const laneOrder = [
  "precommit",
  ...preferredLaneOrder,
  ...[...allLanes].sort().filter(
    (l) => l !== "precommit" && !preferredLaneOrder.includes(l),
  ),
].filter((l) => allLanes.has(l));
// Drop lanes that invoke no displayed recipe (plus keep precommit).
const includedLanes = laneOrder.filter(
  (lane) =>
    lane === "precommit" ||
    directInvocations.some(
      (inv) => inv.lane === lane && displaySet.has(inv.recipe),
    ),
);

// Displayed children for the rule graph.
function displayedChildren(name) {
  const r = recipeByName.get(name);
  if (!r) return [];
  if (name === "precommit") {
    return ["check", "test"].filter((n) => recipeByName.has(n) && displaySet.has(n)).sort();
  }
  return (r.deps || []).filter((d) => displaySet.has(d)).sort();
}

// displayed recipes with no displayed parent
const childSet = new Set();
for (const name of displaySet) {
  for (const c of displayedChildren(name)) childSet.add(c);
}
// Displayed recipes that no other recipe calls and that call nothing.
const noCaller = [...displaySet].filter(
  (n) => !childSet.has(n) && displayedChildren(n).length === 0,
).sort();

// render rule graph as an adjacency list
const graphSources = [...displaySet].filter((n) => displayedChildren(n).length > 0);
const priority = ["precommit", "check", "test", "audit"];
const orderedSources = [
  ...priority.filter((s) => graphSources.includes(s)),
  ...graphSources.filter((s) => !priority.includes(s)).sort(),
];
const maxSrcWidth = orderedSources.reduce((m, s) => Math.max(m, s.length), 0);
const graphIndent = " ".repeat(maxSrcWidth + 4);
const graphLines = [];
for (const src of orderedSources) {
  const items = displayedChildren(src);
  const prefix = src.padEnd(maxSrcWidth) + " -> ";
  let cur = prefix;
  const out = [];
  for (const item of items) {
    const withItem = cur === prefix ? prefix + item : cur + ", " + item;
    if (withItem.length <= 100) {
      cur = withItem;
    } else {
      if (cur !== prefix) out.push(cur + ",");
      cur = graphIndent + item;
    }
  }
  if (cur !== prefix && cur.trim() !== "") out.push(cur);
  graphLines.push(...out);
}

// table helper (prettier-canonical)
function renderTable(headers, rows) {
  const widths = headers.map((h, i) =>
    Math.max(h.length, ...rows.map((r) => String(r[i] ?? "").length)),
  );
  const fmtRow = (cells) =>
    "| " + cells.map((c, i) => String(c).padEnd(widths[i])).join(" | ") + " |";
  const sep = "| " + widths.map((w) => "-".repeat(w)).join(" | ") + " |";
  const out = [fmtRow(headers), sep];
  for (const r of rows) out.push(fmtRow(r));
  return out;
}

// Lanes table rows
const laneRows = [];
for (const lane of includedLanes) {
  const triggers =
    lane === "precommit"
      ? "local"
      : [...(laneTriggers.get(lane) || new Set())].join(", ") || "\u2014";
  // distinct files
  const files = [
    ...new Set(directInvocations.filter((inv) => inv.lane === lane).map((inv) => basename(inv.file))),
  ];
  const workflowCell =
    lane === "precommit"
      ? "`justfile`"
      : files.length
      ? files.map((f) => "`" + f + "`").join(", ")
      : "\u2014";
  // directly invoked gate recipes
  let invokes = [];
  if (lane === "precommit") {
    invokes = ["check", "test"];
  } else {
    invokes = [
      ...new Set(
        directInvocations
          .filter((inv) => inv.lane === lane)
          .map((inv) => inv.recipe)
          .filter((name) => recipeByName.has(name) && isGateRecipe(recipeByName.get(name))),
      ),
    ].sort();
  }
  const invokesCell = invokes.length ? invokes.map((n) => "`" + n + "`").join(", ") : "\u2014";
  laneRows.push([lane, triggers, workflowCell, invokesCell]);
}

// Check families for the overview. Membership is curated; a gate recipe
// that matches no entry lands in "Other" with a warning.
const familyMap = [
  ["Backend (Rust)", ["backend-check", "backend-test", "backend-test-release", "utils-check", "utils-test", "msrv-check"]],
  ["Frontend", ["frontend-check", "frontend-vitest", "frontend-playwright", "frontend-test"]],
  ["API contract", ["openapi-check", "openapi-breakcheck", "openapi-json-match", "openapi-lint", "openapi-routes-match", "openapi-routes-release", "openapi-sanity", "auth-matrix-check"]],
  ["Security & supply chain", ["audit", "backend-audit", "frontend-audit", "zizmor-check", "dylint-check"]],
  ["Fuzzing", ["fuzz-check", "fuzz-matrix"]],
  ["Specs & catalogs", ["quint-check", "spec-check", "events-check", "catalog-check"]],
  ["Repo & docs hygiene", ["docs-check", "plan-lint", "env-var-check", "gates-check", "branches-prune-test"]],
];

const displayedGateNames = [...displaySet].filter((n) => {
  const r = recipeByName.get(n);
  return r && isGateRecipe(r);
});

// map recipe -> family
const recipeFamily = new Map();
for (const [fam, members] of familyMap) {
  for (const m of members) recipeFamily.set(m, fam);
}

const familySections = []; // {name, recipes:[]}
const otherRecipes = [];
for (const name of displayedGateNames) {
  if (recipeFamily.has(name)) {
    // ensure section exists
    if (!familySections.find((s) => s.name === recipeFamily.get(name))) {
      familySections.push({ name: recipeFamily.get(name), recipes: [] });
    }
    familySections.find((s) => s.name === recipeFamily.get(name)).recipes.push(name);
  } else {
    otherRecipes.push(name);
  }
}
// order families per familyMap, then Other
const orderedFamilies = familyMap
  .map(([fam]) => fam)
  .filter((fam) => familySections.some((s) => s.name === fam));
if (otherRecipes.length) orderedFamilies.push("Other");

// warning for unmapped
for (const name of otherRecipes.sort()) {
  warnings.push(`recipe '${name}' has no check family (add it to the family map in scripts/quality-gates.mjs)`);
}

// build doc
const docLines = [];
docLines.push("# Quality gates");
docLines.push("");
docLines.push(
  "Generated by `scripts/quality-gates.mjs` from the `justfile` and workflow files. Do not edit by hand.",
);
docLines.push("");
docLines.push("## Rule graph");
docLines.push("");
docLines.push("```text");
docLines.push(...graphLines);
docLines.push("```");
docLines.push("");
docLines.push(
  "`precommit` runs `check` and `test` on `main`; on other branches it runs a path-targeted subset of their members.",
);
docLines.push("");
if (noCaller.length) {
  docLines.push(
    "Checks no other recipe calls: " + noCaller.map((n) => "`" + n + "`").join(", ") + ".",
  );
  docLines.push("");
}

docLines.push("## Lanes");
docLines.push("");
docLines.push(
  "Workflows live under `.github/workflows/`. `Invokes` lists the recipes a lane calls directly (aggregate members are in the rule graph); triggers come from each workflow's `on:` block. `workflow_dispatch` is not listed because any workflow that declares it can be run by hand.",
);
docLines.push("");
docLines.push(...renderTable(["Lane", "Triggers", "Workflow", "Invokes"], laneRows));
docLines.push("");

docLines.push("## Check families");
docLines.push("");

for (const fam of orderedFamilies) {
  docLines.push("### " + fam);
  docLines.push("");
  const section = familySections.find((s) => s.name === fam);
  const members = fam === "Other" ? otherRecipes.sort() : (section ? section.recipes : []);
  // sort members by family map order if possible
  let orderedMembers = members;
  if (fam !== "Other") {
    const fm = familyMap.find(([f]) => f === fam);
    if (fm) {
      const orderIndex = new Map(fm[1].map((n, i) => [n, i]));
      orderedMembers = [...members].sort((a, b) => (orderIndex.get(a) ?? 999) - (orderIndex.get(b) ?? 999));
    }
  } else {
    orderedMembers = [...members].sort();
  }
  const rows = orderedMembers.map((name) => {
    const r = recipeByName.get(name);
    const desc = r && r.annotation ? r.annotation : "\u2014";
    const lanes = [...(recipeLanes.get(name) || new Set())].sort((a, b) => {
      const ia = laneOrder.indexOf(a);
      const ib = laneOrder.indexOf(b);
      if (ia !== -1 || ib !== -1) {
        if (ia === -1) return 1;
        if (ib === -1) return -1;
        return ia - ib;
      }
      return a < b ? -1 : 1;
    });
    const runsIn = lanes.length ? lanes.join(", ") : "\u2014";
    return ["`" + name + "`", desc, runsIn];
  });
  docLines.push(...renderTable(["Check", "Description", "Runs in"], rows));
  docLines.push("");
  // allowlist notes for this family
  for (const name of orderedMembers) {
    const reason = allowlistByName.get(name);
    if (reason) docLines.push("Note: `" + name + "` - " + reason);
  }
  if (orderedMembers.some((n) => allowlistByName.has(n))) docLines.push("");
}

let content = docLines.join("\n");
if (!content.endsWith("\n")) content += "\n";
content = content.replace(/\n{2,}$/, "\n");


// ---------- generate and/or drift-check ----------
// --generate <path>: write fresh output there (explicit generation).
// --check <path>: diff fresh output against that file instead of the
//   committed doc (explicit drift target).
// --check-only: check against the committed doc without writing
//   anything (same as the default; makes the mode explicit).
// Default (no flags): check-only against the committed doc.
const checkOnly = args.includes("--check-only");
if (generateArg && checkOnly) {
  console.error("Error: gates-check --generate and --check-only conflict");
  process.exit(1);
}
const checkPath =
  checkArg != null
    ? checkArg.startsWith("/")
      ? checkArg
      : join(root, checkArg)
    : outPath;
const checkDisplay =
  checkArg != null ? checkArg : join("docs", "quality-gates.md");
if (generateArg) {
  const target = generateArg.startsWith("/") ? generateArg : join(root, generateArg);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, content);
  formatFile(target);
  console.log(`gates-check: wrote ${generateArg}`);
}
// Skip the drift check when generation already covers it: lone
// --generate just writes (drift is checked separately), and when the
// generated file IS the check target the comparison is trivially true
// (the written file went through prettier, the in-memory content did not).
const genAbs =
  generateArg != null
    ? generateArg.startsWith("/")
      ? generateArg
      : join(root, generateArg)
    : null;
const skipDrift =
  generateArg != null && (checkArg == null || genAbs === checkPath);
if (!skipDrift) {
  let original = null;
  try {
    original = readFileSync(checkPath, "utf8");
  } catch {
    original = null;
  }
  if (original !== content) {
    failures.push(
      checkArg != null
        ? `${checkDisplay} differs from regenerated output. Run 'node scripts/quality-gates.mjs ${root} --generate ${checkArg}' and commit the result.`
        : `docs/quality-gates.md differs from regenerated output. Run 'just gates-docs' and commit the result.`,
    );
  }
}

// format with prettier if available
function formatFile(p) {
  const prettierBin = join(root, "frontend", "node_modules", ".bin", "prettier");
  try {
    if (existsSync(prettierBin)) {
      execFileSync(prettierBin, ["--write", p], { stdio: "ignore" });
      return;
    }
  } catch {
    // fall through
  }
  try {
    execFileSync("npx", ["--no-install", "prettier", "--write", p], {
      cwd: join(root, "frontend"),
      stdio: "ignore",
    });
  } catch {
    // formatting best-effort
  }
}

// ---------- report ----------
for (const w of warnings) console.error(`gates-check: WARN: ${w}`);
if (failures.length) {
  for (const f of failures) console.error(`gates-check: FAIL: ${f}`);
  process.exit(1);
}
console.log("gates-check: OK");
process.exit(0);
