//! Reachability of panicking constructs from Rocket request parameters.
//!
//! The repo could already *enumerate* panic sites (clippy's restriction lints,
//! the `CodeQL` inventory recorded in `.plan/rust-taint-gate-evaluation.md`).
//! This module decides the question that matters for a request handler: **can
//! a value derived from a client-supplied parameter reach a panicking
//! construct?** It exists because of two historical bugs, both on `GET
//! /get/get-scroll-bar` / `GET /get/get-rows`:
//!
//! - `read_scrollbar` called
//!   `.expect("failed to read tree snapshot for scrollbar")` on a snapshot id
//!   that is a client-held query parameter, one function call deep through a
//!   `spawn_blocking` closure; an unknown (expired) id panicked into a 500.
//! - Both handlers bound `GuardResult<GuardTimestamp>` and discarded it with
//!   `let _ = auth;`, answering 200 to unauthenticated callers.
//!
//! The second bug is *not* a taint question and no taint analysis can see it:
//! there is no use to find. It is checked separately by the guard-propagation
//! check (see [`Report::guards`]).
//!
//! # What is analysed
//!
//! The input is source text: the files of the crate under analysis, passed to
//! [`analyze`] as `(path, content)` pairs. Like `ast_scan.rs`, the module is
//! pure — it never touches the file system and never prints; callers
//! (`build.rs`, the gate test in `src/tests/reachability.rs`) do the reading
//! and render the report.
//!
//! # Seeds: which handler parameters are client-supplied
//!
//! A function with a Rocket verb attribute (`#[get]`, `#[post]`, … — the same
//! detection `ast_scan.rs` applies) is a **route handler**. A handler's
//! parameter is a **seed** — the origin of a taint label — when its *type* is
//! client-supplied data:
//!
//! - the primitives Rocket extracts from the URI/query: `str`/`String`
//!   (also behind `&`), the integer types, `f64`, `bool`, `char`;
//! - request bodies: `Json<T>` and `&str`/`String`/`&[u8]` data;
//! - `Option<T>` / `Vec<T>` (nestable) of any of the above.
//!
//! The rule is deliberately **type-based**, mirroring the reasoning of
//! `CodeQL`'s own `Poem` model: the route attribute declares the route's
//! *shape*, while the parameter's *type* is what Rocket actually extracts
//! from the URI, query or body, and a type rule keeps seed identification
//! independent of how the attribute is written. (Attribute token trees are
//! unavailable to source-*external* tools — `CodeQL`'s extractor drops them,
//! see the evaluation in `.plan/rust-taint-gate-evaluation.md` — but even
//! with syn available here, the type remains the ground truth of provenance;
//! the attribute is used only to recognise that a function is a handler.)
//! Guard types (`GuardResult<…>`, `GuardX`) and framework types (`State`,
//! `Data`, …) are *not* seeds; guards are covered by the separate check.
//!
//! # Propagation
//!
//! Within a function body, taint flows through `let` bindings (including
//! rebinding and shadowing), method and function calls on tainted
//! receivers/arguments, field access, tuple/struct construction, references,
//! indexing, casts, arithmetic, `?`, `await`, and macro arguments (the
//! identifiers inside a macro invocation's token stream).
//!
//! A **slot** is one abstract local: the pair *(function, binding name)*.
//! Slots are name-keyed and monotone — taint only ever grows into a slot, and
//! a rebinding or a shadowing binding merges with the earlier taint instead of
//! killing it. That is the conservative direction (it can report a value as
//! tainted where a strong update would have cleared it, never the reverse)
//! and it is what makes ordering within a body irrelevant: the whole crate is
//! iterated to a fixpoint.
//!
//! A tainted value reaching a branch condition (`if`, `while`), a loop's
//! iteration source (`for`), or a `match` scrutinee taints every binding
//! *made in that branch* — control dependence is over-approximated by keeping
//! the branch's taint "ambient" while walking its body. Ambient taint also
//! feeds the panic macros (`panic!()`, `unreachable!()`, `todo!()`,
//! `unimplemented!()`), which have no tainted operand of their own; the
//! operand-based sinks (`unwrap`, `expect`, indexing) require a tainted
//! operand and are never reported from ambient taint alone.
//!
//! # Closures
//!
//! Closures are walked **inline in the defining function's scope**: a closure
//! body sees the enclosing slots, so capturing a tainted variable taints the
//! body — `move` included, since capture-by-move and capture-by-reference
//! name the same slot either way. When a closure is an argument of a call
//! (`tokio::task::spawn_blocking(..)`, `tokio::spawn(..)`,
//! `std::thread::spawn(..)`, rayon's `.map(..)`, any call), the closure is
//! analysed as if inlined at that point, the call's name is pushed onto the
//! reported path (`… -> spawn_blocking -> …`), and the closure's parameters
//! are conservatively seeded with the call's other-argument taint (so
//! `(start..end).map(|index| …)` taints `index`). The `get_rows` historical
//! panic was reachable only through such a closure.
//!
//! # Interprocedural propagation and the fixpoint
//!
//! A tainted value passed as an argument to a function defined in this crate
//! taints the callee's corresponding parameter; analysis continues in the
//! callee. Calls are resolved syntactically by path: `f(..)`, `Type::f(..)`,
//! `self.f(..)`, `Type::f(self, ..)` — free functions and inherent `impl`
//! methods only (see the limits below). Return flow uses a **per-function
//! summary**: the set of parameter indices whose taint can reach the
//! function's return value (tail expression, `return` operands and `?`
//! operands). Summary markers are seeded on every parameter and travel with
//! the ordinary taint; whichever markers reach a return point define the
//! summary. At a call site the return value is tainted by exactly those
//! arguments the callee's summary admits — more precise than unioning all
//! arguments, and still monotone.
//!
//! Everything — slots, summaries, findings, path strings — is iterated to a
//! fixpoint over the whole crate; taint only grows, so a bounded number of
//! passes suffices. [`ITERATION_LIMIT`] caps the passes; hitting the bound is
//! reported as [`Error::IterationLimit`] (an *error*, not a silent
//! truncation) so the gate fails rather than passing quietly on incomplete
//! results.
//!
//! # Sinks
//!
//! [`SinkKind`] — `unwrap()`, `expect(..)`, `panic!`, `unreachable!`,
//! `todo!`, `unimplemented!`, and indexing `v[i]` where the **index** (not
//! the collection) is tainted: a tainted index is a panic, a tainted
//! collection is a data question. For each (sink, seed) pair the report
//! carries the file, line, sink kind, the enclosing function, the seed, and
//! a short human-readable path string, e.g.
//! `get_rows(index) -> spawn_blocking -> TreeSnapshot::read_row -> …`.
//!
//! # The guard-propagation check
//!
//! A handler that binds a guard parameter must *use* it: `let _ = auth?;`
//! (propagated), a match/inspection — anything but letting the value die.
//! Three shapes are reported: `let _ = <guard>` without `?`, a guard binding
//! never read again, and a `let _ = …` of a guard-typed value.
//!
//! This check exists because it is exactly the shape of the `get-rows` /
//! `get-scroll-bar` bug: a `GuardResult<GuardTimestamp>` was bound and thrown
//! away with `let _ = auth;`, and **no taint analysis can see a use that is
//! not there** — the guard's error is never propagated, so nothing downstream
//! ever observes it. The taint pass would correctly report nothing; this pass
//! reports the missing use instead.
//!
//! # The registry gate
//!
//! [`parse_registry`] / [`gate_diff`] implement the review mechanism in
//! `backend/reachability-registry.txt`: every known site carries a reason,
//! and the gate fails on a **new** site (analysis finding with no registry
//! entry) and on a **stale** entry (registry entry whose site no longer
//! exists). The registry key is
//! `kind \t file \t function \t subject \t snippet` — deliberately no line
//! number: line numbers drift on any edit above the site and would produce
//! review churn without adding information, while enclosing function + kind
//! and the sink's own source line identify the site stably. A moved site
//! therefore still matches (preferred over new+stale churn); a renamed
//! function, a changed snippet or a changed subject (seed, guard binding) is
//! a reviewable change and fails as new+stale.
//!
//! # Limits — a clean report is not a proof of absence
//!
//! - **No flow through trait objects / `dyn`.** A call through a `dyn Trait`
//!   receiver resolves to nothing; arguments are unioned conservatively but
//!   the concrete implementation's body is never entered, so a sink inside it
//!   is not attributed to the seed.
//! - **Calls through function parameters resolve to nothing.** Passing a
//!   closure or `fn` value around and invoking it later is not modelled; the
//!   closure is still analysed at its definition site.
//! - **Unresolved `&mut`/`&self` methods.** When a method call cannot be
//!   resolved to an inherent method in this crate, the return value is the
//!   union of receiver and arguments, but side effects (an out-parameter
//!   re-tainted through `&mut`) are not modelled: the caller's variable is
//!   not re-tainted by what the callee wrote into it.
//! - **Generics are resolved syntactically.** No monomorphisation and no
//!   trait-bound resolution: a call is matched by path and parameter *index*,
//!   type parameters are ignored, `impl Trait` and associated types carry no
//!   taint of their own.
//! - **Loops are a fixpoint over-approximation.** Zero-kill monotone slots
//!   mean a loop that clears a binding in one iteration still carries its
//!   taint; bindings under a tainted iteration source are tainted.
//! - **Macro-generated code is not analysed.** Derive/attribute-macro bodies
//!   never become AST nodes (the same limitation documented in
//!   `ast_scan.rs`); only macro *invocations* are seen, and taint inside them
//!   is read off the identifiers in the token stream.
//! - **Control-dependence stops at summaries.** A parameter that only guards
//!   a branch taints the bindings in that branch, but it does not taint the
//!   function's *return* through the summary unless a value carrying it
//!   reaches a return point.
//! - **Trait impl methods are not indexed** (only free functions and
//!   inherent `impl` methods, per the resolution rules); sinks in trait-impl
//!   bodies behind an unresolved call are missed.
//! - **Slots are name-keyed**: shadowing merges taint (reported more, never
//!   less), and field access is field-insensitive — one tainted field taints
//!   the whole struct value.
//! - **Sinks require a seed.** A panic with neither a tainted operand nor a
//!   tainted branch context is not reported, and unreachable code is not
//!   reported at all. Two sites whose `kind` and `function` and source line
//!   collide in the registry key are reported as one.
//!
//! In short: the analysis over-approximates where it can and
//! under-approximates where resolution fails; a clean report means *no path
//! was found*, not that none exists.

