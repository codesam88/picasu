// Step branch-completeness check.
//
// The proof's reachable set is defined by a step action's `any { ... }`
// branch list: an action declared in a reachable module but forgotten in the
// list silently narrows what `quint verify` and the MBT lane explore. This
// check derives the list of declared actions from the step module's import
// graph and fails for each one the branch list omits.
//
// Action universe. By default a step action must cover every declared action
// in its module's reachable graph. When several step actions share one module
// (a merged file), that union rule would let one step's list cover for
// another's. Such steps carry `// @step-universe A, B, C` (repeatable;
// consecutive lines merge) naming exactly the actions that step must cover.
// Then:
// - every name in the tag must be an action declared in the reachable graph
//   (`init`/`init<Name>` and step actions cannot be universe members: they
//   are entry points and schedulers, not branches);
// - every name in the tag must appear in that step's branch list (the same
//   missing-branch failure as the default rule);
// - every declared action in the file must appear in at least one step
//   universe of that file (an action in no universe fails: coverage is
//   opt-in per step, never silently dropped).
//
// Scope and limitations:
// - Only declarations matching `action <step...>: bool = any { ... }` are
//   treated as step actions; other `any`-valued declarations are ignored, and
//   a universe tag on anything else is an error.
// - Imports are resolved from `import M.* from "./file"` (whole module) and
//   `import M.name from "./file"` (that one action only). Module-qualified
//   references (`M.name(...)`) are not scanned; no current model uses them.
// - Excluded from the required set: `init` and `init<Name>` entry actions
//   (the `--init` operands of a run or verify invocation, not scheduler
//   branches) and any name matching the step pattern (`step`, `stepAspects`),
//   which are the step actions themselves rather than branches.
// - A missing `Skip`-style stutter branch is not reported: stutter is a
//   modeling policy, not a completeness fact.
// - Every reachable `action` declaration is required as a branch; a helper
//   action deliberately left out of the step must not be declared in a
//   reachable module.
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { resolveBase, fail } from "../lib/common.ts";
import { listQntFiles } from "../lib/qnt.ts";

interface ActionDecl {
  file: string;
  line: number;
}

interface UniverseTag {
  names: string[];
  line: number;
}

interface StepAction {
  file: string;
  name: string;
  line: number;
  branches: number;
  refs: Set<string>;
  universe: UniverseTag | null;
}

const STEP_RE =
  /^[ \t]*action[ \t]+(step\w*)[ \t]*:[ \t]*bool[ \t]*=[ \t]*any[ \t]*\{/gm;
const ACTION_RE = /^[ \t]*action[ \t]+([A-Za-z_]\w*)/gm;
const UNIVERSE_RE = /^\s*\/\/\s*@step-universe\b\s*(.*)$/;
const IMPORT_RE =
  /^\s*import\s+([A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)\.(\*|[A-Za-z_]\w*)\s+from\s+"([^"]+)"/gm;

function isStepName(name: string): boolean {
  return /^step\w*$/.test(name);
}

// `init` and `init<Name>` are run/verify entry points (an `--init` operand),
// not branches the scheduler may pick.
function isInitName(name: string): boolean {
  return /^init\w*$/.test(name);
}

// Replace comments (and, unless `keepStrings`, double-quoted strings) with
// spaces while preserving newlines and byte offsets, so regex matches keep
// their line numbers and braces inside strings/comments do not affect block
// matching. Imports are parsed with strings kept, since the path is quoted.
function strip(text: string, keepStrings: boolean): string {
  let out = "";
  let i = 0;
  while (i < text.length) {
    const c = text[i];
    const n = text[i + 1];
    if (c === "/" && n === "/") {
      while (i < text.length && text[i] !== "\n") {
        out += " ";
        i++;
      }
    } else if (c === "/" && n === "*") {
      out += "  ";
      i += 2;
      while (i < text.length && !(text[i] === "*" && text[i + 1] === "/")) {
        out += text[i] === "\n" ? "\n" : " ";
        i++;
      }
      if (i < text.length) {
        out += "  ";
        i += 2;
      }
    } else if (c === '"') {
      out += keepStrings ? c : " ";
      i++;
      while (i < text.length && text[i] !== '"') {
        if (text[i] === "\\") {
          out += keepStrings ? text.slice(i, i + 2) : "  ";
          i += 2;
          continue;
        }
        out += keepStrings ? text[i] : text[i] === "\n" ? "\n" : " ";
        i++;
      }
      if (i < text.length) {
        out += keepStrings ? '"' : " ";
        i++;
      }
    } else {
      out += c;
      i++;
    }
  }
  return out;
}

