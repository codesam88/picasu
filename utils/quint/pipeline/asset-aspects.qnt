// Asset-aspect slice of the index pipeline model.
//
// Extends the eventual-consistency model with per-asset-aspect alignment:
// an asset is aligned only when its file (media bytes + sidecar bytes),
// derived rendition (thumbnail), and DB row agree. The predicates below
// name that agreement; the new actions model mutation windows the main
// model does not (directory moves that touch the filesystem only, partial
// sidecar edits, dropped sidecar watcher events).
module asset_aspects {
  import eventual_consistency_state.* from "./eventual-consistency-state"
  import eventual_consistency_assumptions.* from "./eventual-consistency-assumptions"
  import eventual_consistency_goal.* from "./eventual-consistency-goal"
  import eventual_consistency.* from "./eventual-consistency"

  // @assumes ASS-3
  // @assumes ASS-6
  def AspectAligned(p: str): bool = {
    if (p.in(unprocessable)) {
      true
    } else if (files.get(p).present) {
      rows.keys().contains(p) and
      rows.get(p).assetHash == files.get(p).assetHash and
      rows.get(p).modified == files.get(p).mtime and
      rows.get(p).fileSize == files.get(p).len and
      rows.get(p).sidecar == files.get(p).sidecar and
      rows.get(p).converged and
      rows.get(p).thumb
    } else {
      not(rows.keys().contains(p))
    }
  }

  // @assumes ASS-3
  // @assumes ASS-6
  val AspectsAligned = PATHS.forall(p => AspectAligned(p))

  // @assumes ASS-4
  def ThumbPresent(p: str): bool =
    if (rows.keys().contains(p)) { rows.get(p).thumb } else { false }

