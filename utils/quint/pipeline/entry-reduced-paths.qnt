// Reduced (paths only): three paths, canonical HASHES/SMALL. Used to
// measure the PATHS-only reduction impact on the run suite.
module entry_reduced_paths {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3"),
    HASHES = 0.to(2),
    SMALL = 0.to(2),
    SECOND_ROOT_PATHS = Set("p3"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }],
  ).* from "./merged"
}