use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::{Span, TokenStream, TokenTree};
use syn::visit::Visit;
use syn::{
    Attribute, BinOp, Block, Expr, ExprCall, ExprClosure, ExprMethodCall, FnArg, GenericArgument,
    Ident, ImplItem, Item, ItemFn, ItemImpl, Pat, PatType, Signature, Stmt, Type,
};

/// Rocket HTTP-method attributes that mark a function as a route handler.
/// Mirrors `ast_scan.rs`'s `VERB_ATTRIBUTES`; the router-coverage tests pin
/// that list, so the two cannot drift silently.
const VERB_ATTRIBUTES: [&str; 7] = ["get", "post", "put", "delete", "patch", "head", "options"];

/// Primitive types Rocket extracts from the URI/query — part of the
/// seed-type rule. `String` is a path and listed separately.
const CLIENT_PRIMITIVES: [&str; 14] = [
    "str", "i8", "i16", "i32", "i64", "isize", "u8", "u16", "u32", "u64", "usize", "f64", "bool",
    "char",
];

/// Maximum number of whole-crate fixpoint passes. Taint, summaries and path
/// strings all grow monotonically into finite sets, so convergence is
/// expected in a handful of passes; the bound exists so a modelling mistake
/// (facts that keep growing) surfaces as [`Error::IterationLimit`] instead of
/// hanging the build.
pub const ITERATION_LIMIT: usize = 64;

/// A panicking (or out-of-bounds-indexing) construct that tainted data can
/// reach.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SinkKind {
    /// `.unwrap()` on a tainted value.
    Unwrap,
    /// `.expect(..)` on a tainted value.
    Expect,
    /// `panic!(..)` with a tainted argument or under a tainted branch.
    Panic,
    /// `unreachable!(..)` with a tainted argument or under a tainted branch.
    Unreachable,
    /// `todo!(..)` with a tainted argument or under a tainted branch.
    Todo,
    /// `unimplemented!(..)` with a tainted argument or under a tainted branch.
    Unimplemented,
    /// `v[i]` where the *index* `i` is tainted (a tainted collection alone
    /// is not this sink).
    Index,
}

impl SinkKind {
    /// The registry `kind` string for this sink kind.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            SinkKind::Unwrap => "taint-unwrap",
            SinkKind::Expect => "taint-expect",
            SinkKind::Panic => "taint-panic",
            SinkKind::Unreachable => "taint-unreachable",
            SinkKind::Todo => "taint-todo",
            SinkKind::Unimplemented => "taint-unimplemented",
            SinkKind::Index => "taint-index",
        }
    }
}

/// One (sink, seed) pair: a panicking construct reached by taint from a
/// request parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkFinding {
    /// Source file, as passed to [`analyze`] (e.g. `src/storage/cache.rs`).
    pub file: String,
    /// 1-based line of the sink itself (the method ident, the macro path, or
    /// the opening bracket of an index expression).
    pub line: usize,
    /// Which kind of panicking construct this is.
    pub kind: SinkKind,
    /// Enclosing function, e.g. `TreeSnapshot::read_scrollbar`.
    pub function: String,
    /// The seeding request parameter, e.g. `get_scroll_bar(timestamp)`.
    pub seed: String,
    /// Human-readable path from the handler to the sink, e.g.
    /// `get_scroll_bar(timestamp) -> TreeSnapshot::read_scrollbar -> .expect(..)`.
    pub path: String,
    /// The sink's source line, trimmed — part of the registry key.
    pub snippet: String,
}

impl SinkFinding {
    /// Registry key: `kind \t file \t function \t seed \t snippet`. Stable
    /// across edits above the site (no line number); see the module docs.
    #[must_use]
    pub fn registry_key(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.kind.as_str(),
            self.file,
            self.function,
            self.seed,
            self.snippet
        )
    }

    /// The finding rendered as a registry line, with `reason` appended.
    #[must_use]
    pub fn registry_line(&self, reason: &str) -> String {
        format!("{}\t{reason}", self.registry_key())
    }
}

/// Why a guard binding was reported by the guard-propagation check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardKind {
    /// The binding appears nowhere in the handler body.
    NeverRead,
    /// Every occurrence sits in a `let _ = …` right-hand side that contains
    /// no `?`, so the guard's result is discarded unpropagated.
    Discarded,
}

impl GuardKind {
    /// The registry `kind` string for this guard finding.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            GuardKind::NeverRead => "guard-never-read",
            GuardKind::Discarded => "guard-discarded",
        }
    }
}

/// A guard parameter that is bound but never propagated or inspected — the
/// shape of the `let _ = auth;` bug. Distinct from [`SinkFinding`]: it
/// reports a *missing* use, not a reachable sink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardFinding {
    /// Source file, as passed to [`analyze`].
    pub file: String,
    /// 1-based line of the parameter (never-read) or of the `let _` (discarded).
    pub line: usize,
    /// The handler the guard belongs to.
    pub handler: String,
    /// The guard type as written, e.g. `GuardResult<GuardTimestamp>`.
    pub guard_type: String,
    /// The binding name, e.g. `auth`.
    pub binding: String,
    /// Which shape was detected.
    pub kind: GuardKind,
    /// The relevant source line, trimmed — part of the registry key.
    pub snippet: String,
}

impl GuardFinding {
    /// Registry key: `kind \t file \t handler \t binding:guard_type \t snippet`.
    #[must_use]
    pub fn registry_key(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}:{}\t{}",
            self.kind.as_str(),
            self.file,
            self.handler,
            self.binding,
            self.guard_type,
            self.snippet
        )
    }

    /// The finding rendered as a registry line, with `reason` appended.
    #[must_use]
    pub fn registry_line(&self, reason: &str) -> String {
        format!("{}\t{reason}", self.registry_key())
    }

    /// The message a caller would print for this finding. Names the handler,
    /// the guard type and the `file:line`, as the gate's output requires.
    #[must_use]
    pub fn message(&self) -> String {
        match self.kind {
            GuardKind::NeverRead => format!(
                "{}:{}: handler {} binds guard {}: {} but never reads it — the guard's \
                 result is never propagated or inspected",
                self.file, self.line, self.handler, self.binding, self.guard_type
            ),
            GuardKind::Discarded => format!(
                "{}:{}: handler {} discards guard {}: {} with `let _ = …` (no `?`) — the \
                 guard's check never runs; propagate it (`let _ = {}?;`) or inspect the value",
                self.file, self.line, self.handler, self.binding, self.guard_type, self.binding
            ),
        }
    }
}

/// A problem that makes the analysis incomplete or the registry malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A source file could not be parsed and was skipped entirely.
    UnparsableFile {
        /// File that failed to parse.
        file: String,
        /// Parser error as reported by `syn`.
        error: String,
    },
    /// The fixpoint did not converge within [`ITERATION_LIMIT`] passes. The
    /// findings are then incomplete by construction and the gate must fail —
    /// truncating silently would turn an analysis bug into a false green.
    IterationLimit {
        /// The bound that was hit.
        iterations: usize,
    },
    /// A registry file line is malformed (or duplicated).
    RegistryEntry {
        /// 1-based line in the registry text.
        line: usize,
        /// What is wrong with the line.
        error: String,
    },
}

impl Error {
    /// The message a caller would print for this error.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Error::UnparsableFile { file, error } => format!("failed to parse {file}: {error}"),
            Error::IterationLimit { iterations } => format!(
                "reachability analysis did not converge within {iterations} iterations — \
                 findings are incomplete; this is an analysis bug, not a code finding"
            ),
            Error::RegistryEntry { line, error } => {
                format!("reachability registry line {line}: {error}")
            }
        }
    }
}

/// Everything [`analyze`] found.
#[derive(Debug, Default, Clone)]
pub struct Report {
    /// (sink, seed) pairs, sorted by file, line, kind, seed.
    pub sinks: Vec<SinkFinding>,
    /// Guards bound but not propagated, sorted by file, line.
    pub guards: Vec<GuardFinding>,
    /// Parse failures and analysis errors. The gate fails on any error.
    pub errors: Vec<Error>,
}

// ── Program model ────────────────────────────────────────────────────────────

type FuncId = usize;
type SeedId = usize;

/// One taint label source: a handler parameter.
struct Seed {
    handler: FuncId,
    param: String,
}

/// A guard-typed parameter of a route handler.
struct GuardParam {
    binding: String,
    /// The type as written, e.g. `GuardResult<GuardTimestamp>`.
    ty: String,
    /// 1-based line of the parameter binding.
    line: usize,
}

