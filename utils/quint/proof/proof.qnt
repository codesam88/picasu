// Shape catalog and heal proof for the G1 covering lemma.
//
// Each shape is one negated conjunct of `Converged and AspectsAligned`,
// projected over the reduced instance (3 paths / 2 roots). Shapes do not
// mention `generations`: the goal never reads generation values, and the
// heal section fixes generations separately.
//
// Notation: `id = assetByPath.get(p)` when the path row exists and
// `h = assetById.get(id).contentHash` when the asset record exists. Reads are
// guarded the same way `Converged` and `AspectAligned` guard theirs.
//
// The heal section runs a drain+heal suffix over the reduced instance
// (3 paths / 2 roots) from the witness and family inits below. The witness
// inits start from a converged base state and inject one shape. Each assigns all 16 state vars explicitly. They do not conjoin
// `initCompletion` (or `init`) with a second `files'` assignment because Quint
// actions are formulas: two assignments to the same primed var are a
// conjunction, not an override, so the pair would be unsatisfiable.
module proof {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3"),
    HASHES = 0.to(1),
    SMALL = 0.to(1),
    SECOND_ROOT_PATHS = Set("p3"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }],
  ).* from "../pipeline/merged"

  // ---------- section: shape catalog ----------
  // MissingRow: the file is present but no path row exists.
  def MissingRow(p: str): bool =
    files.get(p).present and not(assetByPath.keys().contains(p))

  // GhostRow: the path row outlives an absent file.
  def GhostRow(p: str): bool =
    not(files.get(p).present) and assetByPath.keys().contains(p)

  // TornRow: a present file has a path row whose asset record is gone.
  def TornRow(p: str): bool =
    files.get(p).present and
    assetByPath.keys().contains(p) and
    not(assetById.keys().contains(assetByPath.get(p)))

  // WrongPath: the asset record exists but names a different path.
  def WrongPath(p: str): bool =
    files.get(p).present and
    assetByPath.keys().contains(p) and
    assetById.keys().contains(assetByPath.get(p)) and
    assetById.get(assetByPath.get(p)).path != p

  // StaleContent: the record's content hash differs from the file bytes.
  def StaleContent(p: str): bool =
    files.get(p).present and
    assetByPath.keys().contains(p) and
    assetById.keys().contains(assetByPath.get(p)) and
    assetById.get(assetByPath.get(p)).contentHash != files.get(p).assetHash

  // MissingMeta: the path row has no metadata payload.
  def MissingMeta(p: str): bool =
    assetByPath.keys().contains(p) and
    not(metadataPayload.keys().contains(assetByPath.get(p)))

  // StaleMetaHash: metadata content snapshot disagrees with the file bytes.
  def StaleMetaHash(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    metadataPayload.get(assetByPath.get(p)).assetHash != files.get(p).assetHash

  // StaleMtime: metadata mtime disagrees with the file mtime.
  def StaleMtime(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    metadataPayload.get(assetByPath.get(p)).modified != files.get(p).mtime

  // StaleSize: metadata file size disagrees with the file length.
  def StaleSize(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    metadataPayload.get(assetByPath.get(p)).fileSize != files.get(p).len

  // StaleSidecar: metadata sidecar id disagrees with the file sidecar.
  def StaleSidecar(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    metadataPayload.get(assetByPath.get(p)).sidecar != files.get(p).sidecar

  // NotConvergedFlag: the metadata converged flag is still unset.
  def NotConvergedFlag(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    not(metadataPayload.get(assetByPath.get(p)).converged)

  // MissingThumb: a media record (contentHash >= 0) has no thumbnail.
  def MissingThumb(p: str): bool =
    assetByPath.keys().contains(p) and
    assetById.keys().contains(assetByPath.get(p)) and
    assetById.get(assetByPath.get(p)).contentHash >= 0 and
    (not(thumbPresent.keys().contains(assetById.get(assetByPath.get(p)).contentHash)) or
      not(thumbPresent.get(assetById.get(assetByPath.get(p)).contentHash)))

  // MissingDupe: a media record (contentHash >= 0) is absent from its
  // content-hash bucket in the dupe index.
  def MissingDupe(p: str): bool =
    assetByPath.keys().contains(p) and
    assetById.keys().contains(assetByPath.get(p)) and
    assetById.get(assetByPath.get(p)).contentHash >= 0 and
    (not(dupeIndex.keys().contains(assetById.get(assetByPath.get(p)).contentHash)) or
      not(dupeIndex.get(assetById.get(assetByPath.get(p)).contentHash).contains(assetByPath.get(p))))

  // QueuedIntake: the intake queue is not empty.
  val QueuedIntake: bool = intake.size() > 0

  // ShapeCatalog: some catalog shape holds for a processable path, or work is
  // still queued. PendingSweep and LeaseHeld are not goal shapes; they belong
  // to the Quiescent antecedent.
  val ShapeCatalog: bool =
    QueuedIntake or
    PATHS.exists(p =>
      not(p.in(unprocessable)) and (
        MissingRow(p) or
        GhostRow(p) or
        TornRow(p) or
        WrongPath(p) or
        StaleContent(p) or
        MissingMeta(p) or
        StaleMetaHash(p) or
        StaleMtime(p) or
        StaleSize(p) or
        StaleSidecar(p) or
        NotConvergedFlag(p) or
        MissingThumb(p) or
        MissingDupe(p)
      )
    )

  // Covering: every violation of the goal is explained by the catalog.
  val Covering: bool =
    not(Converged and AspectsAligned) implies ShapeCatalog

  // Covering counterexample: a state that violates the goal but satisfies no
  // catalog shape. A counterexample to CoveringNeg is a completeness gap.
  val CoveringNeg: bool =
    not(Converged and AspectsAligned) and not(ShapeCatalog)

  // Shape soundness: a catalog shape must itself be a goal violation. A
  // counterexample to the negation means a shape is too broad and its heal
  // obligation is weaker than claimed.
  val ShapeOverBroad: bool =
    ShapeCatalog and Converged and AspectsAligned

  // Shape soundness in positive form, the shape the soundness check runs.
  val ShapeSound: bool = not(ShapeOverBroad)

  // ---------- section: reachability invariants ----------
  // Reachability invariants: characterize the states reachable from
  // initCompletion. Used to constrain universal shape inits; each is
  // falsification-checked below and becomes an inductive obligation.
  def DupeCompleteRow(p: str): bool =
    not(assetByPath.keys().contains(p)) or
    not(assetById.keys().contains(assetByPath.get(p))) or
    assetById.get(assetByPath.get(p)).contentHash < 0 or
    (dupeIndex.keys().contains(assetById.get(assetByPath.get(p)).contentHash) and
      dupeIndex.get(assetById.get(assetByPath.get(p)).contentHash).contains(assetByPath.get(p)))

  def MetadataCompleteRow(p: str): bool =
    not(assetByPath.keys().contains(p)) or
    not(assetById.keys().contains(assetByPath.get(p))) or
    metadataPayload.keys().contains(assetByPath.get(p))

  def PathConsistentRow(p: str): bool =
    not(assetByPath.keys().contains(p)) or
    not(assetById.keys().contains(assetByPath.get(p))) or
    assetById.get(assetByPath.get(p)).path == p

  def ConvergedFlagSetRow(p: str): bool =
    not(assetByPath.keys().contains(p)) or
    not(assetById.keys().contains(assetByPath.get(p))) or
    not(metadataPayload.keys().contains(assetByPath.get(p))) or
    metadataPayload.get(assetByPath.get(p)).converged

  val ReachableInv: bool =
    (PATHS.forall(p => DupeCompleteRow(p) and MetadataCompleteRow(p) and PathConsistentRow(p) and ConvergedFlagSetRow(p)))
    and WithinBounds
    and AssetIdKeyCoherent

  val ReachableInvNeg: bool = not(ReachableInv)

  // ---------- section: reachability witnesses ----------
  // Anti-vacuity for StaleContent. The content draw in ExternalModify /
  // ForceModify can be a no-op, so there is no deterministic single run:
  // this predicate is violated exactly when the shape is reachable.
  val StaleContentReached: bool = not(StaleContent("p1"))

  // Reachability witnesses. Each deterministic run injects one shape from
  // `initCompletion`; the trailing `Skip` keeps the phase vars assigned.
  run ghostRowReachable = initCompletion
    .then(Stay(ExternalDelete("p1")))
    .then(all { assert(GhostRow("p1")), Skip })

  // ApiMoveFsOnly moves files without rows, but every initial path has a row,
  // so first drop p1's file and row, then move p2's file into the rowless p1.
  run missingRowReachable = initCompletion
    .then(Stay(ExternalDelete("p1")))
    .then(Stay(SubmitWatcherRemove("p1")))
    .then(Stay(WorkerRemoveApply))
    .then(Stay(ApiMoveFsOnly("p2", "p1")))
    .then(all { assert(MissingRow("p1")), Skip })

  run tornRowReachable = initCompletion
    .then(BeginDrain)
    .then(all { RemoveIdOnly("p1"), faults' = faults + 1, phase' = 0, scannedRoots' = scannedRoots })
    .then(all { assert(TornRow("p1")), Skip })

  run staleSidecarReachable = initCompletion
    .then(Stay(ExternalSidecarCreate("p1")))
    .then(all { assert(StaleSidecar("p1")), Skip })

  run missingThumbReachable = initCompletion
    .then(BeginDrain)
    .then(ThumbLossReboot)
    .then(all { assert(MissingThumb("p1")), Skip })

  run queuedIntakeReachable = initCompletion
    .then(Stay(SubmitScan("p1")))
    .then(all { assert(QueuedIntake), Skip })

  // ---------- section: heal scheduler ----------
  // Anti-vacuity: the heal antecedent is reachable from the ghost-row init.
  val HealAntecedentReached: bool =
    not(phase == 2 and scannedRoots == ROOTS and Quiescent)

  // Minimal drain+heal scheduler used for the first shape checks: drain, scan
  // both roots, sweep and apply worker work. The full action list from the
  // plan is added later. Pipeline actions stutter the phase machine through
  // `Stay`; the completion actions assign it themselves. Environment actions,
  // untracked `ScanRoot`, lease acquisition, rotation write, submits and API
  // actions are excluded, so generations and the filesystem stay fixed during
  // the heal. The two lease-window finishers (`MoveRowRewrite`, `RotateFinish`)
  // only release leases and rewrite rows/metadata, so they are included.
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
    all { Stay(MoveRowRewrite("p1", "p2")) },
    all { Stay(MoveRowRewrite("p1", "p3")) },
    all { Stay(MoveRowRewrite("p2", "p1")) },
    all { Stay(MoveRowRewrite("p2", "p3")) },
    all { Stay(MoveRowRewrite("p3", "p1")) },
    all { Stay(MoveRowRewrite("p3", "p2")) },
    all { Stay(RotateFinish("p1")) },
    all { Stay(RotateFinish("p2")) },
    all { Stay(RotateFinish("p3")) },
    Skip,
  }

  // ---------- section: heal families ----------
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

  // Cross-shape combination family: the three paths diverge independently and
  // the heal must repair several kinds at once. The row, when it exists, maps
  // the path to its own initialId, so PathConsistent holds by construction.
  // All content hashes are 0, matching the initial file bytes.
  //
  // Drawn dimensions:
  //   - lease windows: a rotate window (`rotateWindow`) is a path whose file
  //     sidecar is 1, whose metadata sidecar is still 0, and which holds a
  //     lease. It is the state `RotateWriteSidecar` leaves behind
  //     (merged.qnt: file sidecar toggled, `leasedPaths` plus p); `RotateFinish`
  //     requires a held lease, a sidecar mismatch, a present file and a
  //     resolvable record plus metadata, and it rewrites the metadata sidecar
  //     and releases the lease.
  //   - move windows: at most one drawn move `src>dst`, laid out as
  //     `ApiMoveFsOnly` leaves it (merged.qnt): the source becomes `ghost`
  //     (file absent, row and record kept), the destination becomes
  //     `missingRow` (file present with the source's bytes, no row and no
  //     record), and both endpoints are leased. `MoveRowRewrite` requires a row
  //     for the source, its record, a lease on both endpoints, source absent
  //     and destination present; it rewrites the record path, moves the row and
  //     releases both leases.
  //     `ApiMoveFsOnly` copies the source's file fields, so a move of a stale
  //     file leaves stale bytes at the destination. The `mvStale` draw opens
  //     that window: the destination takes the `missingRowStale` kind (stale
  //     file bytes, no row, record or metadata). After `MoveRowRewrite` moves
  //     the source's record and metadata, the destination's metadata still
  //     holds the source's pre-move snapshot while the file bytes are stale, so
  //     the scan must re-index the destination; that is the behavior under
  //     test. With `mv == "none"` the flag has no effect.
  //     The move overrides the per-path kind of both endpoints (`effKind`), so
  //     a drawn move forces the source to the ghost kind and the destination to
  //     the missing-row kind, and both endpoints are leased. A move draw
  //     therefore also satisfies the ghost-presence guard.
  //   - stale dupe entries: dupeIndex[0] holds the ids of rows whose records
  //     exist (DupeCompleteRow) plus a `staleExtra` subset of the initial ids.
  //     An id can sit in the bucket without a record: RemoveIdOnly drops the
  //     record and metadata while keeping dupeIndex, and DupeCompleteRow only
  //     requires entries for rows whose records exist, so the union stays
  //     inside the reachable closure. AspectAligned does not forbid extra
  //     bucket ids, so a stale entry is not itself a goal violation.
  //   - intake: zero or one queued item for any path and kind
  //     (watcher/remove), snapped to the drawn file state for that path at the
  //     drawn generation. A (path, kind, current-snapshot) item is reachable:
  //     submits snapshot the file at submit time, and later environment changes
  //     only make an item stale or moot, which the worker actions handle
  //     (apply, skip stale, skip superseded, open-fails, remove).
  //   - unprocessable: any subset of PATHS. `MarkUnprocessable` unions without
  //     a guard, so every subset is reachable, and `stepHeal` never changes it.
  //     Unprocessable paths are excluded from the goal (`Converged`,
  //     `AspectAligned`) and from `ShapeCatalog`, so a drawn subset only
  //     removes heal obligations; it is drawn to check that exclusion holds.
  //   - generations: drawn uniformly in 0..2. `ScanRoot` and the submit
  //     actions bump a path's generation, so nonzero values occur after submits
  //     and applies. `MoveRowRewrite` has no generation guard; uniform nonzero
  //     draws check the rewrite and the sweep/worker guards at nonzero
  //     generations.
  //
  // Pinned dimensions (logged limitations, not drawn):
  //   - full field draws per path: a kind fixes the whole field tuple, so e.g.
  //     a stale file with a moved sidecar is not drawn.
  //
  // Family size: 7 kinds, 7 move options (`none` plus the six ordered pairs),
  // 2 thumbnail flags, 4 sweepPending sets, 4 staleExtra sets and 12 intake
  // draws (hasItem x 3 item paths x 2 item kinds). With the guard applied the
  // raw draw product is (6 x 7^3 + (7^3 - 6^3)) x 384 = 839,040 combinations.
  // TLC reports 16,856 distinct initial states: the endpoint overrides collapse
  // every kind drawn at a move endpoint into the ghost/missing-row kind, and the
  // staleExtra union collapses against the derived dupe bucket. The three new
  // dimensions multiply that base by 8 unprocessable subsets x 2 stale-move
  // flags x 3 generations (16,856 x 8 x 2 x 3 = 809,088 as a raw bound). The
  // stale-move flag only distinguishes states that draw a move, measured at
  // 22,232 initial states for that dimension alone, so the family projects to
  // about 22,232 x 8 x 3 = 533,568 distinct initial states and roughly 40M raw
  // draw candidates. That exceeds the run cap, so the dimensions are attributed
  // one at a time by the recorded per-dimension runs.
  val ComboKinds: Set[str] =
    Set("ok", "stale", "staleSidecar", "ghost", "missingRow", "torn", "rotateWindow")

  // The kind drawn for a path; the reduced instance is exactly p1..p3.
  pure def comboKind(p: str, k1: str, k2: str, k3: str): str =
    if (p == "p1") k1 else (if (p == "p2") k2 else k3)

  // Move-window source and destination. The six ordered pairs over PATHS are
  // the drawn moves; `none` draws no move and both accessors return "", which
  // no path equals.
  pure def moveSrc(mv: str): str =
    if (mv == "p1>p2" or mv == "p1>p3") "p1"
    else if (mv == "p2>p1" or mv == "p2>p3") "p2"
    else if (mv == "p3>p1" or mv == "p3>p2") "p3"
    else ""

  pure def moveDst(mv: str): str =
    if (mv == "p1>p2" or mv == "p3>p2") "p2"
    else if (mv == "p1>p3" or mv == "p2>p3") "p3"
    else if (mv == "p2>p1" or mv == "p3>p1") "p1"
    else ""

  // The kind that holds for a path once a drawn move is applied on top of the
  // per-path kinds: the move window forces the source to the ghost kind and the
  // destination to the missing-row kind, or to its stale-byte variant when the
  // moved file bytes are stale.
  pure def effKind(p: str, k1: str, k2: str, k3: str, mv: str, mvStale: bool): str =
    if (moveSrc(mv) == p) "ghost"
    else if (moveDst(mv) == p) (if (mvStale) "missingRowStale" else "missingRow")
    else comboKind(p, k1, k2, k3)

  // File bytes for a kind: `stale` moves mtime/len, `staleSidecar` moves the
  // sidecar id, `rotateWindow` moves the sidecar id and keeps the lease, `ghost`
  // removes the file, `missingRowStale` holds moved stale bytes with no row,
  // the rest keep the initial bytes.
  pure def comboFile(p: str, kind: str): FileState =
    if (kind == "stale") { present: true, assetHash: 0, mtime: 1, len: 1, sidecar: 0 }
    else if (kind == "staleSidecar") { present: true, assetHash: 0, mtime: 0, len: 0, sidecar: 1 }
    else if (kind == "rotateWindow") { present: true, assetHash: 0, mtime: 0, len: 0, sidecar: 1 }
    else if (kind == "ghost") { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }
    else if (kind == "missingRowStale") { present: true, assetHash: 0, mtime: 1, len: 1, sidecar: 0 }
    else initialFile(p)

  // A row exists for every kind except the missing-row kinds. A rotate window
  // is opened on an existing row (`RotateWriteSidecar` requires it).
  pure def comboRowPresent(kind: str): bool =
    kind != "missingRow" and kind != "missingRowStale"

  // Record and metadata exist for every kind except the missing-row kinds and
  // `torn`. The rotate window keeps both: its metadata differs only in the
  // sidecar id.
  pure def comboRecordPresent(kind: str): bool =
    kind != "missingRow" and kind != "missingRowStale" and kind != "torn"

  action initShapeCombination: bool = {
    nondet k1 = ComboKinds.oneOf()
    nondet k2 = ComboKinds.oneOf()
    nondet k3 = ComboKinds.oneOf()
    nondet mv = Set("none", "p1>p2", "p1>p3", "p2>p1", "p2>p3", "p3>p1", "p3>p2").oneOf()
    nondet mvStale = Set(true, false).oneOf()
    nondet th0 = Set(true, false).oneOf()
    nondet sp = Set(Set(), Set("r1"), Set("r2"), Set("r1", "r2")).oneOf()
    nondet staleExtra = Set(Set(), Set(1), Set(2), Set(3)).oneOf()
    nondet hasItem = Set(true, false).oneOf()
    nondet ip = PATHS.oneOf()
    nondet ik = Set("watcher", "remove").oneOf()
    nondet up = Set(Set(), Set("p1"), Set("p2"), Set("p3"), Set("p1", "p2"), Set("p1", "p3"), Set("p2", "p3"), Set("p1", "p2", "p3")).oneOf()
    nondet gen = 0.to(2).oneOf()
    all {
      // At least one ghost: the family must exercise the sweep-prune path. A
      // drawn move forces its source to the ghost kind, so it satisfies this
      // guard as well.
      mv != "none" or k1 == "ghost" or k2 == "ghost" or k3 == "ghost",
      files' = PATHS.mapBy(p => comboFile(p, effKind(p, k1, k2, k3, mv, mvStale))),
      generations' = PATHS.mapBy(_ => gen),
      // The item snapshots the drawn file state: `files` is the pre-state in an
      // init action, so the item is built from `comboFile` instead. Its
      // generation is the drawn uniform generation, so the worker guards match
      // as they do at generation 0.
      intake' = if (hasItem)
        Set({
          path: ip,
          generation: gen,
          kind: ik,
          mtime: comboFile(ip, effKind(ip, k1, k2, k3, mv, mvStale)).mtime,
          len: comboFile(ip, effKind(ip, k1, k2, k3, mv, mvStale)).len,
          sidecar: comboFile(ip, effKind(ip, k1, k2, k3, mv, mvStale)).sidecar,
          assetHash: comboFile(ip, effKind(ip, k1, k2, k3, mv, mvStale)).assetHash,
        })
        else Set(),
      assetById' = PATHS
        .filter(p => comboRecordPresent(effKind(p, k1, k2, k3, mv, mvStale)))
        .fold(Map(0 -> initialAlbumAssetRecord), (acc, p) => acc.put(initialId(p), initialAssetRecord(p))),
      assetByPath' = PATHS
        .filter(p => comboRowPresent(effKind(p, k1, k2, k3, mv, mvStale)))
        .mapBy(p => initialId(p)),
      dupeIndex' = Map(
        0 -> PATHS
          .filter(p => comboRecordPresent(effKind(p, k1, k2, k3, mv, mvStale)))
          .map(p => initialId(p))
          .union(staleExtra),
        1 -> Set(),
      ),
      metadataPayload' = PATHS
        .filter(p => comboRecordPresent(effKind(p, k1, k2, k3, mv, mvStale)))
        .fold(Map(0 -> initialAlbumMetadataRecord), (acc, p) => acc.put(initialId(p), initialMetadataRecord(p))),
      thumbPresent' = Map(0 -> th0, 1 -> false),
      sweepPending' = ROOTS.mapBy(r => r.in(sp)),
      sweepBatch' = Map(),
      // Every rotate window holds its path lease; a drawn move leases both
      // endpoints. Both are released by the matching finish action.
      leasedPaths' =
        PATHS.filter(p => effKind(p, k1, k2, k3, mv, mvStale) == "rotateWindow")
          .union(if (mv == "none") Set() else Set(moveSrc(mv), moveDst(mv))),
      nextAssetId' = 4,
      unprocessable' = up,
      phase' = 0,
      scannedRoots' = Set(),
      faults' = 0,
    }
  }

  // Durable antecedent witness for the combination family. The large
  // combination-family witness run reaches the antecedent but TLC stalls in
  // violation reporting on it, so the antecedent is machine-checked on one
  // combination-family state instead: all three paths ghost (a file-less row
  // with its record and metadata), no move, no thumbnail, no sweep owed, no
  // stale dupe extra, no intake, no unprocessable path, generation 0. The heal
  // reaches quiescence through the sweep alone; the combination-family heal
  // run records that. Only the drawn fields change here relative to
  // `initShapeCombination`, so the single state is a member of that family.
  action initCombinationWitness: bool = all {
    files' = PATHS.mapBy(p => comboFile(p, effKind(p, "ghost", "ghost", "ghost", "none", false))),
    generations' = PATHS.mapBy(_ => 0),
    intake' = Set(),
    assetById' = PATHS.fold(Map(0 -> initialAlbumAssetRecord), (acc, p) => acc.put(initialId(p), initialAssetRecord(p))),
    assetByPath' = PATHS.mapBy(p => initialId(p)),
    dupeIndex' = Map(0 -> PATHS.map(p => initialId(p)), 1 -> Set()),
    metadataPayload' = PATHS.fold(Map(0 -> initialAlbumMetadataRecord), (acc, p) => acc.put(initialId(p), initialMetadataRecord(p))),
    thumbPresent' = Map(0 -> false, 1 -> false),
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
