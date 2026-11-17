// Canonical-bound entry point: the merged pipeline model at the bounds its
// own declarations define (five paths, three content hashes, three small
// mtime/len values, two roots). `just quint test` executes the runs it
// imports from merged.qnt.
module entry_canonical {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3", "p4", "p5"),
    HASHES = 0.to(2),
    SMALL = 0.to(2),
    SECOND_ROOT_PATHS = Set("p5"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }, { src: "p2", dst: "p4" }],
  ).* from "./merged"
}