/// One function (free or inherent-`impl` method) with its body.
struct Func {
    /// Index into [`Program::files`].
    file: usize,
    /// Display name: `get_rows` or `TreeSnapshot::read_scrollbar`.
    display: String,
    /// The enclosing `impl` block's type name, for `self.f(..)`/`Self::f(..)`.
    self_ty: Option<String>,
    /// Parameter binding names grouped per argument, `self` first for
    /// methods — one entry per `FnArg`, so call-site argument `i` maps onto
    /// entry `i` (a compound pattern binds several names under one index).
    params: Vec<Vec<String>>,
    /// Parameters whose type is client-supplied data (handlers only).
    seed_params: Vec<String>,
    /// Guard-typed parameters (handlers only).
    guard_params: Vec<GuardParam>,
    /// Whether this function carries a Rocket verb attribute.
    is_handler: bool,
    body: Block,
}

/// The whole crate: parsed functions plus the resolution indexes.
struct Program {
    /// File paths as given by the caller.
    files: Vec<String>,
    /// One line per file (1-based line numbers index into this).
    lines: Vec<Vec<String>>,
    funcs: Vec<Func>,
    /// Free functions by bare name.
    by_name: BTreeMap<String, Vec<FuncId>>,
    /// Inherent methods by method name (any self type).
    methods: BTreeMap<String, Vec<FuncId>>,
    /// Inherent methods by `(self type name, method name)`.
    methods_of_ty: BTreeMap<(String, String), Vec<FuncId>>,
    /// Seeds, in creation order.
    seeds: Vec<Seed>,
}

impl Program {
    /// The report display of a seed: `get_scroll_bar(timestamp)`.
    fn seed_display(&self, seed: SeedId) -> String {
        let seed = &self.seeds[seed];
        format!("{}({})", self.funcs[seed.handler].display, seed.param)
    }

    /// The trimmed source line at `line` (empty when out of range),
    /// with tabs replaced by spaces and truncated to 160 characters, so the
    /// tab-separated registry format cannot be confused by a source line.
    fn snippet(&self, file: usize, line: usize) -> String {
        self.lines
            .get(file)
            .and_then(|lines| lines.get(line.wrapping_sub(1)))
            .map_or_else(String::new, |text| {
                let trimmed = text.trim().replace('\t', " ");
                if trimmed.chars().count() > 160 {
                    let cut: String = trimmed.chars().take(160).collect();
                    format!("{cut}…")
                } else {
                    trimmed
                }
            })
    }
}

// ── Taint facts ──────────────────────────────────────────────────────────────

/// The taint of one expression or slot: which seeds reach it, plus which
/// *summary markers* (parameter indices of the enclosing function) reach it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Taint {
    seeds: BTreeSet<SeedId>,
    markers: BTreeSet<usize>,
}

impl Taint {
    fn is_empty(&self) -> bool {
        self.seeds.is_empty() && self.markers.is_empty()
    }

    fn union_with(&mut self, other: &Taint) {
        self.seeds.extend(other.seeds.iter().copied());
        self.markers.extend(other.markers.iter().copied());
    }

    fn union(mut self, other: &Taint) -> Self {
        self.union_with(other);
        self
    }

    fn from_seed(seed: SeedId) -> Self {
        Taint {
            seeds: BTreeSet::from([seed]),
            markers: BTreeSet::new(),
        }
    }
}

/// All mutable fixpoint state. Every mutation goes through a method that
/// flags `changed`, which is how the driver knows to run another pass.
struct State {
    changed: bool,
    /// Slot → seeds. Key `(function, binding name)`.
    slot_seeds: BTreeMap<(FuncId, String), BTreeSet<SeedId>>,
    /// Slot → summary markers. Key `(function, binding name)`.
    slot_markers: BTreeMap<(FuncId, String), BTreeSet<usize>>,
    /// How a seed entered a function: the frames from the handler to this
    /// function, kept shortest-first (strictly-shorter updates only, so each
    /// entry can only decrease and the fixpoint terminates).
    chains: BTreeMap<(FuncId, SeedId), Vec<String>>,
    /// Per-function summary: parameter indices whose taint reaches a return.
    summary: BTreeMap<FuncId, BTreeSet<usize>>,
    /// Findings by registry key; a re-discovery keeps the lexicographically
    /// smallest path so the output is independent of pass order.
    sinks: BTreeMap<String, SinkFinding>,
}

impl State {
    fn new() -> Self {
        State {
            changed: false,
            slot_seeds: BTreeMap::new(),
            slot_markers: BTreeMap::new(),
            chains: BTreeMap::new(),
            summary: BTreeMap::new(),
            sinks: BTreeMap::new(),
        }
    }

    /// The taint currently on the named slot of `func`.
    fn slot(&self, func: FuncId, name: &str) -> Taint {
        let key = (func, name.to_string());
        Taint {
            seeds: self.slot_seeds.get(&key).cloned().unwrap_or_default(),
            markers: self.slot_markers.get(&key).cloned().unwrap_or_default(),
        }
    }

    /// Merge `taint` into a slot; flags `changed` on growth.
    fn add_to_slot(&mut self, func: FuncId, name: &str, taint: &Taint) {
        let key = (func, name.to_string());
        let seeds = self.slot_seeds.entry(key.clone()).or_default();
        let before = seeds.len();
        seeds.extend(taint.seeds.iter().copied());
        let seeds_grew = seeds.len() != before;
        let markers = self.slot_markers.entry(key).or_default();
        let before = markers.len();
        markers.extend(taint.markers.iter().copied());
        if seeds_grew || markers.len() != before {
            self.changed = true;
        }
    }

    /// The frames from the handler to `func` for `seed` (empty for the
    /// handler itself).
    fn chain(&self, func: FuncId, seed: SeedId) -> Vec<String> {
        self.chains.get(&(func, seed)).cloned().unwrap_or_default()
    }

    /// Record how a seed entered a function; keeps the shortest path, with a
    /// lexicographic tie-break so pass order cannot influence the output.
    fn shorten_chain(&mut self, func: FuncId, seed: SeedId, candidate: Vec<String>) {
        match self.chains.get(&(func, seed)) {
            None => {
                self.chains.insert((func, seed), candidate);
                self.changed = true;
            }
            Some(current) => {
                let shorter = candidate.len() < current.len();
                let tied = candidate.len() == current.len() && candidate < *current;
                if shorter || tied {
                    self.chains.insert((func, seed), candidate);
                    self.changed = true;
                }
            }
        }
    }

    /// Add a finding; keeps the lexicographically smallest path per key.
    fn record_sink(&mut self, finding: SinkFinding) {
        let key = finding.registry_key();
        match self.sinks.get(&key) {
            None => {
                self.sinks.insert(key, finding);
                self.changed = true;
            }
            Some(current) => {
                if finding.path < current.path {
                    self.sinks.insert(key, finding);
                    self.changed = true;
                }
            }
        }
    }

    /// Grow a function's summary; flags `changed` on growth.
    fn grow_summary(&mut self, func: FuncId, markers: impl IntoIterator<Item = usize>) {
        let summary = self.summary.entry(func).or_default();
        let before = summary.len();
        summary.extend(markers);
        if summary.len() != before {
            self.changed = true;
        }
    }
}

// ── The walker: one pass over one function body ──────────────────────────────

/// Walks one function body with the current fixpoint state, emitting slot
/// updates, pushes into callees, summary markers and sink findings.
struct Walker<'a> {
    prog: &'a Program,
    state: &'a mut State,
    /// The function whose body is being walked.
    func: FuncId,
    /// Taint of the enclosing branch conditions: bindings made while this is
    /// non-empty inherit it (control dependence, see module docs).
    ambient: Taint,
    /// Closure frames currently open, e.g. `["spawn_blocking"]` inside a
    /// `spawn_blocking` closure — appended to reported sink paths.
    frames: Vec<String>,
    /// Markers observed at return points (tail, `return`, `?` operands) —
    /// becomes the function's summary for this pass.
    ret_markers: BTreeSet<usize>,
}

impl<'a> Walker<'a> {
    fn new(prog: &'a Program, state: &'a mut State, func: FuncId) -> Self {
        Walker {
            prog,
            state,
            func,
            ambient: Taint::default(),
            frames: Vec::new(),
            ret_markers: BTreeSet::new(),
        }
    }

    /// The enclosing implementation's self type, for `self.f(..)` and
    /// `Self::f(..)` resolution (`None` for free functions).
    fn enclosing_self_ty(&self) -> Option<&str> {
        self.prog.funcs[self.func].self_ty.as_deref()
    }

    fn slot(&self, name: &str) -> Taint {
        self.state.slot(self.func, name)
    }

    fn add_to_slot(&mut self, name: &str, taint: &Taint) {
        self.state.add_to_slot(self.func, name, taint);
    }

    /// Report `kind` at `line` for every seed in `taint` (the operand taint,
    /// or operand ∪ ambient for the panic macros). No seeds — no finding:
    /// the output is (sink, seed) pairs.
    fn sink(&mut self, kind: SinkKind, line: usize, taint: &Taint) {
        if taint.seeds.is_empty() {
            return;
        }
        let file = self.prog.funcs[self.func].file;
        let snippet = self.prog.snippet(file, line);
        let function = self.prog.funcs[self.func].display.clone();
        for &seed in &taint.seeds {
            let mut path = vec![self.prog.seed_display(seed)];
            path.extend(self.state.chain(self.func, seed));
            path.extend(self.frames.iter().cloned());
            path.push(snippet.clone());
            self.state.record_sink(SinkFinding {
                file: self.prog.files[file].clone(),
                line,
                kind,
                function: function.clone(),
                seed: self.prog.seed_display(seed),
                path: path.join(" -> "),
                snippet: snippet.clone(),
            });
        }
    }

