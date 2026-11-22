// Inductive proof entry at the reduced instance: Domains-only TypeOK.
module inductive_proof {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3"),
    HASHES = 0.to(1),
    SMALL = 0.to(1),
    SECOND_ROOT_PATHS = Set("p3"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }],
  ).* from "./inductive-core"

  pure val IDS = 0.to(PATHS.size() + 1)
  pure val G = 2

  val Domains: bool =
    files.keys() == PATHS and
    generations.in(PATHS.setOfMaps(0.to(G))) and
    assetByPath.in(PATHS.setOfMaps(IDS)) and
    assetById.keys() == IDS and
    metadataPayload.keys() == IDS and
    dupeIndex.in(HASHES.setOfMaps(IDS.powerset())) and
    thumbPresent.in(HASHES.setOfMaps(Set(true, false))) and
    sweepPending.in(ROOTS.setOfMaps(Set(true, false))) and
    sweepBatch.in(PATHS.setOfMaps(Set(-1, 0, 1, 2))) and
    leasedPaths.subseteq(PATHS) and
    unprocessable.subseteq(PATHS) and
    intake.forall(i =>
      i.path.in(PATHS) and
      i.generation.in(0.to(G)) and
      i.kind.in(Set("watcher", "remove", "scan")) and
      i.mtime.in(SMALL) and
      i.len.in(SMALL) and
      i.sidecar.in(SMALL) and
      i.assetHash.in(HASHES)
    ) and
    nextAssetId.in(IDS) and
    phase.in(0.to(2)) and
    scannedRoots.subseteq(ROOTS) and
    faults.in(0.to(1))

  val TypeOK: bool = Domains
}
