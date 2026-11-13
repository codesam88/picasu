#!/usr/bin/env node
// Generate docs/quality-gates.md: an overview for documentation and a
// manual review aid for the validation gates. The diagrams show recipe
// wiring; the orphans list and per-group tables expose gaps such as
// recipes with no workflow, no caller, or duplicate coverage.
//
// The output is data-driven from `just --json` and the workflow files.
// Keep manual maps and filters to a minimum; where one is unavoidable,
// keep it narrow and close to the data it classifies.
//
// Every non-private recipe is tracked; CI steps that run check tooling
// outside a `just` recipe warn, and the committed doc must match the
// generated output.
//
// Usage: quality-gates.mjs [repo-root] [--generate <path>] [--check <path>] [--check-only]
//
// Check-only by default: verifies doc freshness
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
import { basename, dirname, join } from "node:path";

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

const workflowsDir = join(root, ".github", "workflows");
const docsDir = join(root, "docs");
const outPath = join(docsDir, "quality-gates.md");

const failures = [];
const warnings = [];

// ---------- recipe loading via `just --json` ----------
function loadRecipes() {
  let out;
  try {
    out = execFileSync("just", ["--json"], { cwd: root, encoding: "utf8" });
  } catch (e) {
    const detail = e.stderr ? e.stderr.toString().trim() : e.message;
    console.error(`Error: just --json failed in ${root}: ${detail}`);
    process.exit(1);
  }
  const json = JSON.parse(out);
  const recipes = [];
  function walk(modObj) {
    for (const r of Object.values(modObj.recipes || {})) {
      const name = r.namepath;
      const module = name.includes("::")
        ? name.slice(0, name.lastIndexOf("::"))
        : "";
      const topLevelModule = module ? module.split("::")[0] : "";
      let group = "";
      for (const a of r.attributes || []) {
        if (a && typeof a === "object" && a.group) {
          group = a.group;
          break;
        }
      }
      if (!group && module) group = topLevelModule;
      recipes.push({
        name,
        module,
        deps: [],
        rawDeps: r.dependencies || [],
        annotation: r.doc ?? "",
        group,
        private: !!r.private,
        body: r.body || [],
      });
    }
    for (const sub of Object.values(modObj.modules || {})) {
      walk(sub);
    }
  }
  walk(json);
  const byName = new Map(recipes.map((r) => [r.name, r]));
  for (const r of recipes) {
    const depsSet = new Set();
    for (const d of r.rawDeps) {
      const depName = d.recipe;
      if (depName.includes("::")) {
        if (byName.has(depName)) depsSet.add(depName);
      } else if (r.module) {
        const q = `${r.module}::${depName}`;
        if (byName.has(q)) depsSet.add(q);
        else if (byName.has(depName)) depsSet.add(depName);
      } else {
        if (byName.has(depName)) depsSet.add(depName);
      }
    }
    r.deps = [...depsSet].sort();
    delete r.rawDeps;
  }
  return recipes;
}

const recipes = loadRecipes();
const recipeByName = new Map(recipes.map((r) => [r.name, r]));

// Flatten `just --json` body fragments into plain command lines.
function flattenBody(r) {
  const out = [];
  for (const line of r.body || []) {
    if (Array.isArray(line)) {
      const text = line
        .map((part) =>
          Array.isArray(part) ? part.join("") : String(part ?? ""),
        )
        .join("");
      out.push(text);
    } else {
      out.push(String(line ?? ""));
    }
  }
  return out;
}

// ---------- trigger parsing ----------
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

