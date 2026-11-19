// Proof-lane step: the environment (FS events, API calls, faults) fires at
// any time and the pipeline is the system, so the goal is as-if-serial
// eventual consistency — a drained state agrees with the FS record (ASS-3/G5).
//
// Completion machine. The environment does not stop on its own, so the
// invariant needs an observation point: a three-phase cycle in which the
// environment is frozen, the pipeline drains, and a scan of each root runs
// last (G1: changes stopped *and* a full scan completed).
//
//   phase 0 free     environment actions and every pipeline branch may fire;
//                    the pipeline may be arbitrarily dirty.
//   phase 1 draining the environment is frozen; work-consuming branches,
//                    crashes, and untracked rescans fire; BeginScan opens
//                    phase 2 once the pipeline is quiescent.
//   phase 2 scanning CompleteScan runs the scan of each root exactly once,
//                    then work-consuming branches drain the work the scan
//                    creates. Quiescence here is the state
//                    CompletionInvariant claims about.
//
// Why the scan is last: ScanRoot creates work (one intake item per present
// path plus sweepPending), so it cannot follow the drain it feeds; only the
// scan observes FS state no watcher reported (ForceModify, a dropped sidecar
// event, crash residue), so the drain before it cannot replace it. Scan
// triggers are system behaviour (Flag::Rescan, the periodic backstop), not
// user events, so they may also fire alongside the drain and its crashes:
// plain ScanRoot stays available in phases 0 and 1 as an untracked scan. Only
// the final cycle is assumed clean — one tracked CompleteScan per root in
// phase 2, with no plain ScanRoot there. Any work an untracked scan creates
// must drain before BeginScan admits the final cycle.
//
// Crash resilience: ProcessCrash/LoseThumbs/DbLoss are allowed in phases 0 and
// 1 — a crash before or during the drain cannot be excluded — and excluded
// from phase 2, where completion is claimed; their residue is what the phase-2
// scan repairs. RemoveIdOnly (the N5 torn window) follows the same rule: it
// creates residue only a scan rediscovers, so it is not a resolution action.
//
// Phase classification. Criterion: does the action create pending work or
// consume it? Pending work = intake items, sweepPending, sweepBatch, a held
// lease, or FS/DB divergence only a scan rediscovers.
//
//   consumers (phases 1+2):
//     WorkerApplySuccess      tasks/queue/worker.rs IntakeKind apply -> workflow::process_file
//     WorkerSkipSuperseded    tasks/queue/worker.rs superseded-generation branch
//     WorkerSkipStale         tasks/queue/worker.rs FS_MODIFY_INFLIGHT drop branch
//     WorkerSkipUnchanged     tasks/queue/worker.rs unchanged-snapshot branch
//     WorkerOpenFails         tasks/queue/worker.rs open-failure branch
//     WorkerRemoveApply       tasks/queue/worker.rs remove branch
//     UploadIndexed           tasks/queue/worker.rs intake apply (index path)
//     IntakePickupFileGone    tasks/queue/worker.rs gone-file outcome
//     SweepEnumerate          album_index.rs::sweep_stale_asset_paths enumeration
//     SweepRecheck            sweep_stale_asset_paths inline re-check
//     SweepRemoveBatchApply   flush_tree.rs::sweep_remove_batch
//     SweepRoot               album_index.rs sweep trigger
//     MoveRowRewrite          assign_album.rs::move_asset_into_album row rewrite + lease release
//     RotateFinish            router/put/rotate_image.rs finish phase
//     MemberWrite             mark_album_for_update (no-op in this model)
//
//   initiators (phase 0 only):
//     ExternalModify          watcher FS_CREATE/FS_MODIFY/FS_ATOMIC_REPLACE
//     ExternalDelete          watcher FS_DELETE
//     ExternalRenameFile      watcher FS_RENAME_FILE (remove + create submits)
//     ExternalRenameDir       watcher FS_RENAME_DIR
//     ExternalSidecarCreate   watcher FS_SIDECAR_CREATE
//     ExternalSidecarModify   watcher FS_SIDECAR_MODIFY
//     ExternalSidecarDelete   watcher FS_SIDECAR_DELETE
//     ForceModify             FS_MODIFY_INFLIGHT (file changed, nothing submitted)
//     MarkUnprocessable       ingest failure (model-only unprocessable set)
//     RestoreIndexed          restore-from-backup resubmit
//     AppSidecarEdit          process/sidecar_edit.rs::commit_metadata_edits
//     RotateWriteSidecar      router/put/rotate_image.rs write phase
//     ApiUpload               router upload endpoint
//     ApiRemove               router delete -> process_deletes
//     ApiMove                 assign_album.rs::move_asset_into_album (synchronous)
//     ApiAssignDirMove        assign_album.rs + apply_dir_move_plans
//     ApiMoveFsOnly           assign_album.rs move, filesystem half only
//     DirMoveFsOnly           apply_dir_move_plans
//     MoveLeaseAcquire        assign_album.rs move window (lease_paths)
//     UploadStoredOnly        post_upload.rs::upload + submit_uploaded_path
//     SidecarEditPartial      process/sidecar_edit.rs partial-commit window
//     DroppedSidecarModify    dropped watcher sidecar event
//     AlbumShareEdit          edit_share RMW (DB-only share_list)
//     TrashToggle             trash toggle RMW + album delta
//     SubmitScan              Flag::Rescan / scan submit
//     SubmitWatcherIndex      tasks/queue/intake.rs watcher index submit
//     SubmitWatcherRemove     tasks/queue/intake.rs watcher remove submit
//
//   work-creating observer (untracked phases 0+1, tracked phase 2):
//     ScanRoot                album_index.rs::index_album_with_root (scan + sweep);
//                             direct branches in phases 0-1 do not update the
//                             watermark, CompleteScan is its phase-2 wrapper
//
//   crashes/losses (phases 0+1):
//     ProcessCrash            PROCESS_CRASH recovery (SCOPE-6)
//     LoseThumbs              DERIVED_STORE_LOSS recovery
//     DbLoss                  DB_LOSS recovery
//     RemoveIdOnly            N5 torn-window remove (scan-only residue)
//
//   observers (all phases): ApiRead, ReadMissing — router read paths, pure
//   stutters, no work created.
//
//   completion bookkeeping (this module): Stay, BeginDrain, BeginScan,
//   CompleteScan, initCompletion, stepCompletion, Skip — phase and scan
//   watermark only, no pipeline-state change beyond the composed action.
module asset_aspects_step {
  import eventual_consistency_state.* from "./eventual-consistency-state"
  import eventual_consistency_assumptions.* from "./eventual-consistency-assumptions"
  import eventual_consistency_goal.* from "./eventual-consistency-goal"
  import eventual_consistency.* from "./eventual-consistency"
  import asset_aspects.* from "./asset-aspects"

