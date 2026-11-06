#!/usr/bin/env node
// Verify the quality gates: every check/test recipe in the justfile is
// scheduled in a pipeline lane, CI runs no check/test tooling outside
// of a `just` recipe, and docs/quality-gates.md matches the generated
// output. Also (re)generates docs/quality-gates.md.
//
// Tracked universe (step 9): U = transitive closure of justfile deps from
// roots {precommit, test, check, audit} UNION the same closure from every
// `just X` invoked in any enabled workflow file. Display set D = (U ∩
// isGateRecipe-incl-exacts) ∪ {precommit, test, check, audit}. Helpers stay
// in U for reachability but are never sections/graph nodes/requirements.
//
// Lane mapping rule: a recipe's lane comes from (1) the workflows that
// invoke it directly and (2) the aggregates that reach it.
//
// Trigger labels: triggerTokens(wfText) scans the workflow's `on:` region
// for pull_request/PR, push+branches:main/main, tags:/tags, schedule/
// scheduled, workflow_dispatch/manual. laneTriggers[lane] unions tokens over
// its backing workflow files; laneLabel appends " (a+b+...)" when non-empty,
// tokens ordered PR, main, tags, scheduled, manual.
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
  statSync,
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
  if (/workflow_dispatch/.test(regionText)) tokens.push("manual");
  const order = ["PR", "main", "tags", "scheduled", "manual"];
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