  // @operation OP-5
  // @scope SCOPE-5
  // @scope SCOPE-6
  action DirMoveFsOnly: bool = all {
    SUBTREE_PAIRS.foldl(true, (acc, pr) => acc and files.get(pr.src).present and not(files.get(pr.dst).present)),
    files' = SUBTREE_PAIRS.foldl(files, (acc, pr) =>
      acc.set(pr.src, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 })
         .set(pr.dst, { present: true, assetHash: files.get(pr.src).assetHash, mtime: files.get(pr.src).mtime, len: files.get(pr.src).len, sidecar: files.get(pr.src).sidecar })
    ),
    generations' = SUBTREE_PAIRS.foldl(generations, (acc, pr) =>
      acc.set(pr.src, generations.get(pr.src) + 1)
         .set(pr.dst, generations.get(pr.dst) + 1)
    ),
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @operation OP-4
  // @scope SCOPE-5
  // @scope SCOPE-10
  action ApiMoveFsOnly(p: str, q: str): bool = all {
    p.in(PATHS),
    q.in(PATHS),
    not(p == q),
    files.get(p).present,
    not(files.get(q).present),
    rows.keys().contains(p),
    files' = files.set(p, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }).set(q, { present: true, assetHash: files.get(p).assetHash, mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar }),
    generations' = generations.set(p, generations.get(p) + 1).set(q, generations.get(q) + 1),
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths.union(Set(p, q)),
  }

  // @operation OP-9
  // @scope SCOPE-8
  action SweepEnumerate(root: str): bool = all {
    root.in(ROOTS),
    sweepPending.get(root),
    sweepBatch' = PATHS.filter(p => rootOf(p) == root and rows.keys().contains(p) and not(files.get(p).present)).mapBy(p => generations.get(p)),
    sweepPending' = sweepPending.put(root, false),
    files' = files,
    generations' = generations,
    intake' = intake,
    rows' = rows,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    leasedPaths' = leasedPaths,
  }

  // @operation OP-9
  // @scope SCOPE-9
  action SweepRecheck: bool = all {
    sweepBatch.keys().size() > 0,
    sweepBatch' = sweepBatch.keys().filter(p =>
      sweepBatch.get(p) == generations.get(p) and
      not(p.in(leasedPaths)) and
      not(files.get(p).present)
    ).mapBy(p => sweepBatch.get(p)),
    sweepPending' = sweepPending,
    files' = files,
    generations' = generations,
    intake' = intake,
    rows' = rows,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    leasedPaths' = leasedPaths,
  }

  // @operation OP-9
  // @scope SCOPE-11
  action SweepWriterApply: bool = all {
    sweepBatch.keys().size() > 0,
    rows' = rows.keys().filter(p =>
      not (sweepBatch.keys().contains(p) and
           rows.keys().contains(p) and
           not(files.get(p).present))
    ).mapBy(k => rows.get(k)),
    sweepBatch' = Map(),
    sweepPending' = sweepPending,
    files' = files,
    generations' = generations,
    intake' = intake,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    leasedPaths' = leasedPaths,
  }

  // @operation OP-4
  // @scope SCOPE-10
  action MoveLeaseAcquire(p: str, q: str): bool = all {
    p.in(PATHS),
    q.in(PATHS),
    not(p == q),
    files.get(p).present,
    not(files.get(q).present),
    rows.keys().contains(p),
    files' = files,
    rows' = rows,
    generations' = generations.set(p, generations.get(p) + 1).set(q, generations.get(q) + 1),
    intake' = intake,
    sweepPending' = sweepPending,
    sweepBatch' = sweepBatch,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    leasedPaths' = leasedPaths.union(Set(p, q)),
  }

  // @operation OP-4
  // @scope SCOPE-10
  action MoveRowRewrite(p: str, q: str): bool = all {
    p.in(PATHS),
    q.in(PATHS),
    not(p == q),
    rows.keys().contains(p),
    leasedPaths' = leasedPaths.exclude(Set(p, q)),
    rows' = rows.keys().exclude(Set(p)).mapBy(k => rows.get(k)).put(q, rows.get(p)),
    files' = files,
    generations' = generations,
    intake' = intake,
    sweepPending' = sweepPending,
    sweepBatch' = sweepBatch,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @operation OP-7
  // @scope SCOPE-5
  action SidecarEditPartial(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: if (f.sidecar == 0) 1 else 0 }),
    generations' = generations.set(p, generations.get(p) + 1),
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @exempt environment
  // @assumes ASS-6
  action DroppedSidecarModify(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: if (f.sidecar == 0) 1 else 0 }),
    generations' = generations,
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @assumes ASS-6
  run aspectsScanApplyTest = init
    .then(ForceModify("p1"))
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsScanSupersedesEditTest = init
    .then(SubmitScan("p1"))
    .then(AppSidecarEdit("p1"))
    .then(WorkerSkipSuperseded)
    .then(all { assert(Converged and AspectsAligned and rows.get("p1").sidecar == files.get("p1").sidecar), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  run aspectsMoveVsSweepTest = init
    .then(ExternalDelete("p3"))
    .then(SubmitWatcherRemove("p3"))
    .then(WorkerRemoveApply)
    .then(ExternalDelete("p4"))
    .then(SubmitWatcherRemove("p4"))
    .then(WorkerRemoveApply)
    .then(ApiMove("p1", "p3"))
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  // @scope SCOPE-6
  run aspectsDirMoveCrashHealsTest = init
    .then(ExternalDelete("p3"))
    .then(SubmitWatcherRemove("p3"))
    .then(WorkerRemoveApply)
    .then(ExternalDelete("p4"))
    .then(SubmitWatcherRemove("p4"))
    .then(WorkerRemoveApply)
    .then(DirMoveFsOnly)
    .then(all { assert(not(AspectsAligned)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @operation OP-7
  // @scope SCOPE-5
  // @assumes ASS-6
  run aspectsSidecarPartialEditTest = init
    .then(SidecarEditPartial("p1"))
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @exempt environment
  // @assumes ASS-6
  // @assumes ASS-7
  run aspectsDroppedSidecarTest = init
    .then(DroppedSidecarModify("p1"))
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-4
  // @assumes ASS-6
  run aspectsThumbLossTest = init
    .then(LoseThumbs)
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(AspectAligned("p1")), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  run aspectsWatcherRemoveTest = init
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(not(rows.keys().contains("p1")) and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsSweepLeasedMoveTest = init
    .then(ExternalDelete("p3"))
    .then(SubmitWatcherRemove("p3"))
    .then(WorkerRemoveApply)
    .then(ExternalDelete("p4"))
    .then(SubmitWatcherRemove("p4"))
    .then(WorkerRemoveApply)
    .then(ApiMoveFsOnly("p1", "p3"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(all { assert(rows.keys().contains("p1") and not(files.get("p1").present)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  // @scope SCOPE-10
  run aspectsMoveInFlightThenRewriteSweepTest = init
    .then(ExternalDelete("p3"))
    .then(SubmitWatcherRemove("p3"))
    .then(WorkerRemoveApply)
    .then(ExternalDelete("p4"))
    .then(SubmitWatcherRemove("p4"))
    .then(WorkerRemoveApply)
    .then(ApiMoveFsOnly("p1", "p3"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(all { assert(rows.keys().contains("p1") and not(files.get("p1").present)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(MoveRowRewrite("p1", "p3"))
    .then(all { assert(not(rows.keys().contains("p1")) and rows.keys().contains("p3")), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-9
  run aspectsSupersededSweepTest = init
    .then(ExternalDelete("p1"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SubmitWatcherRemove("p1"))
    .then(SweepRecheck)
    .then(all { assert(rows.keys().contains("p1")), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-9
  run aspectsCleanSweepTest = init
    .then(ExternalDelete("p1"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(SweepWriterApply)
    .then(all { assert(not(rows.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-9
  // @scope SCOPE-10
  // @scope SCOPE-11
  run aspectsAbsorbedSweepTest = init
    .then(ExternalDelete("p3"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(MoveLeaseAcquire("p1", "p3"))
    .then(SweepWriterApply)
    .then(all { assert(not(rows.keys().contains("p3"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ApiMoveFsOnly("p1", "p3"))
    .then(MoveRowRewrite("p1", "p3"))
    .then(all { assert(not(rows.keys().contains("p1")) and rows.keys().contains("p3")), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsRotateVsScanTest = init
    .then(SubmitScan("p1"))
    .then(RotateWriteSidecar("p1"))
    .then(WorkerSkipSuperseded)
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @scope SCOPE-5
  // @assumes ASS-6
  run aspectsRotateTest = init
    .then(RotateWriteSidecar("p1"))
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  def UploadingWindow(p: str): bool =
    files.get(p).present and not(rows.keys().contains(p)) and intake.exists(i => i.path == p and i.kind == "watcher")

  // @assumes ASS-6
  def Corrupt(p: str): bool =
    files.get(p).present and rows.keys().contains(p) and not(AspectAligned(p)) and not(UploadingWindow(p))

  // @assumes ASS-6
  val UploadDoesNotMintRow = PATHS.forall(p => UploadingWindow(p) implies not(rows.keys().contains(p)))

  // @assumes ASS-6
  val UploadingIsNotCorrupt = PATHS.forall(p => UploadingWindow(p) implies not(Corrupt(p)))

  // @assumes ASS-6
  // @exempt observer
  // UploadingWindow is init-false and only UploadStoredOnly can create
  // that state in this module, so the witness is action-specific. The
  // apply/gone cases have no isolating state predicate (scan apply and
  // watcher submits reach the same states); their deterministic runs
  // (aspectsUploadWatcherRemoveBeforeIndexTest et al.) carry the
  // fired-action evidence instead.
  val WitnessUploadStoredOnly = PATHS.exists(p => UploadingWindow(p))

  // @operation OP-2
  // @scope SCOPE-5
  // @assumes ASS-6
  action UploadStoredOnly(p: str): bool = {
    nondet cv = HASHES.oneOf()
    nondet mt = SMALL.oneOf()
    nondet ln = SMALL.oneOf()
    nondet sc = SMALL.oneOf()
    all {
      p.in(PATHS),
      not(files.get(p).present),
      files' = files.set(p, { present: true, assetHash: cv, mtime: mt, len: ln, sidecar: sc }),
      generations' = generations.set(p, generations.get(p) + 1),
      intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: mt, len: ln, sidecar: sc, assetHash: cv })),
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @operation OP-1
  // @assumes ASS-4
  // @assumes ASS-5
  // @scope SCOPE-4
  action UploadIndexed(p: str): bool = {
    nondet it = oneOf(if (intake.exists(i => i.path == p)) intake.filter(i => i.path == p) else Set({ path: p, generation: 0, kind: "watcher", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      p.in(PATHS),
      intake.exists(i => i.path == p),
      it.path == p,
      it.generation == generations.get(p),
      files.get(p).present,
      snapshotMatches(files.get(p), it),
      rows' = rows.put(p, {
        assetId: if (rows.keys().contains(p)) { rows.get(p).assetId } else { nextAssetId },
        assetHash: it.assetHash,
        modified: it.mtime,
        fileSize: it.len,
        sidecar: it.sidecar,
        converged: true,
        thumb: true
      }),
      intake' = intake.filter(i => not (i.path == p and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      sweepPending' = sweepPending,
      nextAssetId' = if (rows.keys().contains(p)) { nextAssetId } else { nextAssetId + 1 },
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @operation OP-1
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-4
  action IntakePickupFileGone(p: str): bool = {
    nondet it = oneOf(if (intake.exists(i => i.path == p)) intake.filter(i => i.path == p) else Set({ path: p, generation: 0, kind: "watcher", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      p.in(PATHS),
      intake.exists(i => i.path == p),
      it.path == p,
      it.generation == generations.get(p),
      not(files.get(p).present),
      intake' = intake.filter(i => not (i.path == p and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsUploadVsSweepPruneTest = init
    .then(ExternalDelete("p1"))
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(all { assert(not(rows.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(UploadStoredOnly("p1"))
    .then(all { assert(UploadingWindow("p1") and not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(UploadIndexed("p1"))
    .then(all { assert(AspectsAligned and rows.keys().contains("p1")), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsUploadBeforeSweepTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(all { assert(rows.keys().contains("p1") and files.get("p1").present), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsUploadWatcherRemoveBeforeIndexTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerSkipSuperseded)
    .then(WorkerRemoveApply)
    .then(all { assert(not(rows.keys().contains("p1")) and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsUploadThenSidecarEditTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ExternalSidecarModify("p1"))
    .then(WorkerSkipSuperseded)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned and rows.get("p1").sidecar == files.get("p1").sidecar), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-6
  run aspectsUploadCrashHealsTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ProcessCrash)
    .then(all { assert(files.get("p1").present and intake.size() == 0), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-11
  val SweepWriterNeverPrunesPresentFile = sweepBatch.keys().forall(p => not(files.get(p).present) or not(sweepBatch.keys().contains(p)))

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-9
  val SweepRecheckBatchClean = sweepBatch.keys().forall(p => sweepBatch.get(p) == generations.get(p) and not(p.in(leasedPaths)) and not(files.get(p).present))

  // @assumes ASS-6
  // @scope SCOPE-11
  val SweepWriterPruneOnly = sweepBatch.keys().size() >= 0 and rows.keys().size() <= PATHS.size()
}
