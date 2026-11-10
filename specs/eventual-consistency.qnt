// Eventual consistency model for the index pipeline.
//
// Event (events.json) -> handling action:
//   FS_CREATE / FS_MODIFY / FS_ATOMIC_REPLACE -> ExternalModify
//   FS_DELETE -> ExternalDelete
//   FS_RENAME_FILE -> ExternalRenameFile
//   FS_RENAME_DIR -> ExternalRenameDir / ApiAssignDirMove
//   FS_SIDECAR_CREATE / FS_SIDECAR_MODIFY -> ExternalSidecarCreate / ExternalSidecarModify
//   FS_SIDECAR_DELETE -> ExternalSidecarDelete
//   FS_MODIFY_INFLIGHT -> ForceModify
//   API_UPLOAD -> ApiUpload
//   API_DELETE -> ApiRemove
//   API_ASSIGN_ALBUM_DIR_MOVE -> ApiMove / ApiAssignDirMove
//   API_EDIT_METADATA -> AppSidecarEdit
//   API_ROTATE_IMAGE -> RotateWriteSidecar / RotateFinish
//   WATCHER_GAP / PROCESS_CRASH / DERIVED_STORE_LOSS / DB_LOSS -> ProcessCrash / LoseThumbs / DbLoss
//   (scan/sweep/submit/worker actions are pipeline machinery, not events)
//
// Out of scope for this model: watcher delivery, general liveness, partial
// durable writes, and hierarchical directory paths (flat pairs only, SCOPE-7).
// Fairness is delegated to the two scan triggers: Flag::Rescan while the
// watcher is enabled and the user-enabled periodic backstop (QUINT-ASS-7).
// The model checks bounded convergence under those triggers, not that they run.
//
// Every action/val/def/run carries @assumes/@scope/@exempt, or an
// @requires tag resolving against specs/requirements.json.
module eventual_consistency {
  import eventual_consistency_state.* from "./eventual-consistency-state"
  import eventual_consistency_assumptions.* from "./eventual-consistency-assumptions"
  import eventual_consistency_goal.* from "./eventual-consistency-goal"