// ---------- base url ----------
function baseUrl() {
  try {
    const out = execFileSync("git", ["remote", "get-url", "origin"], {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
    let m = out.match(/github\.com[:/](.+?)(?:\.git)?$/);
    if (m) {
      return `https://github.com/${m[1].replace(/\.git$/, "")}`;
    }
  } catch {
    // ignore
  }
  return "https://github.com/codesam88/picasu";
}
const base = baseUrl();
const blobBase = `${base}/blob/main`;

function link(path, start, end) {
  const url = `${blobBase}/${path}#L${start}-L${end}`;
  return `[${path}:${start}-${end}](${url})`;
}

// ---------- underlying paths ----------
function underlyingPaths(r) {
  const set = new Set();
  for (const bl of r.bodyLines) {
    const txt = bl.text;
    const tokens = txt.split(/[\s'"`()]+/);
    for (const t of tokens) {
      const p = t.replace(/[),]+$/, "").replace(/^\.\//, "");
      if (/\.(mjs|sh|py|md|yaml|yml|rs|toml|json)$/.test(p)) {
        const full = join(root, p);
        if (existsSync(full)) {
          try {
            if (statSync(full).isFile()) set.add(p);
          } catch (e) {
            // ignore
          }
        }
      }
    }
  }
  return [...set].sort();
}

// ---------- generate doc ----------
const gateRecipes = recipes.filter(isGateRecipe).sort((a, b) =>
  a.name < b.name ? -1 : 1,
);
const displaySet = new Set([
  ...gateRecipes.map((r) => r.name).filter((n) => U.has(n)),
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
  "precommit",
];
// Newcomer lanes (new jobs/files) join after the preferred order,
// sorted for determinism. Lane nodes are emitted only where edged.
const laneIds = [
  ...preferredLaneOrder,
  ...[...new Set(directInvocations.map((inv) => inv.lane))].sort().filter(
    (l) => !preferredLaneOrder.includes(l),
  ),
];
// Label rule: every node is labeled by how you invoke it. Recipe nodes:
// `just <name>`. Workflow lane nodes: `ci <short>`. The `precommit` lane
// is entered by running `just precommit` (the git hook runs exactly that),
// so it is labeled `just precommit`.
function laneDisplayBase(lane) {
  if (lane === "precommit") return "just precommit";
  if (lane.startsWith("ci-")) return `ci ${lane.slice(3)}`;
  return `ci ${lane}`;
}
function laneLabelFor(lane) {
  const toks = [...(laneTriggers.get(lane) || new Set())];
  const base = laneDisplayBase(lane);
  if (toks.length === 0) return base;
  return `${base} (${toks.join("+")})`;
}
const laneLabel = Object.fromEntries(laneIds.map((l) => [l, laneLabelFor(l)]));

// mermaid: left-to-right. Edges only (a) lane -> directly-invoked
// entry recipes and (b) recipe -> its justfile deps.
const mermaidLines = ["```mermaid", "graph LR"];
const edgeSet = new Set();
// (a) lane -> directly-invoked entry recipes (displayed recipes: gate
// recipes plus the check/test root aggregates)
const directPairs = new Set();
for (const inv of directInvocations) {
  if (!laneIds.includes(inv.lane)) continue;
  if (!recipeByName.has(inv.recipe)) continue;
  if (!displaySet.has(inv.recipe)) continue;
  const r = recipeByName.get(inv.recipe);
  const isEntry =
    isGateRecipe(r) || inv.recipe === "check" || inv.recipe === "test";
  if (!isEntry) continue;
  directPairs.add(`${inv.lane}::${inv.recipe}`);
}
for (const pair of directPairs) {
  const [lane, recipe] = pair.split("::");
  edgeSet.add(
    `  lane_${lane.replace(/-/g, "_")} --> ${recipe.replace(/-/g, "_")}["just ${recipe}"]`,
  );
}
// (b) recipe -> its justfile deps (displayed recipes that have lanes only)
for (const name of displaySet) {
  const r = recipeByName.get(name);
  if (!r) continue;
  if (!recipeLanes.has(name)) continue;
  for (const dep of r.deps) {
    if (!recipeByName.has(dep)) continue;
    if (!displaySet.has(dep)) continue;
    edgeSet.add(
      `  ${name.replace(/-/g, "_")} --> ${dep.replace(/-/g, "_")}["just ${dep}"]`,
    );
  }
}
// lane nodes: only those with at least one edge
const laneNodesWithEdges = new Set();
for (const e of edgeSet) {
  const mm = e.match(/^\s*(lane_[A-Za-z0-9_]+)\s*-->/);
  if (mm) laneNodesWithEdges.add(mm[1]);
}
for (const lane of laneIds) {
  const id = `lane_${lane.replace(/-/g, "_")}`;
  if (!laneNodesWithEdges.has(id)) continue;
  mermaidLines.push(`  ${id}["${laneLabel[lane]}"]`);
}
for (const e of [...edgeSet].sort()) mermaidLines.push(e);
mermaidLines.push("```");

const docLines = [];
docLines.push("# Quality gates");
docLines.push("");
docLines.push(
  "Generated by `scripts/quality-gates.mjs` from the `justfile` and workflow files. Do not edit by hand.",
);
docLines.push("");
docLines.push("## Lane graph");
docLines.push("");
docLines.push(...mermaidLines);
docLines.push("");
docLines.push("## Checks");
docLines.push("");

for (const r of gateRecipes) {
  const lanes = [...(recipeLanes.get(r.name) || new Set())].sort();
  docLines.push(`### \`${r.name}\``);
  docLines.push("");
  if (r.annotation) {
    docLines.push(`Annotation: ${r.annotation}`);
    docLines.push("");
  }
  docLines.push(
    `- Recipe: ${link("justfile", r.startLine, r.endLine)} \`justfile:${r.startLine}-${r.endLine}\``,
  );
  docLines.push(`- Lanes: ${lanes.length ? lanes.join(", ") : "(none)"}`);
  const note = allowlistByName.get(r.name);
  if (note) docLines.push(`  Note: ${note}`);
  // invocations
  const invs = directInvocations.filter((inv) => inv.recipe === r.name);
  if (invs.length) {
    docLines.push("- Invocations:");
    for (const inv of invs) {
      docLines.push(
        `  - ${link(inv.file, inv.startLine, inv.endLine)} \`${inv.file}:${inv.startLine}-${inv.endLine}\``,
      );
    }
  } else {
    docLines.push("- Invocations: (none direct; reached via aggregate)");
  }
  // underlying
  const ups = underlyingPaths(r);
  if (ups.length) {
    docLines.push("- Underlying:");
    for (const p of ups) {
      const lc = readFileSync(join(root, p), "utf8").split("\n").length;
      docLines.push(
        `  - ${link(p, 1, lc)} \`${p}:1-${lc}\``,
      );
    }
  }
  // required by (all recipes, including non-gate aggregates)
  const parents = recipes
    .filter((q) => q.deps.includes(r.name))
    .map((q) => q.name)
    .sort();
  docLines.push(
    `- Required by: ${parents.length ? parents.map((p) => `\`${p}\``).join(", ") : "(none - lane entry)"}`,
  );
  docLines.push("");
}

const content = docLines.join("\n");

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