    /// Push a call site's argument taints into `callee`'s parameters.
    /// `list[i]` maps onto `params[i]`; summary markers are *not* pushed
    /// (markers are per-function), seeds are, and each seed's chain grows by
    /// the current closure frames plus `frame` (the callee's display name).
    fn push(&mut self, callee: FuncId, list: &[Taint], frame: &str) {
        let params = self.prog.funcs[callee].params.clone();
        for (index, taint) in list.iter().enumerate() {
            let Some(names) = params.get(index) else {
                break;
            };
            if taint.seeds.is_empty() {
                continue;
            }
            for name in names {
                self.state.add_to_slot(
                    callee,
                    name,
                    &Taint {
                        seeds: taint.seeds.clone(),
                        markers: BTreeSet::new(),
                    },
                );
            }
            for &seed in &taint.seeds {
                let mut candidate = self.state.chain(self.func, seed);
                candidate.extend(self.frames.iter().cloned());
                candidate.push(frame.to_string());
                self.state.shorten_chain(callee, seed, candidate);
            }
        }
    }

    /// The return taint of a resolved call: exactly those arguments the
    /// callee's summary admits (empty until the summary says otherwise; the
    /// fixpoint fills it in on later passes).
    fn resolved_return(&self, callee: FuncId, list: &[Taint]) -> Taint {
        let mut out = Taint::default();
        if let Some(summary) = self.state.summary.get(&callee) {
            for &index in summary {
                if let Some(taint) = list.get(index) {
                    out.union_with(taint);
                }
            }
        }
        out
    }

    /// The taint of `expr`, with all side effects of evaluating it (slot
    /// writes, pushes, sinks, summary markers) applied on the way.
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn expr_taint(&mut self, expr: &Expr) -> Taint {
        match expr {
            Expr::Path(path) if path.qself.is_none() => {
                if let Some(ident) = single_ident(&path.path) {
                    // Multi-segment paths are handled in the `else` below;
                    // here the single ident names a slot (or nothing).
                    self.slot(&ident.to_string())
                } else {
                    // Multi-segment paths name items (constants, variants,
                    // types), never locals: untainted.
                    Taint::default()
                }
            }
            Expr::Lit(_) | Expr::Infer(_) | Expr::Continue(_) => Taint::default(),
            Expr::Field(field) => self.expr_taint(&field.base),
            Expr::Cast(cast) => self.expr_taint(&cast.expr),
            Expr::Reference(reference) => self.expr_taint(&reference.expr),
            Expr::Unary(unary) => self.expr_taint(&unary.expr),
            Expr::Paren(paren) => self.expr_taint(&paren.expr),
            Expr::Group(group) => self.expr_taint(&group.expr),
            Expr::Await(await_expr) => self.expr_taint(&await_expr.base),
            Expr::Binary(binary) => {
                let left = self.expr_taint(&binary.left);
                let right = self.expr_taint(&binary.right);
                // Compound assignment (`x += y`) is `Expr::Binary` in syn 2:
                // the place gains the right-hand side's taint.
                if is_assign_op(&binary.op) {
                    self.bind_expr_place(&binary.left, &right);
                }
                left.union(&right)
            }
            Expr::Index(index) => {
                let base = self.expr_taint(&index.expr);
                let index_taint = self.expr_taint(&index.index);
                // A tainted *index* panics; a tainted collection is a data
                // question and is not this sink.
                let line = line_of(index.bracket_token.span.open());
                self.sink(SinkKind::Index, line, &index_taint);
                base.union(&index_taint)
            }
            Expr::Try(try_expr) => {
                let inner = self.expr_taint(&try_expr.expr);
                // The `Err` path returns from the function, so `?` operands
                // are return points for summary purposes.
                self.ret_markers.extend(inner.markers.iter().copied());
                inner
            }
            Expr::Return(return_expr) => {
                if let Some(value) = &return_expr.expr {
                    let inner = self.expr_taint(value);
                    self.ret_markers.extend(inner.markers.iter().copied());
                }
                Taint::default()
            }
            Expr::Break(break_expr) => {
                if let Some(value) = &break_expr.expr {
                    self.expr_taint(value);
                }
                Taint::default()
            }
            Expr::Tuple(tuple) => union_of(self, &tuple.elems),
            Expr::Array(array) => union_of(self, &array.elems),
            Expr::Repeat(repeat) => self.expr_taint(&repeat.expr),
            Expr::Struct(struct_expr) => {
                let mut out = Taint::default();
                for field in &struct_expr.fields {
                    let taint = self.expr_taint(&field.expr);
                    out.union_with(&taint);
                }
                if let Some(rest) = &struct_expr.rest {
                    let taint = self.expr_taint(rest);
                    out.union_with(&taint);
                }
                out
            }
            Expr::Range(range) => {
                let mut out = Taint::default();
                for part in [&range.start, &range.end].into_iter().flatten() {
                    let taint = self.expr_taint(part);
                    out.union_with(&taint);
                }
                out
            }
            Expr::MethodCall(call) => self.method_call(call),
            Expr::Call(call) => self.call(call),
            Expr::Macro(expr_macro) => {
                let ambient = self.ambient.clone();
                self.mac(&expr_macro.mac, &ambient)
            }
            Expr::Closure(closure) => {
                // A closure outside a call position: no argument taint to
                // seed parameters with (documented limit).
                let empty = Taint::default();
                self.closure(closure, "closure", &empty)
            }
            Expr::Block(block) => self.block(&block.block),
            Expr::If(expr_if) => self.if_expr(expr_if),
            Expr::Match(expr_match) => self.match_expr(expr_match),
            Expr::Let(expr_let) => {
                // `if let`/`while let` scrutinee: the pattern's bindings take
                // the scrutinee's taint (plus the ambient branch context).
                let taint = self.expr_taint(&expr_let.expr);
                let bind = taint.clone().union(&self.ambient);
                self.bind_pat(&expr_let.pat, &bind);
                taint
            }
            Expr::ForLoop(loop_expr) => {
                let iter = self.expr_taint(&loop_expr.expr);
                let scoped = self.ambient.clone().union(&iter);
                let saved = std::mem::replace(&mut self.ambient, scoped.clone());
                self.bind_pat(&loop_expr.pat, &scoped);
                self.block(&loop_expr.body);
                self.ambient = saved;
                Taint::default()
            }
            Expr::While(while_expr) => {
                let cond = self.expr_taint(&while_expr.cond);
                let scoped = self.ambient.clone().union(&cond);
                let saved = std::mem::replace(&mut self.ambient, scoped);
                self.block(&while_expr.body);
                self.ambient = saved;
                Taint::default()
            }
            Expr::Loop(loop_expr) => {
                self.block(&loop_expr.body);
                Taint::default()
            }
            Expr::Assign(assign) => {
                let right = self.expr_taint(&assign.right);
                self.expr_taint(&assign.left);
                self.bind_expr_place(&assign.left, &right);
                Taint::default()
            }
            Expr::Async(expr_async) => self.block(&expr_async.block),
            Expr::TryBlock(try_block) => {
                let out = self.block(&try_block.block);
                self.ret_markers.extend(out.markers.iter().copied());
                out
            }
            Expr::Const(expr_const) => self.block(&expr_const.block),
            // Anything else (Verbatim, …) never becomes AST we can follow:
            // union whatever descendants the default walk can see.
            other => self.children_taint(other),
        }
    }

    /// Union the taint of every descendant expression of an *unhandled*
    /// expression variant. The default `syn` walk dispatches children through
    /// [`Walker::expr_taint`], so handled nodes below an unhandled one still
    /// get their full treatment and are not re-walked.
    fn children_taint(&mut self, expr: &Expr) -> Taint {
        let mut collector = ChildCollector {
            walker: self,
            acc: Taint::default(),
        };
        syn::visit::visit_expr(&mut collector, expr);
        collector.acc
    }

    /// A method call: sinks on a tainted receiver, then resolution against
    /// inherent methods; unresolved calls (external, trait, `dyn`) return the
    /// union of receiver and arguments. Closure arguments are visited with
    /// the call's frame and the receiver+argument taint as their parameter
    /// seed (module docs).
    fn method_call(&mut self, call: &ExprMethodCall) -> Taint {
        let receiver = self.expr_taint(&call.receiver);
        let method = call.method.to_string();

        // Operand-based sinks: the receiver must itself be tainted.
        if method == "unwrap" || method == "expect" {
            let kind = if method == "unwrap" {
                SinkKind::Unwrap
            } else {
                SinkKind::Expect
            };
            let line = line_of(call.method.span());
            self.sink(kind, line, &receiver);
        }

        let candidates = self.resolve_method(&method, &call.receiver);
        let frame = match candidates.as_slice() {
            [single] => self.prog.funcs[*single].display.clone(),
            _ => method.clone(),
        };

        let mut base = receiver.clone();
        let mut list = vec![receiver];
        let mut closures: Vec<(usize, &ExprClosure)> = Vec::new();
        for arg in &call.args {
            if let Expr::Closure(closure) = arg {
                closures.push((list.len(), closure));
                list.push(Taint::default());
            } else {
                let taint = self.expr_taint(arg);
                base.union_with(&taint);
                list.push(taint);
            }
        }
        for (position, closure) in closures {
            let taint = self.closure(closure, &frame, &base);
            list[position] = taint;
        }

        if candidates.is_empty() {
            let mut out = Taint::default();
            for taint in &list {
                out.union_with(taint);
            }
            return out;
        }
        let mut out = Taint::default();
        for candidate in candidates {
            let frame = self.prog.funcs[candidate].display.clone();
            self.push(candidate, &list, &frame);
            let ret = self.resolved_return(candidate, &list);
            out.union_with(&ret);
        }
        out
    }