  // @scope SCOPE-7
  pure val SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }, { src: "p2", dst: "p4" }]

  // ---------- Environment injections ----------

  // @exempt environment
  action ExternalModify(p: str): bool = {
    nondet cv = HASHES.oneOf()
    nondet mt = SMALL.oneOf()
    nondet ln = SMALL.oneOf()
    nondet sc = SMALL.oneOf()
    all {
      p.in(PATHS),
      files' = files.set(p, { present: true, assetHash: cv, mtime: mt, len: ln, sidecar: sc }),
      generations' = generations,
      intake' = intake,
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }
  }

  // @exempt environment
  action ExternalDelete(p: str): bool = all {
    p.in(PATHS),
    files' = files.set(p, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }),
    generations' = generations,
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // Prescribed handling: old-path remove plus new-path create, submitted in
  // the same watcher reaction.
  // @exempt environment
  // @assumes QUINT-ASS-6
  action ExternalRenameFile(p: str, q: str): bool = all {
      p.in(PATHS),
      q.in(PATHS),
      not(p == q),
      files.get(p).present,
      not(files.get(q).present),
      files' = files.set(p, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 })
                  .set(q, { present: true, assetHash: files.get(p).assetHash, mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar }),
      generations' = generations.set(p, generations.get(p) + 1).set(q, generations.get(q) + 1),
      intake' = intake.union(Set(
        { path: p, generation: generations.get(p) + 1, kind: "remove", mtime: 0, len: 0, sidecar: 0, assetHash: 0 },
        { path: q, generation: generations.get(q) + 1, kind: "watcher", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar, assetHash: files.get(p).assetHash }
      )),
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }

  // @assumes QUINT-ASS-1
  // @scope SCOPE-5
  // @scope SCOPE-7
  action ExternalRenameDir: bool = all {
    SUBTREE_PAIRS.foldl(true, (acc, pr) => acc and files.get(pr.src).present and not(files.get(pr.dst).present)),
    files' = SUBTREE_PAIRS.foldl(files, (acc, pr) =>
      acc.set(pr.src, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 })
         .set(pr.dst, { present: true, assetHash: files.get(pr.src).assetHash, mtime: files.get(pr.src).mtime, len: files.get(pr.src).len, sidecar: files.get(pr.src).sidecar })
    ),
    generations' = generations,
    intake' = intake,
    rows' = SUBTREE_PAIRS.foldl(rows, (acc, pr) =>
      if (rows.keys().contains(pr.src)) {
        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k)).put(pr.dst, rows.get(pr.src))
      } else {
        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k))
      }
    ),
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt environment
  // @assumes QUINT-ASS-6
  action ExternalSidecarCreate(p: str): bool = {
    nondet sc = Set(1, 2).oneOf()
    all {
      p.in(PATHS),
      files.get(p).present,
      files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: sc }),
      generations' = generations.set(p, generations.get(p) + 1),
      intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: sc, assetHash: files.get(p).assetHash })),
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }
  }

  // @exempt environment
  // @assumes QUINT-ASS-6
  action ExternalSidecarModify(p: str): bool = {
    nondet sc = Set(1, 2).oneOf()
    all {
      p.in(PATHS),
      files.get(p).present,
      files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: sc }),
      generations' = generations.set(p, generations.get(p) + 1),
      intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: sc, assetHash: files.get(p).assetHash })),
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }
  }

  // @exempt environment
  // @assumes QUINT-ASS-3
  // @assumes QUINT-ASS-6
  action ExternalSidecarDelete(p: str): bool = all {
      p.in(PATHS),
      files.get(p).present,
      files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: 0 }),
      generations' = generations.set(p, generations.get(p) + 1),
      intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: 0, assetHash: files.get(p).assetHash })),
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }

  // @exempt environment
  action ForceModify(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    files' = files.set(p, { present: true, assetHash: files.get(p).assetHash + 1, mtime: files.get(p).mtime + 1, len: files.get(p).len + 1, sidecar: files.get(p).sidecar }),
    generations' = generations,
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt environment
  action MarkUnprocessable(p: str): bool = all {
    p.in(PATHS),
    unprocessable' = unprocessable.union(Set(p)),
    files' = files,
    generations' = generations,
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
  }

  // @assumes QUINT-ASS-5
  action RestoreIndexed(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    files' = files.set(p, { present: true, assetHash: rows.get(p).assetHash, mtime: rows.get(p).modified, len: rows.get(p).fileSize, sidecar: rows.get(p).sidecar }),
    generations' = generations,
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt environment
  // @scope SCOPE-6
  action ProcessCrash: bool = all {
    intake' = Set(),
    files' = files,
    generations' = generations,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt environment
  // @scope SCOPE-6
  action LoseThumbs: bool = all {
    rows' = rows.keys().mapBy(k => { ...rows.get(k), thumb: false }),
    files' = files,
    generations' = generations,
    intake' = intake,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt environment
  // @scope SCOPE-6
  action DbLoss: bool = all {
    rows' = Map(),
    generations' = PATHS.mapBy(_ => 0),
    nextAssetId' = 1,
    intake' = Set(),
    sweepPending' = ROOTS.mapBy(_ => false),
    files' = files,
    unprocessable' = unprocessable,
  }

  // ---------- API / direct writes ----------

  // @scope SCOPE-5
  action AppSidecarEdit(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: 1 }),
    generations' = generations.set(p, generations.get(p) + 1),
    rows' = rows.put(p, { assetId: rows.get(p).assetId, assetHash: files.get(p).assetHash, modified: files.get(p).mtime, fileSize: files.get(p).len, sidecar: 1, converged: true, thumb: true }),
    intake' = intake,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @scope SCOPE-5
  action RotateWriteSidecar(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: if (files.get(p).sidecar == 1) 2 else 1 }),
    generations' = generations.set(p, generations.get(p) + 1),
    rows' = rows,
    intake' = intake,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @scope SCOPE-5
  action RotateFinish(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    files.get(p).sidecar != rows.get(p).sidecar,
    rows' = rows.put(p, { assetId: rows.get(p).assetId, assetHash: files.get(p).assetHash, modified: files.get(p).mtime, fileSize: files.get(p).len, sidecar: files.get(p).sidecar, converged: true, thumb: true }),
    files' = files,
    generations' = generations,
    intake' = intake,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @scope SCOPE-5
  action ApiUpload(p: str): bool = {
    nondet cv = HASHES.oneOf()
    nondet mt = SMALL.oneOf()
    nondet ln = SMALL.oneOf()
    nondet sc = SMALL.oneOf()
    all {
      p.in(PATHS),
      files' = files.set(p, { present: true, assetHash: cv, mtime: mt, len: ln, sidecar: sc }),
      generations' = generations.set(p, generations.get(p) + 1),
      rows' = rows.put(p, { assetId: nextAssetId, assetHash: cv, modified: mt, fileSize: ln, sidecar: sc, converged: true, thumb: true }),
      intake' = intake,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId + 1,
      unprocessable' = unprocessable,
    }
  }

  // @scope SCOPE-5
  action ApiRemove(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    files' = files.set(p, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }),
    generations' = generations,
    intake' = intake,
    rows' = rows.keys().exclude(Set(p)).mapBy(k => rows.get(k)),
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @assumes QUINT-ASS-1
  // @scope SCOPE-5
  action ApiMove(p: str, q: str): bool = all {
    p.in(PATHS),
    q.in(PATHS),
    not(p == q),
    files.get(p).present,
    not(files.get(q).present),
    rows.keys().contains(p),
    files' = files.set(p, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }).set(q, { present: true, assetHash: files.get(p).assetHash, mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar }),
    generations' = generations,
    intake' = intake,
    rows' = rows.keys().exclude(Set(p)).mapBy(k => rows.get(k)).put(q, rows.get(p)),
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @assumes QUINT-ASS-1
  // @scope SCOPE-5
  // @scope SCOPE-7
  action ApiAssignDirMove: bool = all {
    SUBTREE_PAIRS.foldl(true, (acc, pr) => acc and files.get(pr.src).present and not(files.get(pr.dst).present)),
    files' = SUBTREE_PAIRS.foldl(files, (acc, pr) =>
      acc.set(pr.src, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 })
         .set(pr.dst, { present: true, assetHash: files.get(pr.src).assetHash, mtime: files.get(pr.src).mtime, len: files.get(pr.src).len, sidecar: files.get(pr.src).sidecar })
    ),
    generations' = generations,
    intake' = intake,
    rows' = SUBTREE_PAIRS.foldl(rows, (acc, pr) =>
      if (rows.keys().contains(pr.src)) {
        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k)).put(pr.dst, rows.get(pr.src))
      } else {
        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k))
      }
    ),
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // ---------- Intake submits ----------

  // @assumes QUINT-ASS-6
  action SubmitScan(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    generations' = generations.set(p, generations.get(p) + 1),
    intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "scan", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar, assetHash: files.get(p).assetHash })),
    files' = files,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @assumes QUINT-ASS-6
  action SubmitWatcherIndex(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    generations' = generations.set(p, generations.get(p) + 1),
    intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar, assetHash: files.get(p).assetHash })),
    files' = files,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @assumes QUINT-ASS-6
  action SubmitWatcherRemove(p: str): bool = all {
    p.in(PATHS),
    not(files.get(p).present),
    generations' = generations.set(p, generations.get(p) + 1),
    intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "remove", mtime: 0, len: 0, sidecar: 0, assetHash: 0 })),
    files' = files,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // ---------- Worker steps ----------

  // @assumes QUINT-ASS-4
  // @scope SCOPE-4
  action WorkerApplySuccess: bool = {
    nondet it = oneOf(if (intake.size() > 0) intake else Set({ path: "p1", generation: 0, kind: "scan", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      intake.size() > 0,
      it.generation == generations.get(it.path),
      files.get(it.path).present,
      snapshotMatches(files.get(it.path), it),
      rows' = rows.put(it.path, {
        assetId: if (rows.keys().contains(it.path)) { rows.get(it.path).assetId } else { nextAssetId },
        assetHash: it.assetHash,
        modified: it.mtime,
        fileSize: it.len,
        sidecar: it.sidecar,
        converged: true,
        thumb: true
      }),
      intake' = intake.filter(i => not (i.path == it.path and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      sweepPending' = sweepPending,
      nextAssetId' = if (rows.keys().contains(it.path)) { nextAssetId } else { nextAssetId + 1 },
      unprocessable' = unprocessable,
    }
  }

  // @exempt no-backing
  action WorkerSkipSuperseded: bool = {
    nondet it = oneOf(if (intake.size() > 0) intake else Set({ path: "p1", generation: 0, kind: "scan", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      intake.size() > 0,
      it.generation < generations.get(it.path),
      intake' = intake.filter(i => not (i.path == it.path and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }
  }

  // @assumes QUINT-ASS-5
  action WorkerSkipStale: bool = {
    nondet it = oneOf(if (intake.size() > 0) intake else Set({ path: "p1", generation: 0, kind: "scan", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      intake.size() > 0,
      it.generation == generations.get(it.path),
      files.get(it.path).present,
      not(snapshotMatches(files.get(it.path), it)),
      intake' = intake.filter(i => not (i.path == it.path and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }
  }

  // @assumes QUINT-ASS-4
  // @assumes QUINT-ASS-5
  action WorkerSkipUnchanged: bool = {
    nondet it = oneOf(if (intake.size() > 0) intake else Set({ path: "p1", generation: 0, kind: "scan", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      intake.size() > 0,
      it.generation == generations.get(it.path),
      files.get(it.path).present,
      snapshotMatches(files.get(it.path), it),
      rows.keys().contains(it.path),
      rows.get(it.path).assetHash == it.assetHash,
      rows.get(it.path).modified == it.mtime,
      rows.get(it.path).fileSize == it.len,
      rows.get(it.path).sidecar == it.sidecar,
      rows.get(it.path).converged,
      rows.get(it.path).thumb,
      intake' = intake.filter(i => not (i.path == it.path and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }
  }

  // @exempt no-backing
  action WorkerOpenFails: bool = {
    nondet it = oneOf(if (intake.size() > 0) intake else Set({ path: "p1", generation: 0, kind: "scan", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      intake.size() > 0,
      it.kind != "remove",
      it.generation == generations.get(it.path),
      (not(files.get(it.path).present) or it.path.in(unprocessable)),
      intake' = intake.filter(i => not (i.path == it.path and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      rows' = rows,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }
  }

  // @assumes QUINT-ASS-3
  action WorkerRemoveApply: bool = {
    nondet it = oneOf(if (intake.size() > 0) intake else Set({ path: "p1", generation: 0, kind: "remove", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      intake.size() > 0,
      it.kind == "remove",
      it.generation == generations.get(it.path),
      not(files.get(it.path).present),
      rows' = if (rows.keys().contains(it.path)) { rows.keys().exclude(Set(it.path)).mapBy(k => rows.get(k)) } else { rows },
      intake' = intake.filter(i => not (i.path == it.path and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    }
  }

  // @assumes QUINT-ASS-3
  // @assumes QUINT-ASS-6
  // @assumes QUINT-ASS-7
  action ScanRoot(r: str): bool = all {
    r.in(ROOTS),
    generations' = PATHS.mapBy(p =>
      if (rootOf(p) == r and files.get(p).present) { generations.get(p) + 1 } else { generations.get(p) }
    ),
    intake' = intake.union(
      PATHS.filter(p => rootOf(p) == r and files.get(p).present)
           .map(p => { path: p, generation: generations.get(p) + 1, kind: "scan", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar, assetHash: files.get(p).assetHash })
    ),
    sweepPending' = sweepPending.put(r, true),
    files' = files,
    rows' = rows,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @assumes QUINT-ASS-3
  action SweepRoot(r: str): bool = all {
    r.in(ROOTS),
    sweepPending.get(r),
    rows' = rows.keys().filter(p => not (rootOf(p) == r and not(files.get(p).present))).mapBy(k => rows.get(k)),
    sweepPending' = sweepPending.put(r, false),
    files' = files,
    generations' = generations,
    intake' = intake,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt observer
  action ApiRead: bool = all {
    files' = files,
    generations' = generations,
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt observer
  action ReadMissing(p: str): bool = all {
    p.in(PATHS),
    (rows.keys().contains(p) and not(files.get(p).present)) or (not(rows.keys().contains(p))),
    files' = files,
    generations' = generations,
    intake' = intake,
    rows' = rows,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @assumes QUINT-ASS-6
  // @scope SCOPE-3
  action step: bool = any {
    ExternalModify("p1"), ExternalModify("p2"), ExternalModify("p3"), ExternalModify("p4"), ExternalModify("p5"),
    ExternalDelete("p1"), ExternalDelete("p2"), ExternalDelete("p3"), ExternalDelete("p4"), ExternalDelete("p5"),
    ExternalRenameFile("p1", "p2"), ExternalRenameFile("p1", "p3"), ExternalRenameFile("p1", "p4"), ExternalRenameFile("p1", "p5"),
    ExternalRenameFile("p2", "p1"), ExternalRenameFile("p2", "p3"), ExternalRenameFile("p2", "p4"), ExternalRenameFile("p2", "p5"),
    ExternalRenameFile("p3", "p1"), ExternalRenameFile("p3", "p2"), ExternalRenameFile("p3", "p4"), ExternalRenameFile("p3", "p5"),
    ExternalRenameFile("p4", "p1"), ExternalRenameFile("p4", "p2"), ExternalRenameFile("p4", "p3"), ExternalRenameFile("p4", "p5"),
    ExternalRenameFile("p5", "p1"), ExternalRenameFile("p5", "p2"), ExternalRenameFile("p5", "p3"), ExternalRenameFile("p5", "p4"),
    ExternalRenameDir,
    ExternalSidecarCreate("p1"), ExternalSidecarCreate("p2"), ExternalSidecarCreate("p3"), ExternalSidecarCreate("p4"), ExternalSidecarCreate("p5"),
    ExternalSidecarModify("p1"), ExternalSidecarModify("p2"), ExternalSidecarModify("p3"), ExternalSidecarModify("p4"), ExternalSidecarModify("p5"),
    ExternalSidecarDelete("p1"), ExternalSidecarDelete("p2"), ExternalSidecarDelete("p3"), ExternalSidecarDelete("p4"), ExternalSidecarDelete("p5"),
    ForceModify("p1"), ForceModify("p2"), ForceModify("p3"), ForceModify("p4"), ForceModify("p5"),
    MarkUnprocessable("p1"), MarkUnprocessable("p2"), MarkUnprocessable("p3"), MarkUnprocessable("p4"), MarkUnprocessable("p5"),
    RestoreIndexed("p1"), RestoreIndexed("p2"), RestoreIndexed("p3"), RestoreIndexed("p4"), RestoreIndexed("p5"),
    ProcessCrash, LoseThumbs, DbLoss,
    AppSidecarEdit("p1"), AppSidecarEdit("p2"), AppSidecarEdit("p3"), AppSidecarEdit("p4"), AppSidecarEdit("p5"),
    RotateWriteSidecar("p1"), RotateWriteSidecar("p2"), RotateWriteSidecar("p3"), RotateWriteSidecar("p4"), RotateWriteSidecar("p5"),
    RotateFinish("p1"), RotateFinish("p2"), RotateFinish("p3"), RotateFinish("p4"), RotateFinish("p5"),
    ApiUpload("p1"), ApiUpload("p2"), ApiUpload("p3"), ApiUpload("p4"), ApiUpload("p5"),
    ApiMove("p1", "p2"), ApiMove("p1", "p3"), ApiMove("p1", "p4"), ApiMove("p1", "p5"),
    ApiMove("p2", "p1"), ApiMove("p2", "p3"), ApiMove("p2", "p4"), ApiMove("p2", "p5"),
    ApiMove("p3", "p1"), ApiMove("p3", "p2"), ApiMove("p3", "p4"), ApiMove("p3", "p5"),
    ApiMove("p4", "p1"), ApiMove("p4", "p2"), ApiMove("p4", "p3"), ApiMove("p4", "p5"),
    ApiMove("p5", "p1"), ApiMove("p5", "p2"), ApiMove("p5", "p3"), ApiMove("p5", "p4"),
    ApiAssignDirMove,
    ApiRemove("p1"), ApiRemove("p2"), ApiRemove("p3"), ApiRemove("p4"), ApiRemove("p5"),
    SubmitScan("p1"), SubmitScan("p2"), SubmitScan("p3"), SubmitScan("p4"), SubmitScan("p5"),
    SubmitWatcherIndex("p1"), SubmitWatcherIndex("p2"), SubmitWatcherIndex("p3"), SubmitWatcherIndex("p4"), SubmitWatcherIndex("p5"),
    SubmitWatcherRemove("p1"), SubmitWatcherRemove("p2"), SubmitWatcherRemove("p3"), SubmitWatcherRemove("p4"), SubmitWatcherRemove("p5"),
    WorkerApplySuccess, WorkerSkipSuperseded, WorkerSkipStale, WorkerSkipUnchanged, WorkerOpenFails, WorkerRemoveApply,
    ScanRoot("r1"), ScanRoot("r2"), SweepRoot("r1"), SweepRoot("r2"),
    ApiRead,
    ReadMissing("p1"), ReadMissing("p2"), ReadMissing("p3"), ReadMissing("p4"), ReadMissing("p5"),
  }

  // ---------- Foundational runs ----------

  // @assumes QUINT-ASS-6
  run scanApplyTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @exempt no-backing
  run skipSupersededTest = init
    .then(SubmitScan("p1"))
    .then(SubmitScan("p1"))
    .then(WorkerSkipSuperseded)
    .then(all { assert(NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // Pins the FS_MODIFY_INFLIGHT gap: the in-flight item is dropped and nothing resubmits the path until the next scan, so permanent staleness is expected here.
  // @assumes QUINT-ASS-5
  run skipStaleTest = init
    .then(SubmitScan("p1"))
    .then(ForceModify("p1"))
    .then(WorkerSkipStale)
    .then(all { assert(NoLostUpload and not(NoPermanentStale)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-2
  // @assumes QUINT-ASS-5
  run skipUnchangedTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(SubmitScan("p1"))
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-2
  // @assumes QUINT-ASS-5
  run restoreSkipsTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ForceModify("p1"))
    .then(RestoreIndexed("p1"))
    .then(SubmitScan("p1"))
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-1
  run copyDistinctIdsTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(SubmitScan("p2"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p1").assetHash == rows.get("p2").assetHash and rows.get("p1").assetId != rows.get("p2").assetId), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run sidecarDeleteConvergesTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalSidecarModify("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalSidecarDelete("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p1").sidecar == 0), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run renameFileTest = init
    .then(ExternalDelete("p2"))
    .then(SubmitWatcherRemove("p2"))
    .then(WorkerRemoveApply)
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalRenameFile("p1", "p2"))
    .then(WorkerRemoveApply)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and not(rows.keys().contains("p1")) and rows.keys().contains("p2")), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @scope SCOPE-5
  run uploadTest = init
    .then(ApiUpload("p1"))
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run removeTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-1
  run apiMoveSkipsRescanTest = init
    .then(ExternalDelete("p2"))
    .then(SubmitWatcherRemove("p2"))
    .then(WorkerRemoveApply)
    .then(ApiUpload("p1"))
    .then(ApiMove("p1", "p2"))
    .then(SubmitWatcherIndex("p2"))
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p2").assetId == 6), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @scope SCOPE-5
  run apiRemoveTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ApiRemove("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run scanOnlyConvergenceTest = init
    .then(ExternalDelete("p1"))
    .then(all { assert(rows.keys().contains("p1") and not(Converged) and not(NoPermanentStale)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r1"))
    .then(all { assert(NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(SweepRoot("r1"))
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run scanHealsMissingRowTest = init
    .then(ExternalDelete("p1"))
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(ExternalModify("p1"))
    .then(all { assert(not(rows.keys().contains("p1")) and not(Converged)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerSkipSuperseded)
    .then(WorkerSkipSuperseded)
    .then(WorkerSkipSuperseded)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run scanHealsStaleRowTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ForceModify("p1"))
    .then(all { assert(not(Converged)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run perRootSweepTest = init
    .then(ExternalDelete("p5"))
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(all { assert(not(Converged) and not(NoPermanentStale)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r2"))
    .then(all { assert(NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(SweepRoot("r2"))
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-6
  run fsCreateTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalModify("p1"))
    .then(SubmitWatcherIndex("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-6
  run fsModifyTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalModify("p1"))
    .then(SubmitWatcherIndex("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-5
  run fsModifyInflightTest = init
    .then(SubmitScan("p1"))
    .then(ForceModify("p1"))
    .then(WorkerSkipStale)
    .then(all { assert(not(Converged) and intake.size() == 0), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run fsAtomicReplaceTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalModify("p1"))
    .then(SubmitWatcherIndex("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run sidecarCreateTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalSidecarCreate("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p1").sidecar == files.get("p1").sidecar and rows.get("p1").sidecar != 0), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run sidecarModifyTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalSidecarModify("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-1
  // @scope SCOPE-5
  // @scope SCOPE-7
  run subtreeMoveTest = init
    .then(ExternalDelete("p3"))
    .then(SubmitWatcherRemove("p3"))
    .then(WorkerRemoveApply)
    .then(ExternalDelete("p4"))
    .then(SubmitWatcherRemove("p4"))
    .then(WorkerRemoveApply)
    .then(ExternalRenameDir)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and not(rows.keys().contains("p1")) and not(rows.keys().contains("p2")) and rows.keys().contains("p3") and rows.keys().contains("p4") and rows.get("p3").assetId == 1 and rows.get("p4").assetId == 2), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-1
  // @scope SCOPE-5
  // @scope SCOPE-7
  run assignDirMoveTest = init
    .then(ExternalDelete("p3"))
    .then(SubmitWatcherRemove("p3"))
    .then(WorkerRemoveApply)
    .then(ExternalDelete("p4"))
    .then(SubmitWatcherRemove("p4"))
    .then(WorkerRemoveApply)
    .then(ApiAssignDirMove)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and not(rows.keys().contains("p1")) and not(rows.keys().contains("p2")) and rows.keys().contains("p3") and rows.keys().contains("p4") and rows.get("p3").assetId == 1 and rows.get("p4").assetId == 2), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @scope SCOPE-5
  run rotateTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(RotateWriteSidecar("p1"))
    .then(all { assert(rows.get("p1").sidecar != files.get("p1").sidecar and not(Converged)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @scope SCOPE-5
  // @scope SCOPE-6
  run rotateCrashTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(RotateWriteSidecar("p1"))
    .then(ProcessCrash)
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run watcherGapTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ForceModify("p1"))
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // A scan that snapshots before the change drops the stale item; the next
  // backstop scan heals it, so convergence is bounded by two scan cycles
  // (one backstop interval after the change).
  // @assumes QUINT-ASS-3
  // @assumes QUINT-ASS-7
  run boundedConvergenceTest = init
    .then(ScanRoot("r1"))
    .then(ForceModify("p1"))
    .then(WorkerSkipStale)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(all { assert(not(Converged) and intake.size() == 0), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and intake.size() == 0), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @scope SCOPE-6
  run crashTest = init
    .then(SubmitScan("p1"))
    .then(ForceModify("p1"))
    .then(ProcessCrash)
    .then(all { assert(intake.size() == 0 and not(Converged)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-4
  run derivedLossTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(LoseThumbs)
    .then(all { assert(not(Converged)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r1"))
    .then(ScanRoot("r2"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @scope SCOPE-6
  run dbLossTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(SubmitScan("p2"))
    .then(WorkerApplySuccess)
    .then(DbLoss)
    .then(all { assert(rows.keys().size() == 0), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r1"))
    .then(ScanRoot("r2"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p1").assetId != rows.get("p2").assetId), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @exempt no-backing
  run failingForeverTest = init
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(MarkUnprocessable("p1"))
    .then(ExternalModify("p1"))
    .then(ScanRoot("r1"))
    .then(WorkerOpenFails)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(not(rows.keys().contains("p1")) and Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })

  // @assumes QUINT-ASS-3
  run readMissingTest = init
    .then(ExternalDelete("p1"))
    .then(ReadMissing("p1"))
    .then(all { assert(not(Converged)), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, rows' = rows, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable })
}