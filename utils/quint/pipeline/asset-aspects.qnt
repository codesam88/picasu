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
      assetByPath.keys().contains(p) and
      assetById.keys().contains(assetByPath.get(p)) and
      assetById.get(assetByPath.get(p)).path == p and
      assetById.get(assetByPath.get(p)).contentHash == files.get(p).assetHash and
      metadataPayload.keys().contains(assetByPath.get(p)) and
      metadataPayload.get(assetByPath.get(p)).assetHash == files.get(p).assetHash and
      metadataPayload.get(assetByPath.get(p)).modified == files.get(p).mtime and
      metadataPayload.get(assetByPath.get(p)).fileSize == files.get(p).len and
      metadataPayload.get(assetByPath.get(p)).sidecar == files.get(p).sidecar and
      metadataPayload.get(assetByPath.get(p)).converged and
      (assetById.get(assetByPath.get(p)).contentHash < 0 or
        (thumbPresent.keys().contains(assetById.get(assetByPath.get(p)).contentHash) and
         thumbPresent.get(assetById.get(assetByPath.get(p)).contentHash))) and
      (assetById.get(assetByPath.get(p)).contentHash < 0 or
        dupeIndex.keys().contains(assetById.get(assetByPath.get(p)).contentHash) and
        dupeIndex.get(assetById.get(assetByPath.get(p)).contentHash).contains(assetByPath.get(p)))
    } else {
      not(assetByPath.keys().contains(p))
    }
  }

  // @assumes ASS-3
  // @assumes ASS-6
  val AspectsAligned = PATHS.forall(p => AspectAligned(p))

  // @assumes ASS-4
  def ThumbPresent(p: str): bool =
    if (assetByPath.keys().contains(p) and assetById.keys().contains(assetByPath.get(p))) {
      // @exempt observer
      val h = assetById.get(assetByPath.get(p)).contentHash
      h >= 0 and thumbPresent.keys().contains(h) and thumbPresent.get(h)
    } else { false }

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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending,
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths.union(Set(p, q)),
  }

  // @operation OP-9
  // @scope SCOPE-8
  // SweepEnumerate maps to the enumeration loop of tasks/actor/album_index.rs::sweep_stale_asset_paths (candidates from ASSET_BY_PATH rows with missing files, per-candidate generation snapshot).
  action SweepEnumerate(root: str): bool = all {
    root.in(ROOTS),
    sweepPending.get(root),
    sweepBatch' = PATHS.filter(p => rootOf(p) == root and assetByPath.keys().contains(p) and not(files.get(p).present)).mapBy(p => generations.get(p)),
    sweepPending' = sweepPending.put(root, false),
    files' = files,
    generations' = generations,
    intake' = intake,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    leasedPaths' = leasedPaths,
  }

  // @operation OP-9
  // @scope SCOPE-9
  // SweepRecheck maps to the inline re-check loop of sweep_stale_asset_paths (generation unchanged + path_epoch::is_busy + path.exists, pre-submit).
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    leasedPaths' = leasedPaths,
  }

  // @operation OP-9
  // @scope SCOPE-11
  // SweepRemoveBatchApply maps to flush_tree.rs::sweep_remove_batch, submitted as WriteOp::SweepRemoveBatch via tasks/queue/writer.rs::submit; checks path.exists skip + asset-id/record resolution only, no generation/lease check.
  action SweepRemoveBatchApply: bool = all {
    sweepBatch.keys().size() > 0,
    assetByPath' = assetByPath.keys().filter(p =>
      not (sweepBatch.keys().contains(p) and
           assetByPath.keys().contains(p) and
           not(files.get(p).present))
    ).mapBy(k => assetByPath.get(k)),
    assetById' = assetById,
    metadataPayload' = metadataPayload,
    dupeIndex' = dupeIndex,
    thumbPresent' = thumbPresent,
    albumPending' = albumPending,
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
  // MoveLeaseAcquire maps to assign_album.rs move window opening lease_paths(source, dest).
  action MoveLeaseAcquire(p: str, q: str): bool = all {
    p.in(PATHS),
    q.in(PATHS),
    not(p == q),
    files.get(p).present,
    not(files.get(q).present),
    rows.keys().contains(p),
    files' = files,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending,
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
  // MoveRowRewrite maps to assign_album.rs moveAssetIntoAlbum row rewrite (put_asset_by_id + remove_asset_by_path + put_asset_by_path) and lease release.
  action MoveRowRewrite(p: str, q: str): bool = all {
    p.in(PATHS),
    q.in(PATHS),
    not(p == q),
    rows.keys().contains(p),
    leasedPaths' = leasedPaths.exclude(Set(p, q)),
    assetById' = assetById.put(assetByPath.get(p), { assetId: assetByPath.get(p), kind: assetById.get(assetByPath.get(p)).kind, path: q, contentHash: assetById.get(assetByPath.get(p)).contentHash, albumId: assetById.get(assetByPath.get(p)).albumId, isTrashed: assetById.get(assetByPath.get(p)).isTrashed }),
    assetByPath' = assetByPath.keys().exclude(Set(p)).mapBy(k => assetByPath.get(k)).put(q, assetByPath.get(p)),
    metadataPayload' = metadataPayload,
    dupeIndex' = dupeIndex,
    thumbPresent' = thumbPresent,
    albumPending' = albumPending,
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending,
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending,
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
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsScanSupersedesEditTest = init
    .then(SubmitScan("p1"))
    .then(AppSidecarEdit("p1"))
    .then(WorkerSkipSuperseded)
    .then(all { assert(Converged and AspectsAligned and rows.get("p1").sidecar == files.get("p1").sidecar), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(not(AspectsAligned)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @operation OP-7
  // @scope SCOPE-5
  // @assumes ASS-6
  run aspectsSidecarPartialEditTest = init
    .then(SidecarEditPartial("p1"))
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @exempt environment
  // @assumes ASS-6
  // @assumes ASS-7
  run aspectsDroppedSidecarTest = init
    .then(DroppedSidecarModify("p1"))
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-4
  // @assumes ASS-6
  run aspectsThumbLossTest = init
    .then(LoseThumbs)
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(AspectAligned("p1")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  run aspectsWatcherRemoveTest = init
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(not(rows.keys().contains("p1")) and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(rows.keys().contains("p1") and not(files.get("p1").present)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(rows.keys().contains("p1") and not(files.get("p1").present)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(MoveRowRewrite("p1", "p3"))
    .then(all { assert(not(rows.keys().contains("p1")) and rows.keys().contains("p3")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-9
  run aspectsSupersededSweepTest = init
    .then(ExternalDelete("p1"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SubmitWatcherRemove("p1"))
    .then(SweepRecheck)
    .then(all { assert(rows.keys().contains("p1")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-9
  run aspectsCleanSweepTest = init
    .then(ExternalDelete("p1"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(SweepRemoveBatchApply)
    .then(all { assert(not(rows.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(SweepRemoveBatchApply)
    .then(all { assert(not(rows.keys().contains("p3"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ApiMoveFsOnly("p1", "p3"))
    .then(MoveRowRewrite("p1", "p3"))
    .then(all { assert(not(rows.keys().contains("p1")) and rows.keys().contains("p3")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsRotateVsScanTest = init
    .then(SubmitScan("p1"))
    .then(RotateWriteSidecar("p1"))
    .then(WorkerSkipSuperseded)
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @scope SCOPE-5
  // @assumes ASS-6
  run aspectsRotateTest = init
    .then(RotateWriteSidecar("p1"))
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  def UploadingWindow(p: str): bool =
    files.get(p).present and not(assetByPath.keys().contains(p)) and intake.exists(i => i.path == p and i.kind == "watcher")

  // @assumes ASS-6
  def Corrupt(p: str): bool =
    files.get(p).present and assetByPath.keys().contains(p) and not(AspectAligned(p)) and not(UploadingWindow(p))

  // @assumes ASS-6
  val UploadDoesNotMintRow = PATHS.forall(p => UploadingWindow(p) implies not(assetByPath.keys().contains(p)))

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
  // UploadStoredOnly maps to post_upload.rs::upload store + submit_uploaded_path -> submit_watcher_index.
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
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending,
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
  // UploadIndexed maps to tasks/queue/worker.rs intake application running workflow::process_file (index path).
  action UploadIndexed(p: str): bool = {
    nondet it = oneOf(if (intake.exists(i => i.path == p)) intake.filter(i => i.path == p) else Set({ path: p, generation: 0, kind: "watcher", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      p.in(PATHS),
      intake.exists(i => i.path == p),
      it.path == p,
      it.generation == generations.get(p),
      files.get(p).present,
      snapshotMatches(files.get(p), it),
      assetById' = assetById.put(if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }, { assetId: if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }, kind: if (assetByPath.keys().contains(p)) { assetById.get(assetByPath.get(p)).kind } else { "media" }, path: p, contentHash: it.assetHash, albumId: if (assetByPath.keys().contains(p)) { assetById.get(assetByPath.get(p)).albumId } else { 0 }, isTrashed: if (assetByPath.keys().contains(p)) { assetById.get(assetByPath.get(p)).isTrashed } else { false } }),
      assetByPath' = assetByPath.put(p, if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }),
      metadataPayload' = metadataPayload.put(if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }, { kind: if (assetByPath.keys().contains(p)) { metadataPayload.get(assetByPath.get(p)).kind } else { "media" }, assetHash: it.assetHash, modified: it.mtime, fileSize: it.len, sidecar: it.sidecar, converged: true, itemCount: 0, itemSize: 0, hasShareList: false }),
      dupeIndex' = dupeIndex.put(it.assetHash, (if (dupeIndex.keys().contains(it.assetHash)) { dupeIndex.get(it.assetHash) } else { Set() }).union(Set(if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }))),
      thumbPresent' = thumbPresent.put(it.assetHash, true),
      albumPending' = albumPending,
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
  // IntakePickupFileGone maps to tasks/queue/worker.rs intake application running workflow::process_file (gone-file outcome per worker drop).
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
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending,
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
    .then(all { assert(not(rows.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(UploadStoredOnly("p1"))
    .then(all { assert(UploadingWindow("p1") and not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(UploadIndexed("p1"))
    .then(all { assert(AspectsAligned and rows.keys().contains("p1")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsUploadBeforeSweepTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(all { assert(rows.keys().contains("p1") and files.get("p1").present), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(not(rows.keys().contains("p1")) and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsUploadThenSidecarEditTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ExternalSidecarModify("p1"))
    .then(WorkerSkipSuperseded)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned and rows.get("p1").sidecar == files.get("p1").sidecar), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-6
  run aspectsUploadCrashHealsTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ProcessCrash)
    .then(all { assert(files.get("p1").present and intake.size() == 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-11
  // SweepRemoveBatchSkipsPresentFile maps to the `if path.exists() { continue }` skip in flush_tree.rs::sweep_remove_batch.
  val SweepRemoveBatchSkipsPresentFile = sweepBatch.keys().forall(p => not(files.get(p).present) or not(sweepBatch.keys().contains(p)))

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-9
  // SweepRecheckBatchClean maps to the sweep_stale_asset_paths re-check guarantee (generation unchanged + not leased + file absent).
  val SweepRecheckBatchClean = sweepBatch.keys().forall(p => sweepBatch.get(p) == generations.get(p) and not(p.in(leasedPaths)) and not(files.get(p).present))

  // @assumes ASS-6
  // @scope SCOPE-11
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-11
  // Path-map/identity coherence: no asset whose path is present on disk
  // may have its identity stored in assetById without a corresponding
  // assetByPath entry.
  val PathMapIdentityCoherence = assetById.keys().forall(id =>
    assetById.get(id).path.in(PATHS) implies (
      files.get(assetById.get(id).path).present implies assetByPath.keys().contains(assetById.get(id).path)
    )
  )

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-11
  // Dupe-index bijective consistency: dupeIndex membership exactly
  // matches assetById contentHash grouping.
  val DupeIndexBijective = assetById.keys().forall(id =>
    assetById.get(id).contentHash >= 0 implies
      (dupeIndex.keys().contains(assetById.get(id).contentHash) and
       dupeIndex.get(assetById.get(id).contentHash).contains(id))
  ) and dupeIndex.keys().forall(h =>
    dupeIndex.get(h).forall(id =>
      assetById.keys().contains(id) and assetById.get(id).contentHash == h
    )
  )

  // @operation OP-3
  // @scope SCOPE-5
  // @assumes ASS-6
  // RemoveIdOnly models a remove that commits ASSET_BY_ID (and metadata)
  // but crashes before ASSET_BY_PATH is rewritten (N5 torn window).
  action RemoveIdOnly(p: str): bool = all {
    p.in(PATHS),
    assetByPath.keys().contains(p),
    assetById' = assetById.keys().exclude(Set(assetByPath.get(p))).mapBy(k => assetById.get(k)),
    assetByPath' = assetByPath,
    metadataPayload' = metadataPayload.keys().exclude(Set(assetByPath.get(p))).mapBy(k => metadataPayload.get(k)),
    dupeIndex' = dupeIndex,
    thumbPresent' = thumbPresent,
    albumPending' = albumPending,
    files' = files,
    generations' = generations,
    intake' = intake,
    sweepPending' = sweepPending,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt environment
  // @assumes ASS-6
  action MemberWrite(p: str): bool = all {
    p.in(PATHS),
    assetByPath.keys().contains(p),
    albumPending' = albumPending.union(Set(assetByPath.get(p))),
    assetById' = assetById,
    assetByPath' = assetByPath,
    dupeIndex' = dupeIndex,
    metadataPayload' = metadataPayload,
    thumbPresent' = thumbPresent,
    files' = files,
    generations' = generations,
    intake' = intake,
    sweepPending' = sweepPending,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt environment
  // @assumes ASS-6
  action AlbumTaskApply: bool = all {
    albumPending.size() > 0,
    albumPending' = Set(),
    assetById' = assetById,
    assetByPath' = assetByPath,
    dupeIndex' = dupeIndex,
    metadataPayload' = metadataPayload,
    thumbPresent' = thumbPresent,
    files' = files,
    generations' = generations,
    intake' = intake,
    sweepPending' = sweepPending,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsN5SplitWindowTest = init
    .then(ExternalDelete("p1"))
    .then(RemoveIdOnly("p1"))
    .then(all { assert(assetByPath.keys().contains("p1") and not(assetById.keys().contains(assetByPath.get("p1")))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(SweepRemoveBatchApply)
    .then(all { assert(not(assetByPath.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsDupeGroupDeleteTest = init
    .then(ExternalDelete("p2"))
    .then(SubmitWatcherRemove("p2"))
    .then(WorkerRemoveApply)
    .then(ExternalDelete("p3"))
    .then(SubmitWatcherRemove("p3"))
    .then(WorkerRemoveApply)
    .then(ExternalDelete("p4"))
    .then(SubmitWatcherRemove("p4"))
    .then(WorkerRemoveApply)
    .then(ExternalDelete("p5"))
    .then(SubmitWatcherRemove("p5"))
    .then(WorkerRemoveApply)
    .then(all { assert(assetByPath.keys().contains("p1") and thumbPresent.get(0)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(not(assetByPath.keys().contains("p1")) and (not(thumbPresent.keys().contains(0)) or not(thumbPresent.get(0)))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsRemoveTornWindowTest = init
    .then(ExternalDelete("p1"))
    .then(RemoveIdOnly("p1"))
    .then(all { assert(assetByPath.keys().contains("p1") and not(assetById.keys().contains(assetByPath.get("p1")))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(SweepRemoveBatchApply)
    .then(all { assert(not(assetByPath.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsMemberWriteAlbumCascadeTest = init
    .then(MemberWrite("p1"))
    .then(all { assert(albumPending.size() > 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(AlbumTaskApply)
    .then(all { assert(albumPending.size() == 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, albumPending' = albumPending, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

}
