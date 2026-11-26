// L1 sweep-chain slice: SweepEnumerate, SweepRecheck, SweepRemoveBatchApply
// on a pinned 3-path/2-root instance (p1, p2 absent; p3 present).
// Action bodies are copies of `pipeline/merged.qnt` (module `pipeline_merged`)
// modulo indentation; each carries a `// @mirrors-merged` source citation.
module local_sweep {
  import local_common.* from "./local-common"

  // Pinned init: two missing-file rows (p1 on r1, p2 on r2) and one present
  // row (p3 on r2). No faults, no leases, no queued work, generations at 0.
  action initSweepTwoRoot: bool = all {
    files' = Map(
      "p1" -> { present: false, assetHash: 0, mtime: 0, len: 1, sidecar: 0 },
      "p2" -> { present: false, assetHash: 0, mtime: 0, len: 1, sidecar: 0 },
      "p3" -> { present: true, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }
    ),
    generations' = Map("p1" -> 0, "p2" -> 0, "p3" -> 0),
    assetByPath' = Map("p1" -> 1, "p2" -> 2, "p3" -> 3),
    assetById' = Map(
      0 -> { assetId: 0, kind: "album", path: "", contentHash: -1, albumId: -1, isTrashed: false },
      1 -> { assetId: 1, kind: "media", path: "p1", contentHash: 0, albumId: 0, isTrashed: false },
      2 -> { assetId: 2, kind: "media", path: "p2", contentHash: 0, albumId: 0, isTrashed: false },
      3 -> { assetId: 3, kind: "media", path: "p3", contentHash: 0, albumId: 0, isTrashed: false }
    ),
    metadataPayload' = Map(
      0 -> { kind: "album", assetHash: 0, modified: 0, fileSize: 0, sidecar: 0, converged: true, itemCount: 3, itemSize: 2, hasShareList: false },
      1 -> { kind: "media", assetHash: 0, modified: 0, fileSize: 1, sidecar: 0, converged: true, itemCount: 0, itemSize: 0, hasShareList: false },
      2 -> { kind: "media", assetHash: 0, modified: 0, fileSize: 1, sidecar: 0, converged: true, itemCount: 0, itemSize: 0, hasShareList: false },
      3 -> { kind: "media", assetHash: 0, modified: 0, fileSize: 0, sidecar: 0, converged: true, itemCount: 0, itemSize: 0, hasShareList: false }
    ),
    // The pending flags represent the walk having happened: the sweep stage is
    // entered from the scan job, not from a standalone trigger.
    sweepPending' = Map("r1" -> true, "r2" -> true),
    sweepBatch' = Map(),
    intake' = Set(),
    leasedPaths' = Set(),
    unprocessable' = Set(),
    nextAssetId' = 4,
    dupeIndex' = Map(0 -> Set(1, 2, 3)),
    thumbPresent' = Map(0 -> true),
  }

  // @mirrors-merged merged.qnt:1724 SweepEnumerate
  action SweepEnumerate(root: str): bool = all {
    root.in(ROOTS),
    sweepPending.get(root),
    sweepBatch' = PATHS.filter(p => rootOf(p) == root and assetByPath.keys().contains(p) and not(files.get(p).present))
      .fold(
        sweepBatch.keys().filter(p => not(rootOf(p) == root)).mapBy(p => sweepBatch.get(p)),
        (acc, p) => acc.put(p, generations.get(p))
      ),
    sweepPending' = sweepPending.put(root, false),
    files' = files,
    generations' = generations,
    intake' = intake,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    leasedPaths' = leasedPaths,
  }

  // @mirrors-merged merged.qnt:1748 SweepRecheck
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    leasedPaths' = leasedPaths,
  }

  // @mirrors-merged merged.qnt:1771 SweepRemoveBatchApply
  action SweepRemoveBatchApply: bool = all {
    sweepBatch.keys().size() > 0,
    // Album-liveness re-check (SCOPE-12): apply_album_delta no-ops without the album row.
    metadataPayload.keys().contains(0),
    assetByPath' = assetByPath.keys().filter(p =>
      not (sweepBatch.keys().contains(p) and
           assetByPath.keys().contains(p) and
           not(files.get(p).present))
    ).mapBy(k => assetByPath.get(k)),
    assetById' = assetById.keys().filter(id =>
      not (sweepBatch.keys().exists(p =>
        assetByPath.keys().contains(p) and
        assetByPath.get(p) == id and
        not(files.get(p).present)))
    ).mapBy(k => assetById.get(k)),
    metadataPayload' = metadataPayload.keys().filter(id =>
      not (sweepBatch.keys().exists(p =>
        assetByPath.keys().contains(p) and
        assetByPath.get(p) == id and
        not(files.get(p).present)))
    ).mapBy(k => metadataPayload.get(k))
      .put(0, { kind: metadataPayload.get(0).kind, assetHash: metadataPayload.get(0).assetHash, modified: metadataPayload.get(0).modified, fileSize: metadataPayload.get(0).fileSize, sidecar: metadataPayload.get(0).sidecar, converged: metadataPayload.get(0).converged,
        itemCount: metadataPayload.get(0).itemCount - sweepBatch.keys().filter(p => assetByPath.keys().contains(p) and not(files.get(p).present) and assetById.keys().contains(assetByPath.get(p)) and assetById.get(assetByPath.get(p)).albumId == 0 and not(assetById.get(assetByPath.get(p)).isTrashed)).size(),
        itemSize: metadataPayload.get(0).itemSize - sweepBatch.keys().filter(p => assetByPath.keys().contains(p) and not(files.get(p).present) and assetById.keys().contains(assetByPath.get(p)) and assetById.get(assetByPath.get(p)).albumId == 0 and not(assetById.get(assetByPath.get(p)).isTrashed)).fold(0, (acc, p) => acc + (if (metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).fileSize } else { 0 })),
        hasShareList: metadataPayload.get(0).hasShareList }),
    dupeIndex' = dupeIndex,
    thumbPresent' = thumbPresent,
    sweepBatch' = Map(),
    sweepPending' = sweepPending,
    files' = files,
    generations' = generations,
    intake' = intake,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    leasedPaths' = leasedPaths,
  }

  // @mirrors-merged merged.qnt:2813 stepCompletion (sweep-action slice only)
  action stepSweep: bool = any {
    SweepEnumerate("r1"),
    SweepEnumerate("r2"),
    SweepRecheck,
    SweepRemoveBatchApply
  }

  val SweepQuiescent = sweepBatch.keys().size() == 0 and ROOTS.forall(r => not(sweepPending.get(r)))
  val PresentRowsKept = PATHS.forall(p => files.get(p).present implies assetByPath.keys().contains(p))
  val NoAbsentRowsAtQuiescence = PATHS.forall(p => (not(files.get(p).present)) implies not(assetByPath.keys().contains(p)))
  val SweepContracts = (SweepQuiescent implies NoPermanentStale) and PresentRowsKept and (SweepQuiescent implies NoAbsentRowsAtQuiescence)
  val SweepQuiescentReached = not(SweepQuiescent)
}