  // Completion-machine state. Base actions do not assign these vars, so they
  // stay unconstrained there; the machine's own actions below assign them.
  var phase: int
  var scannedRoots: Set[str]

  // Stutter branch: models a tick in which nothing observable changes, so
  // the scheduler never has to force progress. It stutters the completion
  // vars too, so it is enabled in every phase.
  // @scope SCOPE-13
  action Skip: bool = all {
    files' = files,
    generations' = generations,
    intake' = intake,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    phase' = phase,
    scannedRoots' = scannedRoots,
  }

  // Branch bookkeeping: branches whose action leaves the phase machine alone
  // must still assign `phase'`/`scannedRoots'`. Quint leaves an unassigned
  // primed var unconstrained and requires every branch of an `any` to update
  // the same variable set (QNT202), so the stutter is explicit.
  // @scope SCOPE-13
  action Stay(b: bool): bool = all {
    b,
    phase' = phase,
    scannedRoots' = scannedRoots,
  }

  // Quiescence: no queued intake, no enumerated sweep batch, no root with a
  // sweep owed, and no held path lease. These four structures are the
  // model's only in-flight markers: intake carries a stored-but-unindexed
  // upload until the worker applies or drops it, leasedPaths carries the move
  // (MoveLeaseAcquire/ApiMoveFsOnly) and rotate (RotateWriteSidecar) windows,
  // and sweepPending/sweepBatch are the two sweep stages. A partial sidecar
  // edit (SidecarEditPartial) and a dropped sidecar event
  // (DroppedSidecarModify) leave no marker at all: only a scan rediscovers
  // them, which is why those actions are free-phase-only.
  // @assumes ASS-6
  // @scope SCOPE-13
  val Quiescent: bool =
    intake.size() == 0 and
    sweepBatch.keys().size() == 0 and
    ROOTS.forall(r => not(sweepPending.get(r))) and
    leasedPaths.size() == 0

  // As-if-serial goal over quiescence alone. Not asserted: quiescence holds
  // in states no scan has reached since the last FS change (the
  // FS_MODIFY_INFLIGHT gap), so this form is not the checked invariant.
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-13
  val ConvergenceInvariant: bool =
    Quiescent implies (Converged and AspectsAligned)

  // Completion invariant: once the machine has taken the scan of every root
  // and quiesced again, the derived state agrees with the FS record. Safety
  // only, no fairness premise (SCOPE-3).
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-13
  val CompletionInvariant: bool =
    (phase == 2 and scannedRoots == ROOTS and Quiescent) implies (Converged and AspectsAligned)

  // Every reachable state stays inside the declared bounds: one map entry
  // per modeled path and root, and no intake item for a path outside PATHS.
  // @assumes ASS-1
  // @scope SCOPE-2
  val WithinBounds: bool =
    files.keys() == PATHS and
    generations.keys() == PATHS and
    sweepPending.keys() == ROOTS and
    intake.forall(i => i.path.in(PATHS))

  // The aspect-table key is the record's assetId, in every state (aspect
  // (d) identity coherence; unaffected by torn-window tombstone states).
  // @assumes ASS-1
  // @scope SCOPE-11
  val AssetIdKeyCoherent: bool =
    assetById.keys().forall(id => assetById.get(id).assetId == id)

