// Canonical-bounds entry point for the inductive lane: the redesigned core
// model at the bounds the suite's runs assume (five paths, three content
// hashes, three small mtime/len values, two roots). Per-slice regression
// runs execute through this entry (`quint test` on `inductive-core.qnt`
// directly cannot bind the module consts). Proof entries at the reduced
// instance live in `inductive-proof.qnt` (one import site), not here.
module inductive_entry_canonical {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3", "p4", "p5"),
    HASHES = 0.to(2),
    SMALL = 0.to(2),
    SECOND_ROOT_PATHS = Set("p5"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }, { src: "p2", dst: "p4" }],
  ).* from "./inductive-core"
}
