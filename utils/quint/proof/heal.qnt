// Heal proof: drain+heal suffix over the reduced instance (3 paths / 2 roots).
//
// This module is a feasibility check for the ghost-row shape only. It is not
// the universal shape init and does not carry the work measure or the other
// shape inits from the execution spec.
//
// `initGhostRowWitness` starts from the base initial state (`initCompletion`)
// and removes `p1` from the filesystem while keeping its path row, asset
// record and metadata: `GhostRow("p1")`. It assigns all 16 state vars
// explicitly. It does not conjoin `initCompletion` with a second `files'`
// assignment because Quint actions are formulas: two assignments to the same
// primed var are a conjunction, not an override, so the pair would be
// unsatisfiable.
module heal {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3"),
    HASHES = 0.to(1),
    SMALL = 0.to(1),
    SECOND_ROOT_PATHS = Set("p3"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }],
  ).* from "../pipeline/merged"

  // GhostRow("p1"): the row survives, the file does not.
  action initGhostRowWitness: bool = all {
    files' = PATHS
      .mapBy(p => initialFile(p))
      .set("p1", { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }),
    generations' = PATHS.mapBy(_ => 0),
    intake' = Set(),
    assetById' = PATHS.fold(Map(0 -> initialAlbumAssetRecord), (acc, p) => acc.put(initialId(p), initialAssetRecord(p))),
    assetByPath' = PATHS.mapBy(p => initialId(p)),
    dupeIndex' = HASHES.mapBy(h => PATHS.filter(p => initialFile(p).assetHash == h).map(p => initialId(p))),
    metadataPayload' = PATHS.fold(Map(0 -> initialAlbumMetadataRecord), (acc, p) => acc.put(initialId(p), initialMetadataRecord(p))),
    thumbPresent' = HASHES.mapBy(h => h == 0),
    sweepPending' = ROOTS.mapBy(_ => false),
    sweepBatch' = Map(),
    leasedPaths' = Set(),
    nextAssetId' = PATHS.size() + 1,
    unprocessable' = Set(),
    phase' = 0,
    scannedRoots' = Set(),
    faults' = 0,
  }

  // Anti-vacuity: the heal antecedent is reachable from the ghost-row init.
  val HealAntecedentReached: bool =
    not(phase == 2 and scannedRoots == ROOTS and Quiescent)

  // Minimal heal scheduler: drain, scan both roots, sweep and apply worker
  // work. Pipeline actions stutter the phase machine through `Stay`; the
  // completion actions assign it themselves. Environment actions, untracked
  // `ScanRoot`, leases, rotation, submits and API actions are excluded, so
  // generations and the filesystem stay fixed during the heal.
  action stepHealGhost: bool = any {
    BeginDrain,
    BeginScan,
    CompleteScan("r1"),
    CompleteScan("r2"),
    all { Stay(SweepEnumerate("r1")) },
    all { Stay(SweepEnumerate("r2")) },
    all { Stay(SweepRecheck) },
    all { Stay(SweepRemoveBatchApply) },
    all { Stay(SweepRoot("r1")) },
    all { Stay(SweepRoot("r2")) },
    all { Stay(WorkerApplySuccess) },
    all { Stay(WorkerSkipSuperseded) },
    all { Stay(WorkerSkipStale) },
    all { Stay(WorkerSkipUnchanged) },
    all { Stay(WorkerOpenFails) },
    all { Stay(WorkerRemoveApply) },
    Skip,
  }
}
