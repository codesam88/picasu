// Heal proof: drain+heal suffix over the reduced instance (3 paths / 2 roots).
//
// The witness inits below start from a converged base state and inject one
// shape. Each assigns all 16 state vars explicitly. They do not conjoin
// `initCompletion` (or `init`) with a second `files'` assignment because Quint
// actions are formulas: two assignments to the same primed var are a
// conjunction, not an override, so the pair would be unsatisfiable.
module heal {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3"),
    HASHES = 0.to(1),
    SMALL = 0.to(1),
    SECOND_ROOT_PATHS = Set("p3"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }],
  ).* from "../pipeline/merged"

  // Reachability audit of the shape catalog (by action inspection on this
  // instance). Unreachable shapes carry no heal obligation.
  //
  // Reachable, with a witness init below:
  //   GhostRow      ExternalDelete leaves the path row behind.
  //   MissingRow    ApiMoveFsOnly and the rename windows move files before rows.
  //   TornRow       RemoveIdOnly (N5) drops the record before the row.
  //   StaleContent  ExternalModify / ForceModify change the file bytes;
  //                 StaleMetaHash, StaleMtime and StaleSize share the witness.
  //   StaleSidecar  ExternalSidecarCreate changes the sidecar bytes.
  //   MissingThumb  ThumbLossReboot / LoseThumbs drop the rendition.
  //   QueuedIntake  SubmitScan / SubmitWatcherIndex queue intake.
  //
  // Unreachable, no heal obligation and no witness init:
  //   MissingMeta       no action drops metadata while keeping the row and
  //                     record; RemoveIdOnly drops row, record and metadata
  //                     together, and an apply rewrites metadata.
  //   MissingDupe       no action drops a dupe-index entry while keeping the
  //                     row and record; removal actions drop the row too.
  //   WrongPath         move and rename rewrite the record path and the path
  //                     key together; ApiMoveFsOnly leaves the old record with
  //                     no row (MissingRow), not a record pointing elsewhere.
  //   NotConvergedFlag  the converged flag is only ever written true.

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

  // GhostRow invariant-constrained family. The ghost path is any path; its
  // row points at initialId(ghost), so the record path stays consistent and no
  // id collides. The record and its metadata are dropped together or kept
  // together (recMissing), the hash-0 thumbnail flag is drawn, and the lease
  // dimension is excluded: stepHeal has no lease-release path, so drawing a
  // lease only makes BeginScan unreachable. dupeIndex[0] is the set of ids
  // whose records exist and dupeIndex[1] is empty, matching DupeComplete.
  //
  // 12 states: 3 ghost paths x 2 record/metadata-dropped x 2 hash-0 thumbnail
  // flag. Constraining the semi-universal draws by the reachability invariants
  // collapses the 2304-state family to 12 drawn states; the 2292 dropped draws
  // are unreachable from initCompletion. Every state has
  // files[ghost].present == false and a row for ghost, so GhostRow(ghost)
  // holds throughout.
  action initGhostRowInvariant: bool = {
    nondet ghost = PATHS.oneOf()
    nondet recMissing = Set(true, false).oneOf()
    nondet th0 = Set(true, false).oneOf()
    all {
      files' = PATHS.mapBy(p =>
        if (p == ghost) { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 } else initialFile(p)
      ),
      generations' = PATHS.mapBy(_ => 0),
      intake' = Set(),
      assetById' = PATHS
        .filter(p => not(recMissing) or not(initialId(p) == initialId(ghost)))
        .fold(Map(0 -> initialAlbumAssetRecord), (acc, p) => acc.put(initialId(p), initialAssetRecord(p))),
      assetByPath' = PATHS.mapBy(p => initialId(p)),
      dupeIndex' = Map(0 -> PATHS.filter(p => not(recMissing) or not(initialId(p) == initialId(ghost))).map(p => initialId(p)), 1 -> Set()),
      metadataPayload' = PATHS
        .filter(p => not(recMissing) or not(initialId(p) == initialId(ghost)))
        .fold(Map(0 -> initialAlbumMetadataRecord), (acc, p) => acc.put(initialId(p), initialMetadataRecord(p))),
      thumbPresent' = Map(0 -> th0, 1 -> false),
      sweepPending' = ROOTS.mapBy(_ => false),
      sweepBatch' = Map(),
      leasedPaths' = Set(),
      nextAssetId' = PATHS.size() + 1,
      unprocessable' = Set(),
      phase' = 0,
      scannedRoots' = Set(),
      faults' = 0,
    }
  }

  // StaleContent("p1"): the file bytes changed; the row and metadata still
  // hold the initial hash/mtime/size, so they are stale.
  action initStaleContentWitness: bool = all {
    files' = PATHS
      .mapBy(p => initialFile(p))
      .set("p1", { present: true, assetHash: 1, mtime: 1, len: 1, sidecar: 0 }),
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

  // QueuedIntake: one watcher item for `p1` at generation 0, matching the
  // initial snapshot. Everything else is the converged base state.
  action initQueuedIntakeWitness: bool = all {
    files' = PATHS.mapBy(p => initialFile(p)),
    generations' = PATHS.mapBy(_ => 0),
    intake' = Set({ path: "p1", generation: 0, kind: "watcher", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }),
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

  // MissingRow("p1"): the file is present but its path row is gone.
  action initMissingRowWitness: bool = all {
    files' = PATHS.mapBy(p => initialFile(p)),
    generations' = PATHS.mapBy(_ => 0),
    intake' = Set(),
    assetById' = PATHS.fold(Map(0 -> initialAlbumAssetRecord), (acc, p) => acc.put(initialId(p), initialAssetRecord(p))),
    assetByPath' = PATHS.filter(p => not(p == "p1")).mapBy(p => initialId(p)),
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

  // TornRow("p1"): the path row survives but its asset record is gone.
  // Metadata for id 1 is dropped with the record.
  action initTornRowWitness: bool = all {
    files' = PATHS.mapBy(p => initialFile(p)),
    generations' = PATHS.mapBy(_ => 0),
    intake' = Set(),
    assetById' = PATHS.filter(p => not(p == "p1")).fold(Map(0 -> initialAlbumAssetRecord), (acc, p) => acc.put(initialId(p), initialAssetRecord(p))),
    assetByPath' = PATHS.mapBy(p => initialId(p)),
    dupeIndex' = HASHES.mapBy(h => PATHS.filter(p => initialFile(p).assetHash == h).map(p => initialId(p))),
    metadataPayload' = PATHS.filter(p => not(p == "p1")).fold(Map(0 -> initialAlbumMetadataRecord), (acc, p) => acc.put(initialId(p), initialMetadataRecord(p))),
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

  // StaleSidecar("p1"): the file sidecar changed; metadata keeps sidecar 0.
  action initStaleSidecarWitness: bool = all {
    files' = PATHS
      .mapBy(p => initialFile(p))
      .set("p1", { present: true, assetHash: 0, mtime: 0, len: 0, sidecar: 1 }),
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

  // MissingThumb: hash 0 has no thumbnail, so every media row misses one.
  action initMissingThumbWitness: bool = all {
    files' = PATHS.mapBy(p => initialFile(p)),
    generations' = PATHS.mapBy(_ => 0),
    intake' = Set(),
    assetById' = PATHS.fold(Map(0 -> initialAlbumAssetRecord), (acc, p) => acc.put(initialId(p), initialAssetRecord(p))),
    assetByPath' = PATHS.mapBy(p => initialId(p)),
    dupeIndex' = HASHES.mapBy(h => PATHS.filter(p => initialFile(p).assetHash == h).map(p => initialId(p))),
    metadataPayload' = PATHS.fold(Map(0 -> initialAlbumMetadataRecord), (acc, p) => acc.put(initialId(p), initialMetadataRecord(p))),
    thumbPresent' = HASHES.mapBy(_ => false),
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

  // Minimal drain+heal scheduler used for the first shape checks: drain, scan
  // both roots, sweep and apply worker work. The full action list from the
  // plan is added later. Pipeline actions stutter the phase machine through
  // `Stay`; the completion actions assign it themselves. Environment actions,
  // untracked `ScanRoot`, leases, rotation, submits and API actions are
  // excluded, so generations and the filesystem stay fixed during the heal.
  action stepHeal: bool = any {
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
