// Canonical-bound entry point: the merged pipeline model at the bounds its
// own declarations define (five paths, three content hashes, three small
// mtime/len values, two roots). `just quint test` executes the runs it
// imports from merged.qnt.
module entry_canonical {
  import pipeline_merged.* from "./merged"
}
