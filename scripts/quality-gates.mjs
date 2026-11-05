#!/usr/bin/env node
// Verify the quality gates: every check/test recipe in the justfile is
// scheduled in a pipeline lane, CI runs no check/test tooling outside
// of a `just` recipe, and docs/quality-gates.md matches the generated
// output. Also (re)generates docs/quality-gates.md.
//
// Lane mapping rule: a recipe's lane comes from (1) the workflows that
// invoke it directly (precommit recipe, ci.yml, docs-check.yml, audit.yml,
// fuzz-advisory.yml, dylint job, release/docker/msrv/codeql/zizmor
// workflows as informational lanes) and (2) the aggregates (check, test,
// precommit, audit, openapi-check, frontend-test, ...) that reach it. A
// check/test recipe with no lane fails the gate.
//
// Usage: quality-gates.mjs [repo-root] [--generate <path>]
//
// Check-only by default: verifies lane scheduling and doc freshness
// without writing anything. --generate <path> (relative to the repo
// root, or absolute) explicitly writes the regenerated document,
// formatted with prettier on a best-effort basis.
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
const genIdx = args.indexOf("--generate");
const generateArg = genIdx >= 0 ? args[genIdx + 1] : null;
if (genIdx >= 0 && !generateArg) {
  console.error("gates-check: FAIL — --generate needs a <path>");
  process.exit(1);
}
const positional = args.filter((a, i) => !a.startsWith("--") && args[i - 1] !== "--generate");
const root = positional[0] ?? ".";

const justfilePath = join(root, "justfile");
const workflowsDir = join(root, ".github", "workflows");
const docsDir = join(root, "docs");
const outPath = join(docsDir, "quality-gates.md");

const failures = [];

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
        let end = idx;
        let j = idx + 1;
        while (j < lines.length) {
          const l = lines[j];
          if (l === "" || !/^\s/.test(l)) break;
          end = j + 1;
          j += 1;
        }
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
  return false;
}

// ---------- workflow parsing ----------
function fileLane(file, recipe) {
  const base = basename(file);
  if (base === "ci.yml") {
    const testLike =
      recipe.endsWith("-test") ||
      recipe === "frontend-vitest" ||
      recipe === "frontend-playwright" ||
      recipe === "fuzz-matrix";
    return testLike ? "pr-test" : "pr-check";
  }
  if (base === "docs-check.yml") return "pr-check";
  if (base === "audit.yml") return "audit";
  if (base === "fuzz-advisory.yml") return "nightly";
  if (base === "zizmor.yml") return "pr-check";
  if (base === "codeql.yml") return "main";
  if (base === "msrv.yml") return "nightly";
  if (base === "release.yml") return "release";
  if (base === "docker-build.yml") return "release";
  if (base === "dependency-review.yml") return "pr-check";
  return "pr-check";
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
const workflowFiles = readdirSync(workflowsDir)
  .filter((n) => n.endsWith(".yml") || n.endsWith(".yaml"))
  .sort();

for (const wf of workflowFiles) {
  const file = `.github/workflows/${wf}`;
  const fullPath = join(workflowsDir, wf);
  const content = readFileSync(fullPath, "utf8");
  const lines = content.split("\n");
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
      // (b) check tooling
      for (const cl of collected) {
        const txt = cl.text.trim();
        if (!txt) continue;
        if (/\bjust\b/.test(txt)) continue;
        if (isSuspicious(txt) && !isBootstrap(txt)) {
          failures.push(
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
          directInvocations.push({
            recipe,
            lane: fileLane(file, recipe),
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
        lane: "precommit-fast",
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
// Reverse propagation: an aggregate whose components are wired is itself
// scheduled (its lane is the union of its wired deps' lanes).
changed = true;
while (changed) {
  changed = false;
  for (const r of recipes) {
    if (recipeLanes.has(r.name)) continue;
    const wiredDeps = r.deps.filter((d) => recipeLanes.has(d));
    if (wiredDeps.length === 0) continue;
    const lanes = new Set();
    for (const d of wiredDeps) {
      for (const l of recipeLanes.get(d)) lanes.add(l);
    }
    recipeLanes.set(r.name, lanes);
    changed = true;
  }
}


// ---------- check (a) ----------
for (const r of recipes) {
  if (!isGateRecipe(r)) continue;
  const lanes = recipeLanes.get(r.name);
  if (!lanes || lanes.size === 0) {
    failures.push(
      `justfile: recipe '${r.name}' (${r.startLine}) is not invoked by any lane`,
    );
  }
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

const laneIds = [
  "precommit-fast",
  "pr-check",
  "pr-test",
  "main",
  "nightly",
  "release",
  "audit",
];
const laneLabel = {
  "precommit-fast": "precommit-fast",
  "pr-check": "pr-check",
  "pr-test": "pr-test",
  main: "main",
  nightly: "nightly / scheduled",
  release: "release",
  audit: "audit",
};

// mermaid
const mermaidLines = ["```mermaid", "graph LR"];
for (const lane of laneIds) {
  mermaidLines.push(`  ${lane.replace(/-/g, "_")}["${laneLabel[lane]}"]`);
}
const edgeSet = new Set();
for (const r of gateRecipes) {
  const lanes = recipeLanes.get(r.name) || new Set();
  for (const lane of lanes) {
    if (!laneIds.includes(lane)) continue;
    const edge = `  ${lane.replace(/-/g, "_")} --> ${r.name.replace(/-/g, "_")}["${r.name}"]`;
    edgeSet.add(edge);
  }
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
  docLines.push("");
}

const content = docLines.join("\n");

// ---------- generate or drift-check ----------
if (generateArg) {
  const target = generateArg.startsWith("/") ? generateArg : join(root, generateArg);
  mkdirSync(dirname(target), { recursive: true });
  writeFileSync(target, content);
  formatFile(target);
  console.log(`gates-check: wrote ${generateArg}`);
} else {
  let original = null;
  try {
    original = readFileSync(outPath, "utf8");
  } catch {
    original = null;
  }
  if (original !== content) {
    failures.push(
      `docs/quality-gates.md differs from regenerated output. Run 'just gates-docs' and commit the result.`,
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
if (failures.length) {
  for (const f of failures) console.error(`gates-check: FAIL — ${f}`);
  process.exit(1);
}
console.log("gates-check: OK");
process.exit(0);