    /// A call expression: resolve the syntactic path first (so closure
    /// arguments can carry the callee's frame), then push argument taints and
    /// take the summary-filtered return. Unresolved callees — including
    /// calls through function parameters — return the union of arguments.
    fn call(&mut self, call: &ExprCall) -> Taint {
        let candidates = self.resolve_path_call(&call.func);
        let fallback_frame = match &*call.func {
            Expr::Path(path) => path
                .path
                .segments
                .last()
                .map_or_else(|| "call".to_string(), |segment| segment.ident.to_string()),
            _ => "call".to_string(),
        };
        let frame = match candidates.as_slice() {
            [single] => self.prog.funcs[*single].display.clone(),
            _ => fallback_frame,
        };

        let mut base = Taint::default();
        let mut list = Vec::with_capacity(call.args.len());
        let mut closures: Vec<(usize, &ExprClosure)> = Vec::new();
        for arg in &call.args {
            if let Expr::Closure(closure) = arg {
                closures.push((list.len(), closure));
                list.push(Taint::default());
            } else {
                let taint = self.expr_taint(arg);
                base.union_with(&taint);
                list.push(taint);
            }
        }
        for (position, closure) in closures {
            let taint = self.closure(closure, &frame, &base);
            list[position] = taint;
        }

        if candidates.is_empty() {
            let mut out = Taint::default();
            for taint in &list {
                out.union_with(taint);
            }
            return out;
        }
        let mut out = Taint::default();
        for candidate in candidates {
            let frame = self.prog.funcs[candidate].display.clone();
            self.push(candidate, &list, &frame);
            let ret = self.resolved_return(candidate, &list);
            out.union_with(&ret);
        }
        out
    }

    /// Inherent methods named `method`, preferring the enclosing self type
    /// for `self.method(..)` receivers. Empty vec = unresolved.
    fn resolve_method(&self, method: &str, receiver: &Expr) -> Vec<FuncId> {
        if path_single_ident(receiver).is_some_and(|ident| ident == "self") {
            // `self.f(..)`: only the *enclosing type's* method counts — a
            // same-named method on another type is not this call.
            let Some(self_ty) = self.enclosing_self_ty() else {
                return Vec::new();
            };
            return self
                .prog
                .methods_of_ty
                .get(&(self_ty.to_string(), method.to_string()))
                .filter(|found| !found.is_empty())
                .cloned()
                .unwrap_or_default();
        }
        self.prog
            .methods
            .get(method)
            .filter(|found| !found.is_empty())
            .cloned()
            .unwrap_or_default()
    }

    /// Resolve a syntactic call path: bare `f(..)` → free function;
    /// `Self::f(..)` → the enclosing type's method; `Type::f(..)` → that
    /// type's inherent method, then a free function of that name.
    /// Empty vec = unresolved.
    fn resolve_path_call(&self, func: &Expr) -> Vec<FuncId> {
        let Expr::Path(path) = func else {
            return Vec::new();
        };
        let Some(segments_last) = path.path.segments.last() else {
            return Vec::new();
        };
        let last = &segments_last.ident;
        if path.path.segments.len() == 1 {
            return self
                .prog
                .by_name
                .get(&last.to_string())
                .filter(|found| !found.is_empty())
                .cloned()
                .unwrap_or_default();
        }
        if path
            .path
            .segments
            .first()
            .is_some_and(|first| first.ident == "Self")
            && let Some(self_ty) = self.enclosing_self_ty()
            && let Some(found) = self
                .prog
                .methods_of_ty
                .get(&(self_ty.to_string(), last.to_string()))
                .filter(|found| !found.is_empty())
        {
            return found.clone();
        }
        let type_name = &path.path.segments[path.path.segments.len() - 2].ident;
        if let Some(found) = self
            .prog
            .methods_of_ty
            .get(&(type_name.to_string(), last.to_string()))
            .filter(|found| !found.is_empty())
        {
            return found.clone();
        }
        self.prog
            .by_name
            .get(&last.to_string())
            .filter(|found| !found.is_empty())
            .cloned()
            .unwrap_or_default()
    }

    /// A macro invocation: for the panic family, a sink when the macro's
    /// token identifiers or the ambient branch taint carry seeds; as a value,
    /// the taint of its token identifiers (`format!("{x}")` is tainted when
    /// `x` is).
    fn mac(&mut self, mac: &syn::Macro, ambient: &Taint) -> Taint {
        let name = mac
            .path
            .segments
            .last()
            .map_or_else(String::new, |segment| segment.ident.to_string());
        let mut idents = Vec::new();
        collect_idents(&mac.tokens, &mut idents);
        let mut args = Taint::default();
        for ident in &idents {
            let taint = self.slot(ident);
            args.union_with(&taint);
        }
        let family = match name.as_str() {
            "panic" => Some(SinkKind::Panic),
            "unreachable" => Some(SinkKind::Unreachable),
            "todo" => Some(SinkKind::Todo),
            "unimplemented" => Some(SinkKind::Unimplemented),
            _ => None,
        };
        if let Some(kind) = family {
            let reach = args.clone().union(ambient);
            let line = line_of(syn::spanned::Spanned::span(&mac.path));
            self.sink(kind, line, &reach);
        }
        args
    }

    /// Walk a closure inline in the enclosing scope — captures (`move` or
    /// not) *are* the enclosing slots by name — with `frame` on the reported
    /// path and `param_taint` (the call's receiver+other-argument taint)
    /// bound to the closure's parameters. The closure expression's value is
    /// its tail expression (module docs: limits).
    fn closure(&mut self, closure: &ExprClosure, frame: &str, param_taint: &Taint) -> Taint {
        self.frames.push(frame.to_string());
        let bind = param_taint.clone().union(&self.ambient);
        for input in &closure.inputs {
            self.bind_pat(input, &bind);
        }
        let value = match &*closure.body {
            Expr::Block(block) => self.block(&block.block),
            body => self.expr_taint(body),
        };
        self.frames.pop();
        value
    }

    fn if_expr(&mut self, expr_if: &syn::ExprIf) -> Taint {
        let cond = self.expr_taint(&expr_if.cond);
        let scoped = self.ambient.clone().union(&cond);
        let saved = std::mem::replace(&mut self.ambient, scoped);
        let then_taint = self.block(&expr_if.then_branch);
        let else_taint = match &expr_if.else_branch {
            Some((_, else_expr)) => self.expr_taint(else_expr),
            None => Taint::default(),
        };
        self.ambient = saved;
        then_taint.union(&else_taint)
    }

    fn match_expr(&mut self, expr_match: &syn::ExprMatch) -> Taint {
        let scrutinee = self.expr_taint(&expr_match.expr);
        let scoped = self.ambient.clone().union(&scrutinee);
        let saved = std::mem::replace(&mut self.ambient, scoped);
        let mut out = Taint::default();
        for arm in &expr_match.arms {
            // Arm bindings take the scrutinee's taint, and the arm body sits
            // under the tainted scrutinee: both go through the ambient set.
            let bind = self.ambient.clone();
            self.bind_pat(&arm.pat, &bind);
            if let Some((_, guard)) = &arm.guard {
                self.expr_taint(guard);
            }
            let taint = self.expr_taint(&arm.body);
            out.union_with(&taint);
        }
        self.ambient = saved;
        out
    }

    /// Walk a block; returns the tail expression's taint (or `()`), applying
    /// every statement's effects along the way. Nested `fn`/`impl` items are
    /// separate functions and are skipped here.
    fn block(&mut self, block: &Block) -> Taint {
        let mut out = Taint::default();
        let last = block.stmts.len().wrapping_sub(1);
        for (position, stmt) in block.stmts.iter().enumerate() {
            match stmt {
                Stmt::Local(local) => {
                    let mut init = Taint::default();
                    if let Some(local_init) = &local.init {
                        init = self.expr_taint(&local_init.expr);
                        if let Some((_, diverge)) = &local_init.diverge {
                            self.expr_taint(diverge);
                        }
                    }
                    // Bindings under a tainted branch inherit the branch
                    // taint: control dependence, conservative direction.
                    let bind = init.union(&self.ambient);
                    self.bind_pat(&local.pat, &bind);
                }
                Stmt::Expr(expr, _) => {
                    let taint = self.expr_taint(expr);
                    if position == last {
                        out = taint;
                    }
                }
                Stmt::Macro(stmt_macro) => {
                    let ambient = self.ambient.clone();
                    let _ = self.mac(&stmt_macro.mac, &ambient);
                }
                Stmt::Item(_) => {}
            }
        }
        out
    }

    /// Give a pattern's bindings the taint of the value it destructures
    /// (tuple/struct/tuple-struct/slice patterns recurse; `x @ sub` binds
    /// both; or-patterns bind every alternative's names).
    fn bind_pat(&mut self, pat: &Pat, taint: &Taint) {
        if taint.is_empty() {
            return;
        }
        match pat {
            Pat::Ident(pat_ident) => {
                self.add_to_slot(&pat_ident.ident.to_string(), taint);
                if let Some((_, subpat)) = &pat_ident.subpat {
                    self.bind_pat(subpat, taint);
                }
            }
            Pat::Struct(pat_struct) => {
                for field in &pat_struct.fields {
                    self.bind_pat(&field.pat, taint);
                }
                // `..` (PatRest) binds no name of its own.
            }
            Pat::Tuple(pat_tuple) => {
                for elem in &pat_tuple.elems {
                    self.bind_pat(elem, taint);
                }
            }
            Pat::TupleStruct(pat_tuple_struct) => {
                for elem in &pat_tuple_struct.elems {
                    self.bind_pat(elem, taint);
                }
            }
            Pat::Slice(pat_slice) => {
                for elem in &pat_slice.elems {
                    self.bind_pat(elem, taint);
                }
            }
            Pat::Or(pat_or) => {
                for case in &pat_or.cases {
                    self.bind_pat(case, taint);
                }
            }
            Pat::Reference(pat_reference) => self.bind_pat(&pat_reference.pat, taint),
            Pat::Type(pat_type) => self.bind_pat(&pat_type.pat, taint),
            Pat::Paren(pat_paren) => self.bind_pat(&pat_paren.pat, taint),
            // Wild, literals, consts, macros: nothing bound.
            _ => {}
        }
    }