function jobTriggerTokens(fileTriggers, jobIf) {
  if (!jobIf) return fileTriggers;
  const events = new Set(
    [...jobIf.matchAll(/github\.event_name\s*==\s*['"]([a-z_]+)['"]/g)].map(
      (m) => m[1],
    ),
  );
  if (!events.size) return fileTriggers;
  const tokens = [];
  if (events.has("pull_request") && fileTriggers.includes("PR")) tokens.push("PR");
  if (events.has("push")) {
    if (fileTriggers.includes("main")) tokens.push("main");
    if (fileTriggers.includes("tags")) tokens.push("tags");
  }
  if (events.has("schedule") && fileTriggers.includes("scheduled")) {
    tokens.push("scheduled");
  }
  return tokens;
}

// ---------- workflow parsing ----------
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

// { recipe, file, triggers } entries from workflow steps and the precommit body.
const directInvocations = [];
const workflowFiles = readdirSync(workflowsDir)
  .filter((n) => n.endsWith(".yml") || n.endsWith(".yaml"))
  .sort();

for (const wf of workflowFiles) {
  const file = `.github/workflows/${wf}`;
  const fullPath = join(workflowsDir, wf);
  const content = readFileSync(fullPath, "utf8");
  const lines = content.split("\n");
  const fileTriggers = triggerTokens(content);
  let inJobs = false;
  let currentJobIf = null;
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
        currentJobIf = null;
        i += 1;
        continue;
      }
      const ifm = line.match(/^    if:\s*(.+)$/);
      if (ifm) currentJobIf = ifm[1];
    }
    const runMatch = line.match(/^(\s*)(?:-\s*)?run:\s*(.*)$/);
    if (runMatch) {
      const indent = runMatch[1].length;
      const rest = runMatch[2].trim();
      let collected = [];
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
        i = j;
      } else {
        collected = [{ text: rest, lineno: i + 1 }];
        i += 1;
      }
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
      const triggers = jobTriggerTokens(fileTriggers, currentJobIf);
      for (const cl of collected) {
        const txt = cl.text;
        const re = /\bjust\s+([A-Za-z_][\w:-]*)/g;
        let m;
        while ((m = re.exec(txt)) !== null) {
          directInvocations.push({ recipe: m[1], file, triggers });
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
  for (const text of flattenBody(precommitRecipe)) {
    const re = /\bjust\s+([A-Za-z_][\w:-]*)/g;
    let m;
    while ((m = re.exec(text)) !== null) {
      directInvocations.push({
        recipe: m[1],
        file: "justfile",
        triggers: ["local"],
      });
    }
  }
}

// ---------- invocation propagation ----------
const recipeFiles = new Map();
const recipeTriggers = new Map();
function addFile(recipe, file) {
  if (file === "justfile") return;
  if (!recipeFiles.has(recipe)) recipeFiles.set(recipe, new Set());
  recipeFiles.get(recipe).add(file);
}
function addTriggers(recipe, triggers) {
  if (!triggers || triggers.length === 0) return;
  if (!recipeTriggers.has(recipe)) recipeTriggers.set(recipe, new Set());
  for (const trigger of triggers) recipeTriggers.get(recipe).add(trigger);
}
for (const inv of directInvocations) {
  addFile(inv.recipe, inv.file);
  addTriggers(inv.recipe, inv.triggers);
}
let changed = true;
while (changed) {
  changed = false;
  for (const r of recipes) {
    const files = recipeFiles.get(r.name);
    const triggers = recipeTriggers.get(r.name);
    if (!files && !triggers) continue;
    for (const dep of r.deps) {
      if (!recipeByName.has(dep)) continue;
      if (files && files.size) {
        if (!recipeFiles.has(dep)) recipeFiles.set(dep, new Set());
        const before = recipeFiles.get(dep).size;
        for (const f of files) recipeFiles.get(dep).add(f);
        if (recipeFiles.get(dep).size !== before) changed = true;
      }
      if (triggers && triggers.size) {
        if (!recipeTriggers.has(dep)) recipeTriggers.set(dep, new Set());
        const before = recipeTriggers.get(dep).size;
        for (const t of triggers) recipeTriggers.get(dep).add(t);
        if (recipeTriggers.get(dep).size !== before) changed = true;
      }
    }
  }
}
// A recipe inherits the files and trigger tokens of the recipes that call it.
// There is no reverse propagation; an aggregate's trigger is what actually
// invokes it (for `check`/`test`, the precommit hook).

// Policy notes rendered under the corresponding group table.
const localNotes = [
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
const noteByName = new Map(localNotes.map((e) => [e.name, e.reason]));

// ---------- display set ----------
const displayRecipes = recipes.filter((r) => !r.private);
const displaySet = new Set(displayRecipes.map((r) => r.name));

// ---------- displayed children ----------
function displayedChildren(name) {
  const r = recipeByName.get(name);
  if (!r) return [];
  return (r.deps || []).filter((d) => displaySet.has(d)).sort();
}

// ---------- trees ----------
function tree(rootName) {
  const nodes = new Set([rootName]);
  const edges = [];
  const queue = [rootName];
  while (queue.length) {
    const src = queue.shift();
    for (const child of displayedChildren(src)) {
      edges.push(`  ${src} --> ${child}`);
      if (!nodes.has(child)) {
        nodes.add(child);
        queue.push(child);
      }
    }
  }
  return { edges, nodes };
}
const checkTree = tree("check");
const testTree = tree("test");

// ---------- orphans (advisory) ----------
function closureFromRoots(rootNames) {
  const visited = new Set();
  const stack = rootNames.filter((name) => recipeByName.has(name));
  for (const name of stack) visited.add(name);
  while (stack.length) {
    const recipe = recipeByName.get(stack.pop());
    for (const dep of recipe.deps) {
      if (!recipeByName.has(dep) || visited.has(dep)) continue;
      visited.add(dep);
      stack.push(dep);
    }
  }
  return visited;
}
const globalRoots = ["precommit", "check", "test", "audit"];
const invocationRoots = [
  ...new Set(directInvocations.map((inv) => inv.recipe)),
];
const covered = closureFromRoots([...globalRoots, ...invocationRoots]);
const orphanCandidates = displayRecipes
  .filter((r) => !covered.has(r.name))
  .map((r) => r.name)
  .sort();

// ---------- group ordering ----------
const PREFERRED_GROUPS = ["backend", "frontend", "utils", "docs", "deps", "global"];
// normalize empty group -> Other
for (const r of displayRecipes) {
  if (!r.group) r.group = "Other";
}
const groupNames = new Set(displayRecipes.map((r) => r.group));
const otherGroups = [...groupNames]
  .filter((g) => !PREFERRED_GROUPS.includes(g) && g !== "Other")
  .sort();
const orderedGroups = [
  ...PREFERRED_GROUPS.filter((g) => groupNames.has(g)),
  ...otherGroups,
  ...(groupNames.has("Other") ? ["Other"] : []),
];

function capitalize(g) {
  return g ? g.charAt(0).toUpperCase() + g.slice(1) : g;
}

// ---------- render ----------
// Markdown table with prettier-compatible cell padding.
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

// Red cross with emoji presentation; the variation selector keeps the
// codepoint count equal to the rendered width so table padding stays aligned.
const CROSS = "\u274C\uFE0F";

const triggerOrder = ["local", "PR", "main", "tags", "scheduled"];
function checkTriggers(name) {
  const tokens = new Set(recipeTriggers.get(name) || []);
  if (name === "precommit") tokens.add("local");
  return [...tokens].sort(
    (a, b) => triggerOrder.indexOf(a) - triggerOrder.indexOf(b),
  );
}

// build doc
const docLines = [];
docLines.push("# Quality gates");
docLines.push("");
docLines.push(
  "Generated by `scripts/quality-gates.mjs` from the `justfile` and workflow files. Do not edit by hand.",
);
docLines.push("");
docLines.push("## Test and Check Flows");
docLines.push("");
docLines.push("### `just check`");
docLines.push("");
docLines.push("```mermaid");
docLines.push("graph LR", ...checkTree.edges);
docLines.push("```");
docLines.push("");
docLines.push("### `just test`");
docLines.push("");
docLines.push("```mermaid");
docLines.push("graph LR", ...testTree.edges);
docLines.push("```");
docLines.push("");

docLines.push(
  "## Orphans (not reachable from a global rule or CI workflow)",
);
docLines.push("");
if (orphanCandidates.length) {
  const half = Math.ceil(orphanCandidates.length / 2);
  const rows = [];
  for (let i = 0; i < half; i += 1) {
    rows.push([
      "`" + orphanCandidates[i] + "`",
      orphanCandidates[i + half] ? "`" + orphanCandidates[i + half] + "`" : "",
    ]);
  }
  docLines.push(...renderTable(["Orphan", "Orphan"], rows));
} else {
  docLines.push("None.");
}
docLines.push("");

docLines.push("## Rules by group");
docLines.push("");
docLines.push(
  "`Rule` names the justfile recipe, `CI Workflow` lists the workflow files that invoke it directly and, in parentheses, any caller that itself runs in CI, and `Trigger` lists what starts them (`local` is the pre-commit hook; the rest come from each workflow's `on:` block, narrowed by a job's `if:` when present, and `workflow_dispatch` is not listed because any workflow can expose it). A cross marks a recipe with no workflow or CI caller; a dash marks no trigger. Recipes are grouped by their justfile group.",
);
docLines.push("");
// Workflow files that invoke each recipe directly, for the CI Workflow column.
const directFiles = new Map();
for (const inv of directInvocations) {
  if (inv.file === "justfile") continue;
  if (!directFiles.has(inv.recipe)) directFiles.set(inv.recipe, new Set());
  directFiles.get(inv.recipe).add(inv.file);
}
function workflowLabel(file) {
  return basename(file).replace(/\.ya?ml$/, "");
}

const groupHeaders = ["Rule (justfile)", "Description", "CI Workflow", "Trigger"];

function mermaidId(name) {
  return name.replace(/::/g, "__").replace(/[^A-Za-z0-9_-]/g, "_");
}

// Group-local rule graph. Edges only between members of the group; nodes with
// no such edge are ranked below the connected component via one invisible link
// from the deepest sink.
function groupDiagram(members) {
  const memberSet = new Set(members);
  const edges = [];
  const connected = new Set();
  for (const src of members) {
    for (const dep of displayedChildren(src)) {
      if (!memberSet.has(dep)) continue;
      edges.push(`  ${mermaidId(src)} --> ${mermaidId(dep)}`);
      connected.add(src);
      connected.add(dep);
    }
  }
  const orphans = members.filter((n) => !connected.has(n));
  const rank = new Map(members.map((n) => [n, 0]));
  for (let round = 0; round < members.length; round += 1) {
    let changed = false;
    for (const src of members) {
      for (const dep of displayedChildren(src)) {
        if (!memberSet.has(dep)) continue;
        const next = (rank.get(src) ?? 0) + 1;
        if (next > (rank.get(dep) ?? 0)) {
          rank.set(dep, next);
          changed = true;
        }
      }
    }
    if (!changed) break;
  }
  const sinks = members.filter(
    (name) =>
      connected.has(name) &&
      !displayedChildren(name).some((dep) => memberSet.has(dep)),
  );
  const maxRank = sinks.length
    ? Math.max(...sinks.map((name) => rank.get(name) ?? 0))
    : 0;
  const anchor = sinks
    .filter((name) => (rank.get(name) ?? 0) === maxRank)
    .sort()[0];
  const lines = ["graph TD"];
  for (const name of members) {
    const id = mermaidId(name);
    if (id !== name) lines.push(`  ${id}["${name}"]`);
    else lines.push(`  ${name}`);
  }
  lines.push(...edges);
  if (anchor && orphans.length) {
    lines.push(
      `  ${mermaidId(anchor)} ~~~ ${orphans.map(mermaidId).join(" & ")}`,
    );
  }
  return lines;
}

for (const g of orderedGroups) {
  const heading = capitalize(g);
  docLines.push("### " + heading);
  docLines.push("");
  const members = displayRecipes
    .filter((r) => r.group === g)
    .map((r) => r.name)
    .sort();
  docLines.push("```mermaid");
  docLines.push(...groupDiagram(members));
  docLines.push("```");
  docLines.push("");
  const rows = members.map((name) => {
    const r = recipeByName.get(name);
    const direct = [...(directFiles.get(name) || [])]
      .map(workflowLabel)
      .sort();
    const callers = recipes
      .filter(
        (q) =>
          q.name !== name &&
          !q.private &&
          (q.deps || []).includes(name) &&
          (recipeFiles.get(q.name)?.size ?? 0) > 0,
      )
      .map((q) => q.name)
      .sort();
    const workflowParts = [
      ...direct,
      ...callers.map((caller) => `(${caller})`),
    ];
    const triggers = checkTriggers(name);
    return [
      "`" + name + "`",
      r && r.annotation ? r.annotation : "\u2014",
      workflowParts.length ? workflowParts.join(", ") : CROSS,
      triggers.length ? triggers.join(", ") : "-",
    ];
  });
  docLines.push(...renderTable(groupHeaders, rows));
  docLines.push("");
  for (const name of members) {
    const reason = noteByName.get(name);
    if (reason) docLines.push("Note: `" + name + "` - " + reason);
  }
  if (members.some((n) => noteByName.has(n))) docLines.push("");
}

let content = docLines.join("\n");
if (!content.endsWith("\n")) content += "\n";
content = content.replace(/\n{2,}$/, "\n");

// ---------- generate and/or drift-check ----------
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

for (const w of warnings) console.error(`gates-check: WARN: ${w}`);
if (orphanCandidates.length) {
  console.log("Orphan candidates (advisory):");
  for (const name of orphanCandidates) console.log(`  ${name}`);
} else {
  console.log("Orphan candidates: none");
}
if (failures.length) {
  for (const f of failures) console.error(`gates-check: FAIL: ${f}`);
  process.exit(1);
}
console.log("gates-check: OK");
process.exit(0);