function stripNoise(text: string): string {
  return strip(text, false);
}

function lineAt(text: string, index: number): number {
  let line = 1;
  for (let i = 0; i < index && i < text.length; i++) {
    if (text[i] === "\n") line++;
  }
  return line;
}

// Body of the brace block opening at `open`, or null if it never closes.
function blockBody(text: string, open: number): string | null {
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    if (text[i] === "{") depth++;
    else if (text[i] === "}") {
      depth--;
      if (depth === 0) return text.slice(open + 1, i);
    }
  }
  return null;
}

// Branch references: names applied to arguments (`Foo("p1")`) and bare names
// (`Skip`). Arguments are string literals in these blocks, already blanked.
function branchReferences(body: string): { refs: string[]; count: number } {
  const refs: string[] = [];
  for (const m of body.matchAll(/\b([A-Za-z_]\w*)\s*\(/g)) refs.push(m[1]);
  for (const m of body.matchAll(/\b([A-Za-z_]\w*)\b(?!\s*\()/g))
    refs.push(m[1]);
  return { refs, count: refs.length };
}

export function run(args: string[]): number {
  const dirArg = args[0];
  if (!dirArg) {
    console.error("steps: missing required pipeline-dir argument");
    return 2;
  }
  const base = resolveBase(dirArg, "events.json");
  if (!existsSync(base)) {
    fail(`cannot read directory ${base}`);
    return 1;
  }

  let failed = false;
  const readText = (path: string): string | null => {
    try {
      return readFileSync(path, "utf8");
    } catch {
      fail(`cannot read ${path}`);
      failed = true;
      return null;
    }
  };
  const display = (path: string): string => relative(base, path);

  const resolveImport = (fromFile: string, spec: string): string => {
    const p = resolve(dirname(fromFile), spec);
    return p.endsWith(".qnt") ? p : `${p}.qnt`;
  };

  // Declared actions reachable from `startAbs`: the file itself plus every
  // module it imports, transitively. A single-symbol import contributes only
  // that named action and does not pull in the target's imports.
  const collectReachable = (startAbs: string): Map<string, ActionDecl> => {
    const found = new Map<string, ActionDecl>();
    const visited = new Set<string>();

    const addActions = (abs: string, only?: string): void => {
      const text = readText(abs);
      if (text === null) return;
      for (const m of stripNoise(text).matchAll(ACTION_RE)) {
        const name = m[1];
        if (only !== undefined && name !== only) continue;
        if (!found.has(name)) {
          found.set(name, {
            file: display(abs),
            line: lineAt(text, m.index ?? 0),
          });
        }
      }
    };

    const visit = (abs: string): void => {
      if (visited.has(abs)) return;
      visited.add(abs);
      addActions(abs);
      const text = readText(abs);
      if (text === null) return;
      for (const m of strip(text, true).matchAll(IMPORT_RE)) {
        const target = resolveImport(abs, m[3]);
        const leaf = m[2];
        if (leaf === "*") visit(target);
        else addActions(target, leaf);
      }
    };

    visit(startAbs);
    return found;
  };

  const failMsg = (msg: string): void => {
    fail(msg);
    failed = true;
  };

  // `@step-universe` tags attach to the action declaration that follows the
  // contiguous comment block, matching the annotation convention the specs
  // check uses. Returns the tags keyed by the declaration's 1-based line.
  const collectUniverseTags = (
    abs: string,
    text: string,
  ): Map<number, UniverseTag> => {
    const tags = new Map<number, UniverseTag>();
    const lines = text.split("\n");
    let pending: UniverseTag[] = [];
    const attach = (declLine: number, declText: string): void => {
      if (pending.length === 0) return;
      const decl = declText.match(/^[ \t]*action[ \t]+([A-Za-z_]\w*)/);
      if (!decl) {
        for (const p of pending)
          failMsg(
            `${display(abs)}:${p.line}: @step-universe must directly precede an action declaration`,
          );
      } else if (!isStepName(decl[1])) {
        for (const p of pending)
          failMsg(
            `${display(abs)}:${p.line}: @step-universe is only valid on a step action ('${decl[1]}' is not one)`,
          );
      } else {
        tags.set(declLine, {
          names: [...new Set(pending.flatMap((p) => p.names))],
          line: pending[0].line,
        });
      }
      pending = [];
    };
    for (let i = 0; i < lines.length; i++) {
      const line = lines[i];
      if (/^\s*\/\//.test(line)) {
        const m = line.match(UNIVERSE_RE);
        if (m)
          pending.push({
            names: m[1]
              .split(",")
              .map((s) => s.trim())
              .filter((s) => s.length > 0),
            line: i + 1,
          });
        continue;
      }
      if (line.trim() === "") {
        pending = [];
        continue;
      }
      attach(i + 1, line);
    }
    for (const p of pending)
      failMsg(
        `${display(abs)}:${p.line}: @step-universe must directly precede an action declaration`,
      );
    return tags;
  };

  const stepActions: StepAction[] = [];
  for (const f of listQntFiles(base)) {
    const abs = join(base, f);
    const text = readText(abs);
    if (text === null) continue;
    const stripped = stripNoise(text);
    const universeTags = collectUniverseTags(abs, text);
    const fileSteps: StepAction[] = [];
    for (const m of stripped.matchAll(STEP_RE)) {
      const line = lineAt(text, m.index ?? 0);
      const open = (m.index ?? 0) + m[0].length - 1;
      const body = blockBody(stripped, open);
      if (body === null) {
        failMsg(
          `${display(abs)}:${line}: step '${m[1]}' has an unbalanced \`any\` block`,
        );
        continue;
      }
      const { refs, count } = branchReferences(body);
      fileSteps.push({
        file: display(abs),
        name: m[1],
        line,
        branches: count,
        refs: new Set(refs),
        universe: universeTags.get(line) ?? null,
      });
    }
    for (const [declLine, tag] of universeTags) {
      if (fileSteps.some((sa) => sa.line === declLine)) continue;
      failMsg(
        `${display(abs)}:${tag.line}: @step-universe is on a step action without an \`any\` block`,
      );
    }
    stepActions.push(...fileSteps);
  }

  // Group by file: reachability and universe coverage are per module graph.
  const byFile = new Map<string, StepAction[]>();
  for (const sa of stepActions) {
    if (!byFile.has(sa.file)) byFile.set(sa.file, []);
    byFile.get(sa.file)!.push(sa);
  }

  let totalBranches = 0;
  for (const [file, actions] of byFile) {
    const reachable = collectReachable(join(base, file));
    const allRequired: [string, ActionDecl][] = [];
    for (const [name, decl] of reachable) {
      if (isInitName(name) || isStepName(name)) continue;
      allRequired.push([name, decl]);
    }

    const covered = new Set<string>();
    let untagged = false;
    for (const sa of actions) {
      if (sa.universe === null) {
        untagged = true;
        continue;
      }
      for (const name of sa.universe.names) {
        const decl = reachable.get(name);
        if (decl === undefined) {
          failMsg(
            `${file}:${sa.universe.line}: step '${sa.name}' universe names undeclared action '${name}'`,
          );
          continue;
        }
        if (isInitName(name) || isStepName(name)) {
          failMsg(
            `${file}:${sa.universe.line}: step '${sa.name}' universe names '${name}', which is an entry point or step action, not a branch`,
          );
          continue;
        }
        covered.add(name);
      }
    }
    if (!untagged) {
      for (const [name, decl] of allRequired) {
        if (covered.has(name)) continue;
        failMsg(
          `${file}: action '${name}' is not covered by any step universe (declared in ${decl.file}:${decl.line})`,
        );
      }
    }

    for (const sa of actions) {
      const required: [string, ActionDecl][] =
        sa.universe === null
          ? allRequired
          : sa.universe.names
              .filter((name) => reachable.has(name))
              .map((name) => [name, reachable.get(name)!]);
      for (const [name, decl] of required) {
        if (sa.refs.has(name)) continue;
        failMsg(
          `step '${sa.name}' in ${sa.file} is missing declared action '${name}' (declared in ${decl.file}:${decl.line})`,
        );
      }
      totalBranches += sa.branches;
      console.log(
        `steps-check: step '${sa.name}' in ${sa.file}: ${sa.branches} branches, ${required.length} ${sa.universe === null ? "declared" : "universe"} actions`,
      );
    }
  }

  if (failed) {
    console.error("steps-check: FAILED");
    return 1;
  }
  if (stepActions.length === 0) {
    console.log(`steps-check: OK (no step actions in ${base})`);
    return 0;
  }
  console.log(
    `steps-check: OK (${stepActions.length} step actions, ${totalBranches} branches)`,
  );
  return 0;
}
