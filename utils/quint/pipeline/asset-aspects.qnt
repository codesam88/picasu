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
  }

  // @operation OP-4
  // @scope SCOPE-5
  action ApiMoveFsOnly(p: str, q: str): bool = all {
    p.in(PATHS),
    q.in(PATHS),
    not(p == q),
    files.get(p).present,
    not(files.get(q).present),
    rows.keys().contains(p),
    files' = files.set(p, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }).set(q, { present: true, assetHash: files.get(p).assetHash, mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar }),
    generations' = generations,
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @operation OP-9
  // @scope SCOPE-5
  action SweepRootLeased(p: str): bool = all {
    p.in(PATHS),
    sweepPending.get(rootOf(p)),
    rows.keys().contains(p),
    not(files.get(p).present),
    rows' = rows,
    sweepPending' = sweepPending.put(rootOf(p), false),
    files' = files,
    generations' = generations,
    intake' = intake,
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
    generations' = generations,
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
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
  }

  // @assumes ASS-6
  run aspectsScanApplyTest = init
    .then(ForceModify("p1"))
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsScanSupersedesEditTest = init
    .then(SubmitScan("p1"))
    .then(AppSidecarEdit("p1"))
    .then(WorkerSkipSuperseded)
    .then(all { assert(Converged and AspectsAligned and rows.get("p1").sidecar == files.get("p1").sidecar), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

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
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

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
    .then(all { assert(not(AspectsAligned)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @operation OP-7
  // @scope SCOPE-5
  // @assumes ASS-6
  run aspectsSidecarPartialEditTest = init
    .then(SidecarEditPartial("p1"))
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @exempt environment
  // @assumes ASS-6
  // @assumes ASS-7
  run aspectsDroppedSidecarTest = init
    .then(DroppedSidecarModify("p1"))
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes ASS-4
  // @assumes ASS-6
  run aspectsThumbLossTest = init
    .then(LoseThumbs)
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(AspectAligned("p1")), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes ASS-3
  // @assumes ASS-6
  run aspectsWatcherRemoveTest = init
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(not(rows.keys().contains("p1")) and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

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
    .then(SweepRootLeased("p1"))
    .then(all { assert(rows.keys().contains("p1") and not(files.get("p1").present)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsRotateVsScanTest = init
    .then(SubmitScan("p1"))
    .then(RotateWriteSidecar("p1"))
    .then(WorkerSkipSuperseded)
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @scope SCOPE-5
  // @assumes ASS-6
  run aspectsRotateTest = init
    .then(RotateWriteSidecar("p1"))
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
}