    /// The place expression `place` gains `taint` (`x = …`, `x.f = …`,
    /// `x[i] = …` all re-taint the base slot; field-insensitive).
    fn bind_expr_place(&mut self, place: &Expr, taint: &Taint) {
        if taint.is_empty() {
            return;
        }
        match place {
            Expr::Path(path) => {
                if let Some(ident) = single_ident(&path.path) {
                    self.add_to_slot(&ident.to_string(), taint);
                }
            }
            Expr::Field(field) => self.bind_expr_place(&field.base, taint),
            Expr::Index(index) => self.bind_expr_place(&index.expr, taint),
            Expr::Paren(paren) => self.bind_expr_place(&paren.expr, taint),
            Expr::Unary(unary) => self.bind_expr_place(&unary.expr, taint),
            _ => {}
        }
    }
}

/// Collects the taint of every descendant expression of an *unhandled*
/// expression variant by routing children back through
/// [`Walker::expr_taint`]. Nested items are skipped — they are separate
/// functions with their own slots.
struct ChildCollector<'w, 'a> {
    walker: &'w mut Walker<'a>,
    acc: Taint,
}

impl<'ast> Visit<'ast> for ChildCollector<'_, '_> {
    fn visit_expr(&mut self, node: &'ast Expr) {
        let taint = self.walker.expr_taint(node);
        self.acc.union_with(&taint);
    }

    fn visit_item(&mut self, _node: &'ast Item) {}
}

// ── Small helpers ────────────────────────────────────────────────────────────

/// The identifier of a plain one-segment path (`foo`), `None` for qualified
/// paths (`Foo::bar`) — those name items, never locals.
fn single_ident(path: &syn::Path) -> Option<&Ident> {
    if path.segments.len() == 1 {
        Some(&path.segments[0].ident)
    } else {
        None
    }
}

/// The identifier when an expression is a plain `ident` path.
fn path_single_ident(expr: &Expr) -> Option<Ident> {
    let Expr::Path(path) = expr else {
        return None;
    };
    if path.qself.is_none() {
        return single_ident(&path.path).cloned();
    }
    None
}

/// Whether a binary operator is a compound assignment (`+=`, `&=`, …) —
/// syn 2 parses those as `Expr::Binary`, but they *write* the left place.
fn is_assign_op(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::AddAssign(_)
            | BinOp::SubAssign(_)
            | BinOp::MulAssign(_)
            | BinOp::DivAssign(_)
            | BinOp::RemAssign(_)
            | BinOp::BitXorAssign(_)
            | BinOp::BitAndAssign(_)
            | BinOp::BitOrAssign(_)
            | BinOp::ShlAssign(_)
            | BinOp::ShrAssign(_)
    )
}

/// 1-based line of a span. Needs proc-macro2's `span-locations` feature
/// (enabled in `Cargo.toml`), which is accurate outside a proc-macro context
/// — exactly where `build.rs` and the test target run.
fn line_of(span: Span) -> usize {
    span.start().line
}

/// Every identifier in a macro token stream, in order, recursing into
/// delimited groups — the conservative read of `format!("{x}", y)` and kin.
fn collect_idents(tokens: &TokenStream, out: &mut Vec<String>) {
    for token in tokens.clone() {
        match token {
            TokenTree::Ident(ident) => out.push(ident.to_string()),
            TokenTree::Group(group) => collect_idents(&group.stream(), out),
            TokenTree::Literal(literal) => collect_format_idents(&literal.to_string(), out),
            TokenTree::Punct(_) => {}
        }
    }
}

/// Identifiers interpolated into a format-string literal (`{name}`,
/// `{name:?}`) — they sit *inside* the string token, so scanning the token
/// stream for idents alone would miss them and `format!("{timestamp}")`
/// would come back untainted. Positional (`{}`, `{0}`) and captured
/// (`{:?}`) placeholders name no local and are skipped; `{{` is an escape.
fn collect_format_idents(literal: &str, out: &mut Vec<String>) {
    let Some(content) = string_literal_content(literal) else {
        return;
    };
    let mut rest = content;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        if let Some(escaped) = after.strip_prefix('{') {
            rest = escaped; // `{{` escape
            continue;
        }
        let Some(end) = after.find('}') else {
            break;
        };
        let spec = &after[..end];
        rest = &after[end + 1..];
        let name: String = spec
            .chars()
            .take_while(|character| character.is_ascii_alphanumeric() || *character == '_')
            .collect();
        if !name.is_empty() && !name.starts_with(|character: char| character.is_ascii_digit()) {
            out.push(name);
        }
    }
}

/// The *content* of a string-literal token (`"…"`, `r"#…"#`, `b"…"`),
/// `None` for anything that is not a string literal (char literals such as
/// `'{'` would otherwise be mistaken for interpolation).
fn string_literal_content(literal: &str) -> Option<&str> {
    let body = literal
        .strip_prefix('b')
        .or_else(|| literal.strip_prefix('c'))
        .unwrap_or(literal);
    if let Some(rest) = body.strip_prefix('"') {
        let end = rest.rfind('"')?;
        return Some(&rest[..end]);
    }
    let rest = body.strip_prefix('r')?;
    let hashes = rest
        .chars()
        .take_while(|&character| character == '#')
        .count();
    let after_quote = rest.get(hashes..)?;
    let after_quote = after_quote.strip_prefix('"')?;
    let closing = format!("\"{}", "#".repeat(hashes));
    let end = after_quote.find(&closing)?;
    Some(&after_quote[..end])
}

/// The union of every element expression's taint (tuples, arrays).
fn union_of(
    walker: &mut Walker<'_>,
    exprs: &syn::punctuated::Punctuated<Expr, syn::Token![,]>,
) -> Taint {
    let mut out = Taint::default();
    for expr in exprs {
        let taint = walker.expr_taint(expr);
        out.union_with(&taint);
    }
    out
}

/// Every binding name a pattern introduces (`(a, b)`, `x @ Some(y)`, `mut
/// x` → all names; wildcards and literals bind nothing).
fn collect_pat_idents(pat: &Pat, out: &mut Vec<String>) {
    match pat {
        Pat::Ident(pat_ident) => {
            out.push(pat_ident.ident.to_string());
            if let Some((_, subpat)) = &pat_ident.subpat {
                collect_pat_idents(subpat, out);
            }
        }
        Pat::Struct(pat_struct) => {
            for field in &pat_struct.fields {
                collect_pat_idents(&field.pat, out);
            }
        }
        Pat::Tuple(pat_tuple) => {
            for elem in &pat_tuple.elems {
                collect_pat_idents(elem, out);
            }
        }
        Pat::TupleStruct(pat_tuple_struct) => {
            for elem in &pat_tuple_struct.elems {
                collect_pat_idents(elem, out);
            }
        }
        Pat::Slice(pat_slice) => {
            for elem in &pat_slice.elems {
                collect_pat_idents(elem, out);
            }
        }
        Pat::Or(pat_or) => {
            for case in &pat_or.cases {
                collect_pat_idents(case, out);
            }
        }
        Pat::Reference(pat_reference) => collect_pat_idents(&pat_reference.pat, out),
        Pat::Type(pat_type) => collect_pat_idents(&pat_type.pat, out),
        Pat::Paren(pat_paren) => collect_pat_idents(&pat_paren.pat, out),
        _ => {}
    }
}

/// The Rocket verb attribute on a function — the same detection
/// `ast_scan.rs` applies (bare or `rocket::`-qualified last segment).
fn has_verb_attribute(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let segments = &attr.path().segments;
        let Some(last) = segments.last() else {
            return false;
        };
        let verb = last.ident.to_string();
        VERB_ATTRIBUTES.contains(&verb.as_str())
            && (segments.len() == 1
                || segments
                    .first()
                    .is_some_and(|first| first.ident == "rocket"))
    })
}

// ── Seed and guard type rules ────────────────────────────────────────────────

/// Whether a handler parameter's type is client-supplied data — the seed
/// rule documented in the module docs.
fn is_seed_type(ty: &Type) -> bool {
    match ty {
        Type::Reference(reference) => is_seed_type(&reference.elem),
        Type::Slice(slice) => is_seed_type(&slice.elem),
        Type::Array(array) => is_seed_type(&array.elem),
        Type::Path(path) => {
            let Some(segment) = path.path.segments.last() else {
                return false;
            };
            let ident = segment.ident.to_string();
            if ident == "String" || CLIENT_PRIMITIVES.contains(&ident.as_str()) {
                return true;
            }
            // `Json<T>` is a request body whatever `T` is: the payload is
            // client-supplied even when its fields are not primitives.
            if ident == "Json" {
                return true;
            }
            if matches!(ident.as_str(), "Option" | "Vec") {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    return args
                        .args
                        .iter()
                        .any(|arg| matches!(arg, GenericArgument::Type(ty) if is_seed_type(ty)));
                }
                return false;
            }
            false
        }
        _ => false,
    }
}