  // Free -> draining. The environment stops; the pipeline state is untouched.
  // @scope SCOPE-13
  action BeginDrain: bool = all {
    phase == 0,
    phase' = 1,
    scannedRoots' = scannedRoots,
    files' = files,
    generations' = generations,
    intake' = intake,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // Draining -> scanning, taken only at quiescence: the scan must observe the
  // drained state, not an in-flight window it would misread.
  // @assumes ASS-6
  // @scope SCOPE-13
  action BeginScan: bool = all {
    phase == 1,
    Quiescent,
    phase' = 2,
    scannedRoots' = scannedRoots,
    files' = files,
    generations' = generations,
    intake' = intake,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // One full scan of a root in phase 2, recorded in the watermark. The body is
  // ScanRoot plus the watermark: ScanRoot does not assign phase/scannedRoots,
  // so the extra conjuncts do not double-update.
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-13
  action CompleteScan(r: str): bool = all {
    phase == 2,
    not(scannedRoots.contains(r)),
    r.in(ROOTS),
    ScanRoot(r),
    phase' = phase,
    scannedRoots' = scannedRoots.union(Set(r)),
  }

  // Verify entry point: the base initial state plus an empty phase machine.
  // @scope SCOPE-1
  action initCompletion: bool = all {
    init,
    phase' = 0,
    scannedRoots' = Set(),
  }

  // Uniform choice over phase-enabled branches; no ordering inside a phase.
  // @assumes ASS-6
  // @scope SCOPE-13
  action stepCompletion: bool = any {
    // phase 0: every action the pipeline and environment can take.
    all { phase == 0, Stay(ExternalModify("p1")) },
    all { phase == 0, Stay(ExternalModify("p2")) },
    all { phase == 0, Stay(ExternalModify("p3")) },
    all { phase == 0, Stay(ExternalModify("p4")) },
    all { phase == 0, Stay(ExternalModify("p5")) },
    all { phase == 0, Stay(ExternalDelete("p1")) },
    all { phase == 0, Stay(ExternalDelete("p2")) },
    all { phase == 0, Stay(ExternalDelete("p3")) },
    all { phase == 0, Stay(ExternalDelete("p4")) },
    all { phase == 0, Stay(ExternalDelete("p5")) },
    all { phase == 0, Stay(ExternalRenameFile("p1", "p2")) },
    all { phase == 0, Stay(ExternalRenameFile("p1", "p3")) },
    all { phase == 0, Stay(ExternalRenameFile("p1", "p4")) },
    all { phase == 0, Stay(ExternalRenameFile("p1", "p5")) },
    all { phase == 0, Stay(ExternalRenameFile("p2", "p1")) },
    all { phase == 0, Stay(ExternalRenameFile("p2", "p3")) },
    all { phase == 0, Stay(ExternalRenameFile("p2", "p4")) },
    all { phase == 0, Stay(ExternalRenameFile("p2", "p5")) },
    all { phase == 0, Stay(ExternalRenameFile("p3", "p1")) },
    all { phase == 0, Stay(ExternalRenameFile("p3", "p2")) },
    all { phase == 0, Stay(ExternalRenameFile("p3", "p4")) },
    all { phase == 0, Stay(ExternalRenameFile("p3", "p5")) },
    all { phase == 0, Stay(ExternalRenameFile("p4", "p1")) },
    all { phase == 0, Stay(ExternalRenameFile("p4", "p2")) },
    all { phase == 0, Stay(ExternalRenameFile("p4", "p3")) },
    all { phase == 0, Stay(ExternalRenameFile("p4", "p5")) },
    all { phase == 0, Stay(ExternalRenameFile("p5", "p1")) },
    all { phase == 0, Stay(ExternalRenameFile("p5", "p2")) },
    all { phase == 0, Stay(ExternalRenameFile("p5", "p3")) },
    all { phase == 0, Stay(ExternalRenameFile("p5", "p4")) },
    all { phase == 0, Stay(ExternalRenameDir) },
    all { phase == 0, Stay(ExternalSidecarCreate("p1")) },
    all { phase == 0, Stay(ExternalSidecarCreate("p2")) },
    all { phase == 0, Stay(ExternalSidecarCreate("p3")) },
    all { phase == 0, Stay(ExternalSidecarCreate("p4")) },
    all { phase == 0, Stay(ExternalSidecarCreate("p5")) },
    all { phase == 0, Stay(ExternalSidecarModify("p1")) },
    all { phase == 0, Stay(ExternalSidecarModify("p2")) },
    all { phase == 0, Stay(ExternalSidecarModify("p3")) },
    all { phase == 0, Stay(ExternalSidecarModify("p4")) },
    all { phase == 0, Stay(ExternalSidecarModify("p5")) },
    all { phase == 0, Stay(ExternalSidecarDelete("p1")) },
    all { phase == 0, Stay(ExternalSidecarDelete("p2")) },
    all { phase == 0, Stay(ExternalSidecarDelete("p3")) },
    all { phase == 0, Stay(ExternalSidecarDelete("p4")) },
    all { phase == 0, Stay(ExternalSidecarDelete("p5")) },
    all { phase == 0, Stay(ForceModify("p1")) },
    all { phase == 0, Stay(ForceModify("p2")) },
    all { phase == 0, Stay(ForceModify("p3")) },
    all { phase == 0, Stay(ForceModify("p4")) },
    all { phase == 0, Stay(ForceModify("p5")) },
    all { phase == 0, Stay(MarkUnprocessable("p1")) },
    all { phase == 0, Stay(MarkUnprocessable("p2")) },
    all { phase == 0, Stay(MarkUnprocessable("p3")) },
    all { phase == 0, Stay(MarkUnprocessable("p4")) },
    all { phase == 0, Stay(MarkUnprocessable("p5")) },
    all { phase == 0, Stay(RestoreIndexed("p1")) },
    all { phase == 0, Stay(RestoreIndexed("p2")) },
    all { phase == 0, Stay(RestoreIndexed("p3")) },
    all { phase == 0, Stay(RestoreIndexed("p4")) },
    all { phase == 0, Stay(RestoreIndexed("p5")) },
    all { phase == 0, Stay(ProcessCrash) },
    all { phase == 0, Stay(LoseThumbs) },
    all { phase == 0, Stay(DbLoss) },
    all { phase == 0, Stay(AppSidecarEdit("p1")) },
    all { phase == 0, Stay(AppSidecarEdit("p2")) },
    all { phase == 0, Stay(AppSidecarEdit("p3")) },
    all { phase == 0, Stay(AppSidecarEdit("p4")) },
    all { phase == 0, Stay(AppSidecarEdit("p5")) },
    all { phase == 0, Stay(RotateWriteSidecar("p1")) },
    all { phase == 0, Stay(RotateWriteSidecar("p2")) },
    all { phase == 0, Stay(RotateWriteSidecar("p3")) },
    all { phase == 0, Stay(RotateWriteSidecar("p4")) },
    all { phase == 0, Stay(RotateWriteSidecar("p5")) },
    all { phase == 0, Stay(RotateFinish("p1")) },
    all { phase == 0, Stay(RotateFinish("p2")) },
    all { phase == 0, Stay(RotateFinish("p3")) },
    all { phase == 0, Stay(RotateFinish("p4")) },
    all { phase == 0, Stay(RotateFinish("p5")) },
    all { phase == 0, Stay(ApiUpload("p1")) },
    all { phase == 0, Stay(ApiUpload("p2")) },
    all { phase == 0, Stay(ApiUpload("p3")) },
    all { phase == 0, Stay(ApiUpload("p4")) },
    all { phase == 0, Stay(ApiUpload("p5")) },
    all { phase == 0, Stay(ApiMove("p1", "p2")) },
    all { phase == 0, Stay(ApiMove("p1", "p3")) },
    all { phase == 0, Stay(ApiMove("p1", "p4")) },
    all { phase == 0, Stay(ApiMove("p1", "p5")) },
    all { phase == 0, Stay(ApiMove("p2", "p1")) },
    all { phase == 0, Stay(ApiMove("p2", "p3")) },
    all { phase == 0, Stay(ApiMove("p2", "p4")) },
    all { phase == 0, Stay(ApiMove("p2", "p5")) },
    all { phase == 0, Stay(ApiMove("p3", "p1")) },
    all { phase == 0, Stay(ApiMove("p3", "p2")) },
    all { phase == 0, Stay(ApiMove("p3", "p4")) },
    all { phase == 0, Stay(ApiMove("p3", "p5")) },
    all { phase == 0, Stay(ApiMove("p4", "p1")) },
    all { phase == 0, Stay(ApiMove("p4", "p2")) },
    all { phase == 0, Stay(ApiMove("p4", "p3")) },
    all { phase == 0, Stay(ApiMove("p4", "p5")) },
    all { phase == 0, Stay(ApiMove("p5", "p1")) },
    all { phase == 0, Stay(ApiMove("p5", "p2")) },
    all { phase == 0, Stay(ApiMove("p5", "p3")) },
    all { phase == 0, Stay(ApiMove("p5", "p4")) },
    all { phase == 0, Stay(ApiAssignDirMove) },
    all { phase == 0, Stay(ApiRemove("p1")) },
    all { phase == 0, Stay(ApiRemove("p2")) },
    all { phase == 0, Stay(ApiRemove("p3")) },
    all { phase == 0, Stay(ApiRemove("p4")) },
    all { phase == 0, Stay(ApiRemove("p5")) },
    all { phase == 0, Stay(SubmitScan("p1")) },
    all { phase == 0, Stay(SubmitScan("p2")) },
    all { phase == 0, Stay(SubmitScan("p3")) },
    all { phase == 0, Stay(SubmitScan("p4")) },
    all { phase == 0, Stay(SubmitScan("p5")) },
    all { phase == 0, Stay(SubmitWatcherIndex("p1")) },
    all { phase == 0, Stay(SubmitWatcherIndex("p2")) },
    all { phase == 0, Stay(SubmitWatcherIndex("p3")) },
    all { phase == 0, Stay(SubmitWatcherIndex("p4")) },
    all { phase == 0, Stay(SubmitWatcherIndex("p5")) },
    all { phase == 0, Stay(SubmitWatcherRemove("p1")) },
    all { phase == 0, Stay(SubmitWatcherRemove("p2")) },
    all { phase == 0, Stay(SubmitWatcherRemove("p3")) },
    all { phase == 0, Stay(SubmitWatcherRemove("p4")) },
    all { phase == 0, Stay(SubmitWatcherRemove("p5")) },
    all { phase == 0, Stay(WorkerApplySuccess) },
    all { phase == 0, Stay(WorkerSkipSuperseded) },
    all { phase == 0, Stay(WorkerSkipStale) },
    all { phase == 0, Stay(WorkerSkipUnchanged) },
    all { phase == 0, Stay(WorkerOpenFails) },
    all { phase == 0, Stay(WorkerRemoveApply) },
    all { phase == 0, Stay(ScanRoot("r1")) },
    all { phase == 0, Stay(ScanRoot("r2")) },
    all { phase == 0, Stay(SweepRoot("r1")) },
    all { phase == 0, Stay(SweepRoot("r2")) },
    all { phase == 0, Stay(ApiRead) },
    all { phase == 0, Stay(ReadMissing("p1")) },
    all { phase == 0, Stay(ReadMissing("p2")) },
    all { phase == 0, Stay(ReadMissing("p3")) },
    all { phase == 0, Stay(ReadMissing("p4")) },
    all { phase == 0, Stay(ReadMissing("p5")) },
    all { phase == 0, Stay(DirMoveFsOnly) },
    all { phase == 0, Stay(ApiMoveFsOnly("p1", "p2")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p1", "p3")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p1", "p4")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p1", "p5")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p2", "p1")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p2", "p3")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p2", "p4")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p2", "p5")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p3", "p1")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p3", "p2")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p3", "p4")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p3", "p5")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p4", "p1")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p4", "p2")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p4", "p3")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p4", "p5")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p5", "p1")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p5", "p2")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p5", "p3")) },
    all { phase == 0, Stay(ApiMoveFsOnly("p5", "p4")) },
    all { phase == 0, Stay(SweepEnumerate("r1")) },
    all { phase == 0, Stay(SweepEnumerate("r2")) },
    all { phase == 0, Stay(SweepRecheck) },
    all { phase == 0, Stay(SweepRemoveBatchApply) },
    all { phase == 0, Stay(MoveLeaseAcquire("p1", "p2")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p1", "p3")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p1", "p4")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p1", "p5")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p2", "p1")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p2", "p3")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p2", "p4")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p2", "p5")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p3", "p1")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p3", "p2")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p3", "p4")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p3", "p5")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p4", "p1")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p4", "p2")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p4", "p3")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p4", "p5")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p5", "p1")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p5", "p2")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p5", "p3")) },
    all { phase == 0, Stay(MoveLeaseAcquire("p5", "p4")) },
    all { phase == 0, Stay(MoveRowRewrite("p1", "p2")) },
    all { phase == 0, Stay(MoveRowRewrite("p1", "p3")) },
    all { phase == 0, Stay(MoveRowRewrite("p1", "p4")) },
    all { phase == 0, Stay(MoveRowRewrite("p1", "p5")) },
    all { phase == 0, Stay(MoveRowRewrite("p2", "p1")) },
    all { phase == 0, Stay(MoveRowRewrite("p2", "p3")) },
    all { phase == 0, Stay(MoveRowRewrite("p2", "p4")) },
    all { phase == 0, Stay(MoveRowRewrite("p2", "p5")) },
    all { phase == 0, Stay(MoveRowRewrite("p3", "p1")) },
    all { phase == 0, Stay(MoveRowRewrite("p3", "p2")) },
    all { phase == 0, Stay(MoveRowRewrite("p3", "p4")) },
    all { phase == 0, Stay(MoveRowRewrite("p3", "p5")) },
    all { phase == 0, Stay(MoveRowRewrite("p4", "p1")) },
    all { phase == 0, Stay(MoveRowRewrite("p4", "p2")) },
    all { phase == 0, Stay(MoveRowRewrite("p4", "p3")) },
    all { phase == 0, Stay(MoveRowRewrite("p4", "p5")) },
    all { phase == 0, Stay(MoveRowRewrite("p5", "p1")) },
    all { phase == 0, Stay(MoveRowRewrite("p5", "p2")) },
    all { phase == 0, Stay(MoveRowRewrite("p5", "p3")) },
    all { phase == 0, Stay(MoveRowRewrite("p5", "p4")) },
    all { phase == 0, Stay(SidecarEditPartial("p1")) },
    all { phase == 0, Stay(SidecarEditPartial("p2")) },
    all { phase == 0, Stay(SidecarEditPartial("p3")) },
    all { phase == 0, Stay(SidecarEditPartial("p4")) },
    all { phase == 0, Stay(SidecarEditPartial("p5")) },
    all { phase == 0, Stay(DroppedSidecarModify("p1")) },
    all { phase == 0, Stay(DroppedSidecarModify("p2")) },
    all { phase == 0, Stay(DroppedSidecarModify("p3")) },
    all { phase == 0, Stay(DroppedSidecarModify("p4")) },
    all { phase == 0, Stay(DroppedSidecarModify("p5")) },
    all { phase == 0, Stay(UploadStoredOnly("p1")) },
    all { phase == 0, Stay(UploadStoredOnly("p2")) },
    all { phase == 0, Stay(UploadStoredOnly("p3")) },
    all { phase == 0, Stay(UploadStoredOnly("p4")) },
    all { phase == 0, Stay(UploadStoredOnly("p5")) },
    all { phase == 0, Stay(UploadIndexed("p1")) },
    all { phase == 0, Stay(UploadIndexed("p2")) },
    all { phase == 0, Stay(UploadIndexed("p3")) },
    all { phase == 0, Stay(UploadIndexed("p4")) },
    all { phase == 0, Stay(UploadIndexed("p5")) },
    all { phase == 0, Stay(IntakePickupFileGone("p1")) },
    all { phase == 0, Stay(IntakePickupFileGone("p2")) },
    all { phase == 0, Stay(IntakePickupFileGone("p3")) },
    all { phase == 0, Stay(IntakePickupFileGone("p4")) },
    all { phase == 0, Stay(IntakePickupFileGone("p5")) },
    all { phase == 0, Stay(RemoveIdOnly("p1")) },
    all { phase == 0, Stay(RemoveIdOnly("p2")) },
    all { phase == 0, Stay(RemoveIdOnly("p3")) },
    all { phase == 0, Stay(RemoveIdOnly("p4")) },
    all { phase == 0, Stay(RemoveIdOnly("p5")) },
    all { phase == 0, Stay(MemberWrite("p1")) },
    all { phase == 0, Stay(MemberWrite("p2")) },
    all { phase == 0, Stay(MemberWrite("p3")) },
    all { phase == 0, Stay(MemberWrite("p4")) },
    all { phase == 0, Stay(MemberWrite("p5")) },
    all { phase == 0, Stay(AlbumShareEdit) },
    all { phase == 0, Stay(TrashToggle("p1")) },
    all { phase == 0, Stay(TrashToggle("p2")) },
    all { phase == 0, Stay(TrashToggle("p3")) },
    all { phase == 0, Stay(TrashToggle("p4")) },
    all { phase == 0, Stay(TrashToggle("p5")) },
    all { phase == 0, Skip },
    all { phase == 0, BeginDrain },

    // phase 1: drain. Work-consuming branches, then crashes, then the
    // quiescence-gated phase transition. Untracked ScanRoot branches model the
    // rescan/backstop triggers mixing into the drain; their intake and
    // sweepPending must drain before BeginScan is enabled.
    all { phase == 1, Stay(WorkerApplySuccess) },
    all { phase == 1, Stay(WorkerSkipSuperseded) },
    all { phase == 1, Stay(WorkerSkipStale) },
    all { phase == 1, Stay(WorkerSkipUnchanged) },
    all { phase == 1, Stay(WorkerOpenFails) },
    all { phase == 1, Stay(WorkerRemoveApply) },
    all { phase == 1, Stay(SweepEnumerate("r1")) },
    all { phase == 1, Stay(SweepEnumerate("r2")) },
    all { phase == 1, Stay(SweepRecheck) },
    all { phase == 1, Stay(SweepRemoveBatchApply) },
    all { phase == 1, Stay(SweepRoot("r1")) },
    all { phase == 1, Stay(SweepRoot("r2")) },
    all { phase == 1, Stay(UploadIndexed("p1")) },
    all { phase == 1, Stay(UploadIndexed("p2")) },
    all { phase == 1, Stay(UploadIndexed("p3")) },
    all { phase == 1, Stay(UploadIndexed("p4")) },
    all { phase == 1, Stay(UploadIndexed("p5")) },
    all { phase == 1, Stay(IntakePickupFileGone("p1")) },
    all { phase == 1, Stay(IntakePickupFileGone("p2")) },
    all { phase == 1, Stay(IntakePickupFileGone("p3")) },
    all { phase == 1, Stay(IntakePickupFileGone("p4")) },
    all { phase == 1, Stay(IntakePickupFileGone("p5")) },
    all { phase == 1, Stay(MoveRowRewrite("p1", "p2")) },
    all { phase == 1, Stay(MoveRowRewrite("p1", "p3")) },
    all { phase == 1, Stay(MoveRowRewrite("p1", "p4")) },
    all { phase == 1, Stay(MoveRowRewrite("p1", "p5")) },
    all { phase == 1, Stay(MoveRowRewrite("p2", "p1")) },
    all { phase == 1, Stay(MoveRowRewrite("p2", "p3")) },
    all { phase == 1, Stay(MoveRowRewrite("p2", "p4")) },
    all { phase == 1, Stay(MoveRowRewrite("p2", "p5")) },
    all { phase == 1, Stay(MoveRowRewrite("p3", "p1")) },
    all { phase == 1, Stay(MoveRowRewrite("p3", "p2")) },
    all { phase == 1, Stay(MoveRowRewrite("p3", "p4")) },
    all { phase == 1, Stay(MoveRowRewrite("p3", "p5")) },
    all { phase == 1, Stay(MoveRowRewrite("p4", "p1")) },
    all { phase == 1, Stay(MoveRowRewrite("p4", "p2")) },
    all { phase == 1, Stay(MoveRowRewrite("p4", "p3")) },
    all { phase == 1, Stay(MoveRowRewrite("p4", "p5")) },
    all { phase == 1, Stay(MoveRowRewrite("p5", "p1")) },
    all { phase == 1, Stay(MoveRowRewrite("p5", "p2")) },
    all { phase == 1, Stay(MoveRowRewrite("p5", "p3")) },
    all { phase == 1, Stay(MoveRowRewrite("p5", "p4")) },
    all { phase == 1, Stay(RotateFinish("p1")) },
    all { phase == 1, Stay(RotateFinish("p2")) },
    all { phase == 1, Stay(RotateFinish("p3")) },
    all { phase == 1, Stay(RotateFinish("p4")) },
    all { phase == 1, Stay(RotateFinish("p5")) },
    all { phase == 1, Stay(MemberWrite("p1")) },
    all { phase == 1, Stay(MemberWrite("p2")) },
    all { phase == 1, Stay(MemberWrite("p3")) },
    all { phase == 1, Stay(MemberWrite("p4")) },
    all { phase == 1, Stay(MemberWrite("p5")) },
    all { phase == 1, Stay(ApiRead) },
    all { phase == 1, Stay(ReadMissing("p1")) },
    all { phase == 1, Stay(ReadMissing("p2")) },
    all { phase == 1, Stay(ReadMissing("p3")) },
    all { phase == 1, Stay(ReadMissing("p4")) },
    all { phase == 1, Stay(ReadMissing("p5")) },
    all { phase == 1, Stay(ProcessCrash) },
    all { phase == 1, Stay(LoseThumbs) },
    all { phase == 1, Stay(DbLoss) },
    all { phase == 1, Stay(RemoveIdOnly("p1")) },
    all { phase == 1, Stay(RemoveIdOnly("p2")) },
    all { phase == 1, Stay(RemoveIdOnly("p3")) },
    all { phase == 1, Stay(RemoveIdOnly("p4")) },
    all { phase == 1, Stay(RemoveIdOnly("p5")) },
    all { phase == 1, Stay(ScanRoot("r1")) },
    all { phase == 1, Stay(ScanRoot("r2")) },
    BeginScan,
    all { phase == 1, Skip },

    // phase 2: scan each root once, then drain what the scan created. Plain
    // ScanRoot is absent here; CompleteScan is the only scan entry, so the
    // watermark records the final cycle.
    CompleteScan("r1"),
    CompleteScan("r2"),
    all { phase == 2, Stay(WorkerApplySuccess) },
    all { phase == 2, Stay(WorkerSkipSuperseded) },
    all { phase == 2, Stay(WorkerSkipStale) },
    all { phase == 2, Stay(WorkerSkipUnchanged) },
    all { phase == 2, Stay(WorkerOpenFails) },
    all { phase == 2, Stay(WorkerRemoveApply) },
    all { phase == 2, Stay(SweepEnumerate("r1")) },
    all { phase == 2, Stay(SweepEnumerate("r2")) },
    all { phase == 2, Stay(SweepRecheck) },
    all { phase == 2, Stay(SweepRemoveBatchApply) },
    all { phase == 2, Stay(SweepRoot("r1")) },
    all { phase == 2, Stay(SweepRoot("r2")) },
    all { phase == 2, Stay(UploadIndexed("p1")) },
    all { phase == 2, Stay(UploadIndexed("p2")) },
    all { phase == 2, Stay(UploadIndexed("p3")) },
    all { phase == 2, Stay(UploadIndexed("p4")) },
    all { phase == 2, Stay(UploadIndexed("p5")) },
    all { phase == 2, Stay(IntakePickupFileGone("p1")) },
    all { phase == 2, Stay(IntakePickupFileGone("p2")) },
    all { phase == 2, Stay(IntakePickupFileGone("p3")) },
    all { phase == 2, Stay(IntakePickupFileGone("p4")) },
    all { phase == 2, Stay(IntakePickupFileGone("p5")) },
    all { phase == 2, Stay(MoveRowRewrite("p1", "p2")) },
    all { phase == 2, Stay(MoveRowRewrite("p1", "p3")) },
    all { phase == 2, Stay(MoveRowRewrite("p1", "p4")) },
    all { phase == 2, Stay(MoveRowRewrite("p1", "p5")) },
    all { phase == 2, Stay(MoveRowRewrite("p2", "p1")) },
    all { phase == 2, Stay(MoveRowRewrite("p2", "p3")) },
    all { phase == 2, Stay(MoveRowRewrite("p2", "p4")) },
    all { phase == 2, Stay(MoveRowRewrite("p2", "p5")) },
    all { phase == 2, Stay(MoveRowRewrite("p3", "p1")) },
    all { phase == 2, Stay(MoveRowRewrite("p3", "p2")) },
    all { phase == 2, Stay(MoveRowRewrite("p3", "p4")) },
    all { phase == 2, Stay(MoveRowRewrite("p3", "p5")) },
    all { phase == 2, Stay(MoveRowRewrite("p4", "p1")) },
    all { phase == 2, Stay(MoveRowRewrite("p4", "p2")) },
    all { phase == 2, Stay(MoveRowRewrite("p4", "p3")) },
    all { phase == 2, Stay(MoveRowRewrite("p4", "p5")) },
    all { phase == 2, Stay(MoveRowRewrite("p5", "p1")) },
    all { phase == 2, Stay(MoveRowRewrite("p5", "p2")) },
    all { phase == 2, Stay(MoveRowRewrite("p5", "p3")) },
    all { phase == 2, Stay(MoveRowRewrite("p5", "p4")) },
    all { phase == 2, Stay(RotateFinish("p1")) },
    all { phase == 2, Stay(RotateFinish("p2")) },
    all { phase == 2, Stay(RotateFinish("p3")) },
    all { phase == 2, Stay(RotateFinish("p4")) },
    all { phase == 2, Stay(RotateFinish("p5")) },
    all { phase == 2, Stay(MemberWrite("p1")) },
    all { phase == 2, Stay(MemberWrite("p2")) },
    all { phase == 2, Stay(MemberWrite("p3")) },
    all { phase == 2, Stay(MemberWrite("p4")) },
    all { phase == 2, Stay(MemberWrite("p5")) },
    all { phase == 2, Stay(ApiRead) },
    all { phase == 2, Stay(ReadMissing("p1")) },
    all { phase == 2, Stay(ReadMissing("p2")) },
    all { phase == 2, Stay(ReadMissing("p3")) },
    all { phase == 2, Stay(ReadMissing("p4")) },
    all { phase == 2, Stay(ReadMissing("p5")) },
    all { phase == 2, Skip },
  }

  // Sanity runs are simulation-only. stepCompletion picks any enabled branch
  // per tick, so these runs assert every-reachable-state properties or walk a
  // deterministic path to phase-2 quiescence; they do not claim the invariant
  // over all paths.

  // The completion entry starts in phase 0 on the converged initial state.
  // @assumes ASS-6
  // @scope SCOPE-13
  run completionInitTest = initCompletion
    .then(all { assert(phase == 0 and scannedRoots == Set() and Quiescent and Converged and AspectsAligned), Skip })

  // Free -> draining -> scanning, then the scan's intake and both sweeps
  // drain to quiescence, where the completion invariant holds.
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-13
  run completionScanTest = initCompletion
    .then(BeginDrain)
    .then(all { assert(phase == 1 and Quiescent), Skip })
    .then(BeginScan)
    .then(all { assert(phase == 2 and scannedRoots == Set()), Skip })
    .then(CompleteScan("r1"))
    .then(CompleteScan("r2"))
    .then(all { assert(scannedRoots == ROOTS and sweepPending.get("r1") and sweepPending.get("r2")), Skip })
    .then(Stay(SweepRoot("r1")))
    .then(Stay(SweepRoot("r2")))
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(WorkerSkipUnchanged))
    .then(all { assert(Quiescent and Converged and AspectsAligned and CompletionInvariant), Skip })

  // Sweep batches accumulate across roots. Deleting a path under r1 leaves a
  // stale row; both roots are scanned (each queues its own sweep), and the
  // enumeration of the empty second root must not discard the first root's
  // candidate. Re-check and apply then drain the accumulated batch.
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-8
  // @scope SCOPE-9
  // @scope SCOPE-11
  // @scope SCOPE-13
  run sweepBatchAccumulationTest = initCompletion
    .then(all { assert(phase == 0), Stay(ExternalDelete("p2")) })
    .then(BeginDrain)
    .then(BeginScan)
    .then(CompleteScan("r1"))
    .then(CompleteScan("r2"))
    .then(all { assert(sweepBatch.keys() == Set()), Stay(SweepEnumerate("r1")) })
    .then(all { assert(sweepBatch.keys() == Set("p2")), Stay(SweepEnumerate("r2")) })
    .then(all { assert(sweepBatch.keys() == Set("p2")), Stay(WorkerSkipUnchanged) })
    .then(all { assert(sweepBatch.keys() == Set("p2")), Stay(WorkerSkipUnchanged) })
    .then(all { assert(sweepBatch.keys() == Set("p2")), Stay(WorkerSkipUnchanged) })
    .then(all { assert(sweepBatch.keys() == Set("p2")), Stay(WorkerSkipUnchanged) })
    .then(Stay(SweepRecheck))
    .then(Stay(SweepRemoveBatchApply))
    .then(all { assert(Quiescent and Converged and AspectsAligned), Skip })

  // A crash during the drain loses the queued remove. The phase-2 scan
  // rediscovers the deleted path and the sweep prunes its row, so the machine
  // still reaches phase-2 quiescence with the invariant holding.
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-6
  // @scope SCOPE-13
  run completionCrashScanHealsTest = initCompletion
    .then(Stay(ExternalDelete("p3")))
    .then(Stay(SubmitWatcherRemove("p3")))
    .then(BeginDrain)
    .then(Stay(ProcessCrash))
    .then(all { assert(phase == 1 and intake.size() == 0 and not(Converged)), Skip })
    .then(BeginScan)
    .then(CompleteScan("r1"))
    .then(CompleteScan("r2"))
    .then(Stay(SweepRoot("r1")))
    .then(Stay(SweepRoot("r2")))
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(WorkerSkipUnchanged))
    .then(all { assert(phase == 2 and scannedRoots == ROOTS and Quiescent and Converged and AspectsAligned and CompletionInvariant), Skip })

  // Two arbitrary ticks keep the state inside the declared bounds, the
  // aspect-table key coherent, and the phase machine in range.
  // @assumes ASS-1
  // @scope SCOPE-13
  run completionTwoStepTest = initCompletion
    .then(stepCompletion)
    .then(stepCompletion)
    .then(all { assert(WithinBounds and AssetIdKeyCoherent and phase >= 0 and phase <= 2 and scannedRoots.forall(r => r.in(ROOTS))), Skip })

}
