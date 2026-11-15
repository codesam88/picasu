// Proof-lane step: the environment (FS events, API calls, faults) fires at
// any time and the pipeline is the system, so the goal is as-if-serial
// eventual consistency — a drained state agrees with the FS record (ASS-3/G5).
// Coarse and fine-grained variants coexist; step adds no ordering.
module asset_aspects_step {
  import eventual_consistency_state.* from "./eventual-consistency-state"
  import eventual_consistency_assumptions.* from "./eventual-consistency-assumptions"
  import eventual_consistency_goal.* from "./eventual-consistency-goal"
  import eventual_consistency.* from "./eventual-consistency"
  import asset_aspects.* from "./asset-aspects"

  // Stutter branch: models a tick in which nothing observable changes, so
  // the scheduler never has to force progress.
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
  }

  // Quiescence: no queued intake, no enumerated sweep batch, no root with a
  // sweep owed, and no held path lease. A held lease is the in-flight marker
  // of the move (MoveLeaseAcquire/ApiMoveFsOnly) and rotate
  // (RotateWriteSidecar) windows; the sweep stages are the other in-flight
  // markers.
  // @assumes ASS-6
  // @scope SCOPE-13
  val Settled: bool =
    intake.size() == 0 and
    sweepBatch.keys().size() == 0 and
    ROOTS.forall(r => not(sweepPending.get(r))) and
    leasedPaths.size() == 0

  // As-if-serial goal: once the pipeline drains, the derived state agrees
  // with the FS record. Safety only, no fairness premise (SCOPE-3).
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-13
  val ConvergenceInvariant: bool =
    Settled implies (Converged and AspectsAligned)

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

  // @assumes ASS-6
  // @scope SCOPE-13
  action stepAspects: bool = any {
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

    DirMoveFsOnly,
    ApiMoveFsOnly("p1", "p2"), ApiMoveFsOnly("p1", "p3"), ApiMoveFsOnly("p1", "p4"), ApiMoveFsOnly("p1", "p5"),
    ApiMoveFsOnly("p2", "p1"), ApiMoveFsOnly("p2", "p3"), ApiMoveFsOnly("p2", "p4"), ApiMoveFsOnly("p2", "p5"),
    ApiMoveFsOnly("p3", "p1"), ApiMoveFsOnly("p3", "p2"), ApiMoveFsOnly("p3", "p4"), ApiMoveFsOnly("p3", "p5"),
    ApiMoveFsOnly("p4", "p1"), ApiMoveFsOnly("p4", "p2"), ApiMoveFsOnly("p4", "p3"), ApiMoveFsOnly("p4", "p5"),
    ApiMoveFsOnly("p5", "p1"), ApiMoveFsOnly("p5", "p2"), ApiMoveFsOnly("p5", "p3"), ApiMoveFsOnly("p5", "p4"),
    SweepEnumerate("r1"), SweepEnumerate("r2"),
    SweepRecheck,
    SweepRemoveBatchApply,
    MoveLeaseAcquire("p1", "p2"), MoveLeaseAcquire("p1", "p3"), MoveLeaseAcquire("p1", "p4"), MoveLeaseAcquire("p1", "p5"),
    MoveLeaseAcquire("p2", "p1"), MoveLeaseAcquire("p2", "p3"), MoveLeaseAcquire("p2", "p4"), MoveLeaseAcquire("p2", "p5"),
    MoveLeaseAcquire("p3", "p1"), MoveLeaseAcquire("p3", "p2"), MoveLeaseAcquire("p3", "p4"), MoveLeaseAcquire("p3", "p5"),
    MoveLeaseAcquire("p4", "p1"), MoveLeaseAcquire("p4", "p2"), MoveLeaseAcquire("p4", "p3"), MoveLeaseAcquire("p4", "p5"),
    MoveLeaseAcquire("p5", "p1"), MoveLeaseAcquire("p5", "p2"), MoveLeaseAcquire("p5", "p3"), MoveLeaseAcquire("p5", "p4"),
    MoveRowRewrite("p1", "p2"), MoveRowRewrite("p1", "p3"), MoveRowRewrite("p1", "p4"), MoveRowRewrite("p1", "p5"),
    MoveRowRewrite("p2", "p1"), MoveRowRewrite("p2", "p3"), MoveRowRewrite("p2", "p4"), MoveRowRewrite("p2", "p5"),
    MoveRowRewrite("p3", "p1"), MoveRowRewrite("p3", "p2"), MoveRowRewrite("p3", "p4"), MoveRowRewrite("p3", "p5"),
    MoveRowRewrite("p4", "p1"), MoveRowRewrite("p4", "p2"), MoveRowRewrite("p4", "p3"), MoveRowRewrite("p4", "p5"),
    MoveRowRewrite("p5", "p1"), MoveRowRewrite("p5", "p2"), MoveRowRewrite("p5", "p3"), MoveRowRewrite("p5", "p4"),
    SidecarEditPartial("p1"), SidecarEditPartial("p2"), SidecarEditPartial("p3"), SidecarEditPartial("p4"), SidecarEditPartial("p5"),
    DroppedSidecarModify("p1"), DroppedSidecarModify("p2"), DroppedSidecarModify("p3"), DroppedSidecarModify("p4"), DroppedSidecarModify("p5"),
    UploadStoredOnly("p1"), UploadStoredOnly("p2"), UploadStoredOnly("p3"), UploadStoredOnly("p4"), UploadStoredOnly("p5"),
    UploadIndexed("p1"), UploadIndexed("p2"), UploadIndexed("p3"), UploadIndexed("p4"), UploadIndexed("p5"),
    IntakePickupFileGone("p1"), IntakePickupFileGone("p2"), IntakePickupFileGone("p3"), IntakePickupFileGone("p4"), IntakePickupFileGone("p5"),
    RemoveIdOnly("p1"), RemoveIdOnly("p2"), RemoveIdOnly("p3"), RemoveIdOnly("p4"), RemoveIdOnly("p5"),
    MemberWrite("p1"), MemberWrite("p2"), MemberWrite("p3"), MemberWrite("p4"), MemberWrite("p5"),
    AlbumShareEdit,
    TrashToggle("p1"), TrashToggle("p2"), TrashToggle("p3"), TrashToggle("p4"), TrashToggle("p5"),
    Skip,
  }

  // Sanity runs are simulation-only. stepAspects picks any enabled branch
  // per tick (including environment events), so only properties of every
  // reachable state are asserted here; convergence is asserted at the
  // settled initial state, not after arbitrary steps.
  //
  // The composed runs surface a totality gap: arbitrary interleavings reach
  // states the base and aspect action bodies do not expect (a path-map entry
  // whose record was removed by RemoveIdOnly, or no album record after
  // DbLoss), where an unguarded record read raises a runtime error instead
  // of disabling the branch.

  // The settled premise is satisfiable and the invariant holds on it.
  // @assumes ASS-6
  // @scope SCOPE-13
  run stepAspectsSettledInitTest = init
    .then(all { assert(Settled and ConvergenceInvariant and Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // Two arbitrary ticks keep the state inside the declared bounds and the
  // aspect-table key coherent with the record.
  // @assumes ASS-1
  // @scope SCOPE-13
  run stepAspectsTwoStepTest = init
    .then(stepAspects)
    .then(stepAspects)
    .then(all { assert(WithinBounds and AssetIdKeyCoherent), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // Four arbitrary ticks: same claims, so longer compositions do not widen
  // the state domain or break identity keying.
  // @assumes ASS-1
  // @scope SCOPE-13
  run stepAspectsFourStepTest = init
    .then(stepAspects)
    .then(stepAspects)
    .then(stepAspects)
    .then(stepAspects)
    .then(all { assert(WithinBounds and AssetIdKeyCoherent), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
}