/// Whether a handler parameter's type is a guard — `GuardResult<GuardX>` (the
/// deferred-error form whose check only runs if the handler propagates it)
/// or a direct `GuardX` (identifiers starting with `Guard`).
fn is_guard_type(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.path.segments.last().is_some_and(|segment| {
        let ident = segment.ident.to_string();
        ident == "GuardResult" || ident.starts_with("Guard")
    })
}

/// Render a type as written, for guard-finding messages. Hand-rolled so the
/// module needs no `quote` dependency.
fn type_display(ty: &Type) -> String {
    match ty {
        Type::Path(path) => {
            let mut out = String::new();
            for segment in &path.path.segments {
                if !out.is_empty() {
                    out.push_str("::");
                }
                out.push_str(&segment.ident.to_string());
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    let rendered: Vec<String> = args.args.iter().map(display_arg).collect();
                    if !rendered.is_empty() {
                        out.push('<');
                        out.push_str(&rendered.join(", "));
                        out.push('>');
                    }
                }
            }
            out
        }
        Type::Reference(reference) => {
            let lifetime = reference
                .lifetime
                .as_ref()
                .map_or_else(String::new, |life| format!("{life} "));
            let mutability = if reference.mutability.is_some() {
                "mut "
            } else {
                ""
            };
            format!("&{lifetime}{mutability}{}", type_display(&reference.elem))
        }
        Type::Tuple(tuple) => {
            let rendered: Vec<String> = tuple.elems.iter().map(type_display).collect();
            format!("({})", rendered.join(", "))
        }
        Type::Slice(slice) => format!("[{}]", type_display(&slice.elem)),
        Type::Paren(paren) => type_display(&paren.elem),
        Type::Never(_) => "!".to_string(),
        _ => "?".to_string(),
    }
}

fn display_arg(arg: &GenericArgument) -> String {
    match arg {
        GenericArgument::Type(ty) => type_display(ty),
        GenericArgument::Lifetime(lifetime) => lifetime.to_string(),
        GenericArgument::AssocType(assoc) => {
            format!("{} = {}", assoc.ident, type_display(&assoc.ty))
        }
        GenericArgument::Constraint(constraint) => constraint.ident.to_string(),
        _ => "_".to_string(),
    }
}

// ── Program construction ─────────────────────────────────────────────────────

/// Parse every source unit and build the function/resolution model. Parse
/// failures become [`Error::UnparsableFile`] findings, never panics.
fn build_program(sources: &[(String, String)], errors: &mut Vec<Error>) -> Program {
    let mut prog = Program {
        files: Vec::new(),
        lines: Vec::new(),
        funcs: Vec::new(),
        by_name: BTreeMap::new(),
        methods: BTreeMap::new(),
        methods_of_ty: BTreeMap::new(),
        seeds: Vec::new(),
    };

    for (path, content) in sources {
        let file = prog.files.len();
        prog.files.push(path.clone());
        prog.lines
            .push(content.lines().map(str::to_string).collect());
        let ast = match syn::parse_file(content) {
            Ok(ast) => ast,
            Err(err) => {
                errors.push(Error::UnparsableFile {
                    file: path.clone(),
                    error: err.to_string(),
                });
                continue;
            }
        };
        collect_funcs(&ast, file, &mut prog);
    }

    index_functions(&mut prog);
    seed_handlers(&mut prog);
    prog
}

/// Collect every free function and every inherent-`impl` method, in source
/// order. Trait impls are deliberately not indexed (module docs: limits).
/// Nested functions are collected too (the default walk descends into them).
fn collect_funcs(ast: &syn::File, file: usize, prog: &mut Program) {
    struct Collector<'p> {
        prog: &'p mut Program,
        file: usize,
    }
    impl<'ast> Visit<'ast> for Collector<'_> {
        fn visit_item_fn(&mut self, node: &'ast ItemFn) {
            self.push_fn(&node.attrs, &node.sig, &node.block, None);
            syn::visit::visit_item_fn(self, node);
        }
        fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
            if node.trait_.is_none() {
                let self_ty = simple_type_name(&node.self_ty);
                for item in &node.items {
                    if let ImplItem::Fn(method) = item {
                        let self_ty = self_ty.clone();
                        self.push_fn(&node.attrs, &method.sig, &method.block, self_ty);
                    }
                }
            }
            syn::visit::visit_item_impl(self, node);
        }
    }
    impl Collector<'_> {
        fn push_fn(
            &mut self,
            attrs: &[Attribute],
            sig: &Signature,
            block: &Block,
            self_ty: Option<String>,
        ) {
            let name = sig.ident.to_string();
            let display = match &self_ty {
                Some(ty) => format!("{ty}::{name}"),
                None => name,
            };
            let is_handler = has_verb_attribute(attrs);
            let mut params: Vec<Vec<String>> = Vec::new();
            let mut seed_params: Vec<String> = Vec::new();
            let mut guard_params: Vec<GuardParam> = Vec::new();
            for input in &sig.inputs {
                match input {
                    FnArg::Receiver(_) => params.push(vec!["self".to_string()]),
                    FnArg::Typed(PatType { pat, ty, .. }) => {
                        let mut names = Vec::new();
                        collect_pat_idents(pat, &mut names);
                        if names.is_empty() {
                            // `_`-only or literal pattern: one anonymous
                            // slot so parameter indices stay aligned.
                            names.push("_".to_string());
                        }
                        if is_handler && is_seed_type(ty) {
                            seed_params.extend(names.iter().cloned());
                        }
                        if is_handler && is_guard_type(ty) {
                            guard_params.push(GuardParam {
                                binding: names[0].clone(),
                                ty: type_display(ty),
                                line: line_of(syn::spanned::Spanned::span(&**pat)),
                            });
                        }
                        params.push(names);
                    }
                }
            }
            self.prog.funcs.push(Func {
                file: self.file,
                display,
                self_ty,
                params,
                seed_params,
                guard_params,
                is_handler,
                body: block.clone(),
            });
        }
    }

    let mut collector = Collector { prog, file };
    collector.visit_file(ast);
}

/// The outermost name of a (possibly reference-wrapped) type path, e.g.
/// `TreeSnapshot` for `TreeSnapshot` and `&'static TreeSnapshot`.
fn simple_type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        Type::Reference(reference) => simple_type_name(&reference.elem),
        Type::Paren(paren) => simple_type_name(&paren.elem),
        _ => None,
    }
}

/// Fill the resolution indexes: free functions by bare name, inherent
/// methods by name and by `(self type, name)`.
fn index_functions(prog: &mut Program) {
    for id in 0..prog.funcs.len() {
        let (display, self_ty) = {
            let func = &prog.funcs[id];
            (func.display.clone(), func.self_ty.clone())
        };
        let name = match &self_ty {
            Some(_) => display.rsplit("::").next().unwrap_or(&display).to_string(),
            None => display,
        };
        match self_ty {
            Some(self_ty) => {
                prog.methods.entry(name.clone()).or_default().push(id);
                prog.methods_of_ty
                    .entry((self_ty, name))
                    .or_default()
                    .push(id);
            }
            None => {
                prog.by_name.entry(name).or_default().push(id);
            }
        }
    }
}

/// Create one seed per client-supplied parameter of every route handler.
fn seed_handlers(prog: &mut Program) {
    for id in 0..prog.funcs.len() {
        if !prog.funcs[id].is_handler {
            continue;
        }
        for param in prog.funcs[id].seed_params.clone() {
            prog.seeds.push(Seed { handler: id, param });
        }
    }
}

// ── Guard-propagation check ──────────────────────────────────────────────────

/// Collects every read of the guard bindings and classifies which reads are
/// discards. A `let _ = auth?;` is *not* a discard: the `?` propagates the
/// guard's error before the value is dropped — that is the fixed idiom of
/// the historical bug.
struct GuardScan<'n> {
    names: &'n BTreeSet<String>,
    /// Per guard binding, one entry per occurrence: `Some(line)` when the
    /// occurrence sits inside a non-propagating `let _ = …`.
    found: BTreeMap<String, Vec<Option<usize>>>,
    discarding: Vec<Option<usize>>,
}

impl<'ast> Visit<'ast> for GuardScan<'_> {
    fn visit_stmt(&mut self, node: &'ast Stmt) {
        if let Stmt::Local(local) = node
            && matches!(local.pat, Pat::Wild(_))
            && let Some(local_init) = &local.init
            && !expr_contains_try(&local_init.expr)
        {
            let line = line_of(syn::spanned::Spanned::span(&local.pat));
            self.discarding.push(Some(line));
            syn::visit::visit_stmt(self, node);
            self.discarding.pop();
        } else {
            syn::visit::visit_stmt(self, node);
        }
    }

    fn visit_expr_path(&mut self, node: &'ast syn::ExprPath) {
        if node.qself.is_none()
            && let Some(ident) = single_ident(&node.path)
            && self.names.contains(&ident.to_string())
        {
            let discard = self.discarding.last().copied().flatten();
            self.found
                .entry(ident.to_string())
                .or_default()
                .push(discard);
        }
        syn::visit::visit_expr_path(self, node);
    }

    /// Nested functions cannot capture the handler's parameters, so their
    /// guard-named paths are not this handler's reads.
    fn visit_item_fn(&mut self, _node: &'ast ItemFn) {}
}

/// Whether an expression contains a `?` — the difference between
/// `let _ = auth?;` (propagated: the guard ran) and `let _ = auth;`
/// (discarded: the guard never runs).
fn expr_contains_try(expr: &Expr) -> bool {
    struct TryFinder(bool);
    impl<'ast> Visit<'ast> for TryFinder {
        fn visit_expr_try(&mut self, _node: &'ast syn::ExprTry) {
            self.0 = true;
        }
    }
    let mut finder = TryFinder(false);
    finder.visit_expr(expr);
    finder.0
}

/// The guard-propagation check over every route handler: report a guard
/// binding that is never read, or whose every read is a `let _ = …`
/// right-hand side with no `?`.
fn guard_findings(prog: &Program) -> Vec<GuardFinding> {
    let mut out = Vec::new();
    for func in &prog.funcs {
        if !func.is_handler || func.guard_params.is_empty() {
            continue;
        }
        let names: BTreeSet<String> = func
            .guard_params
            .iter()
            .map(|guard| guard.binding.clone())
            .collect();
        let mut scan = GuardScan {
            names: &names,
            found: BTreeMap::new(),
            discarding: Vec::new(),
        };
        scan.visit_block(&func.body);

        for guard in &func.guard_params {
            let occurrences: &[Option<usize>] = scan
                .found
                .get(&guard.binding)
                .map_or(&[][..], Vec::as_slice);
            let file = prog.files[func.file].clone();
            if occurrences.is_empty() {
                out.push(GuardFinding {
                    file,
                    line: guard.line,
                    handler: func.display.clone(),
                    guard_type: guard.ty.clone(),
                    binding: guard.binding.clone(),
                    kind: GuardKind::NeverRead,
                    snippet: prog.snippet(func.file, guard.line),
                });
                continue;
            }
            if occurrences.iter().all(Option::is_some) {
                let line = occurrences
                    .iter()
                    .flatten()
                    .copied()
                    .min()
                    .unwrap_or(guard.line);
                out.push(GuardFinding {
                    file,
                    line,
                    handler: func.display.clone(),
                    guard_type: guard.ty.clone(),
                    binding: guard.binding.clone(),
                    kind: GuardKind::Discarded,
                    snippet: prog.snippet(func.file, line),
                });
            }
        }
    }
    out
}

// ── Driver ───────────────────────────────────────────────────────────────────

/// Analyse the crate: parse, seed the fixpoint from the route handlers,
/// iterate to convergence, then run the guard-propagation check. Pure —
/// source in, findings out (module docs).
#[must_use]
pub fn analyze(sources: &[(String, String)]) -> Report {
    let mut errors = Vec::new();
    let prog = build_program(sources, &mut errors);
    let mut state = State::new();

    // Every parameter carries its own summary marker; handler seeds and
    // their (empty) base chains start the taint.
    for (id, func) in prog.funcs.iter().enumerate() {
        for (index, names) in func.params.iter().enumerate() {
            for name in names {
                state.add_to_slot(
                    id,
                    name,
                    &Taint {
                        seeds: BTreeSet::new(),
                        markers: BTreeSet::from([index]),
                    },
                );
            }
        }
    }
    for (seed_id, seed) in prog.seeds.iter().enumerate() {
        state.add_to_slot(seed.handler, &seed.param, &Taint::from_seed(seed_id));
        state.chains.insert((seed.handler, seed_id), Vec::new());
    }
    state.changed = false;

    let mut iterations = 0;
    let converged = loop {
        iterations += 1;
        state.changed = false;
        for id in 0..prog.funcs.len() {
            let body = &prog.funcs[id].body;
            let markers = {
                let mut walker = Walker::new(&prog, &mut state, id);
                let tail = walker.block(body);
                let mut markers = std::mem::take(&mut walker.ret_markers);
                markers.extend(tail.markers);
                markers
            };
            state.grow_summary(id, markers);
        }
        if !state.changed {
            break true;
        }
        if iterations >= ITERATION_LIMIT {
            break false;
        }
    };
    if !converged {
        errors.push(Error::IterationLimit {
            iterations: ITERATION_LIMIT,
        });
    }

    let mut sinks: Vec<SinkFinding> = state.sinks.into_values().collect();
    sinks.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.kind.cmp(&b.kind))
            .then(a.seed.cmp(&b.seed))
    });
    let mut guards = guard_findings(&prog);
    guards.sort_by(|a, b| {
        a.file
            .cmp(&b.file)
            .then(a.line.cmp(&b.line))
            .then(a.handler.cmp(&b.handler))
            .then(a.binding.cmp(&b.binding))
    });
    Report {
        sinks,
        guards,
        errors,
    }
}

// ── Registry ─────────────────────────────────────────────────────────────────

/// The reason attached to a finding that has no registry entry yet.
const UNREGISTERED: &str = "UNREGISTERED — add to backend/reachability-registry.txt with a reason";

/// One line of `backend/reachability-registry.txt`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryEntry {
    /// `taint-expect`, `guard-discarded`, … (the finding's kind string).
    pub kind: String,
    /// Source file the site lives in.
    pub file: String,
    /// Enclosing function (the handler, for guard entries).
    pub function: String,
    /// Seed (sinks) or `binding:GuardType` (guards).
    pub subject: String,
    /// The site's source line, trimmed.
    pub snippet: String,
    /// Why the site is known — required, and where its class lives.
    pub reason: String,
}

impl RegistryEntry {
    /// The entry's identity: the same five fields the finding keys on.
    #[must_use]
    pub fn key(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.kind, self.file, self.function, self.subject, self.snippet
        )
    }

    /// The entry rendered as a registry line.
    #[must_use]
    pub fn line(&self) -> String {
        format!("{}\t{}", self.key(), self.reason)
    }
}

/// Parse the registry text: `kind\tfile\tfunction\tsubject\tsnippet\treason`
/// per line; blank lines and `#` comments are skipped. The first malformed
/// line, empty reason, or duplicate key is returned as
/// [`Error::RegistryEntry`] so a broken registry fails the gate instead of
/// being partially honoured.
pub fn parse_registry(text: &str) -> Result<Vec<RegistryEntry>, Error> {
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for (position, raw) in text.lines().enumerate() {
        let line = position + 1;
        let trimmed = raw.trim_end();
        if trimmed.trim().is_empty() || trimmed.trim_start().starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = trimmed.splitn(6, '\t').collect();
        if fields.len() < 6 {
            return Err(Error::RegistryEntry {
                line,
                error: format!(
                    "expected 6 tab-separated fields (kind, file, function, subject, \
                     snippet, reason), got {}",
                    fields.len()
                ),
            });
        }
        if fields[5].trim().is_empty() {
            return Err(Error::RegistryEntry {
                line,
                error: "the reason field must not be empty".to_string(),
            });
        }
        let entry = RegistryEntry {
            kind: fields[0].to_string(),
            file: fields[1].to_string(),
            function: fields[2].to_string(),
            subject: fields[3].to_string(),
            snippet: fields[4].to_string(),
            reason: fields[5].trim().to_string(),
        };
        if !seen.insert(entry.key()) {
            return Err(Error::RegistryEntry {
                line,
                error: format!("duplicate entry for key {}", entry.key()),
            });
        }
        entries.push(entry);
    }
    Ok(entries)
}

/// The gate's verdict: findings with no registry entry, and registry entries
/// whose site no longer exists. Both lists are rendered in registry format
/// and sorted, so the failure output is stable and reviewable.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GateDiff {
    /// Analysis findings not present in the registry — a *new* site.
    pub new_sites: Vec<String>,
    /// Registry entries no analysis finding matches — a *stale* entry.
    pub stale_entries: Vec<String>,
}

impl GateDiff {
    /// Whether the registry and the analysis agree.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.new_sites.is_empty() && self.stale_entries.is_empty()
    }

    /// The failure message: offending sites listed, one per line.
    #[must_use]
    pub fn message(&self) -> String {
        let mut out = String::new();
        if !self.new_sites.is_empty() {
            out.push_str("new reachability sites not in the registry:\n");
            for site in &self.new_sites {
                out.push_str(site);
                out.push('\n');
            }
        }
        if !self.stale_entries.is_empty() {
            out.push_str("stale reachability registry entries (site no longer exists):\n");
            for entry in &self.stale_entries {
                out.push_str(entry);
                out.push('\n');
            }
        }
        out.push_str(
            "Fix: register new sites in backend/reachability-registry.txt with a reason \
             (and a class), or remove entries whose sites are gone.",
        );
        out
    }
}

/// Compare analysis output against the registry. The gate fails when either
/// list is non-empty: a *new* site (an unreviewed finding) or a *stale*
/// entry (the registry no longer describes the tree).
#[must_use]
pub fn gate_diff(entries: &[RegistryEntry], report: &Report) -> GateDiff {
    let registered: BTreeSet<String> = entries.iter().map(RegistryEntry::key).collect();
    let mut found: BTreeSet<String> = BTreeSet::new();
    let mut new_sites = Vec::new();

    for finding in &report.sinks {
        let key = finding.registry_key();
        if !registered.contains(&key) {
            new_sites.push(finding.registry_line(UNREGISTERED));
        }
        found.insert(key);
    }
    for finding in &report.guards {
        let key = finding.registry_key();
        if !registered.contains(&key) {
            new_sites.push(finding.registry_line(UNREGISTERED));
        }
        found.insert(key);
    }

    let mut stale_entries: Vec<String> = entries
        .iter()
        .filter(|entry| !found.contains(&entry.key()))
        .map(RegistryEntry::line)
        .collect();
    new_sites.sort_unstable();
    stale_entries.sort_unstable();
    GateDiff {
        new_sites,
        stale_entries,
    }
}
