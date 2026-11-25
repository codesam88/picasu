// Inductive proof entry at the reduced instance: Domains-only TypeOK.
module inductive_proof {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3"),
    HASHES = 0.to(1),
    SMALL = 0.to(2),
    SECOND_ROOT_PATHS = Set("p3"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }],
  ).* from "./inductive-core"

  pure val IDS = 0.to(PATHS.size() + 1)
  pure val G = 7 // working value from the A6 lower bound (see plan §2.3); raise rule applies

  // Possible album itemSize aggregates: sums of up to PATHS.size() member
  // file sizes, each in SMALL (member deltas add/subtract fileSize over at
  // most PATHS.size() members). Built as a set instead of the numeric range
  // `0.to(PATHS.size() * max(SMALL))`: Apalache's constant-range analysis
  // rejects a range whose bound is a fold expression.
  pure val ITEMSIZE_VALUES: Set[int] =
    1.to(PATHS.size()).fold(Set(0), (acc, _) =>
      acc.union(
        acc.fold(Set(), (acc2, s) =>
          SMALL.map(x => s + x).union(acc2)
        )
      )
    )

  // ---------- finite value domains for the record-valued total maps ----------
  // Apalache's inductive rewriter needs each map in setOfMaps(KEYS, VALUES)
  // form with a finite value set. A totality-only conjunct (keys() == KEYS)
  // leaves the value type unbounded and fails the assignment analysis.

  // Every reachable FileState: a present file over HASHES x SMALL, or the
  // absent shape (ExternalDelete/rename zero every field). Writers: init (0s),
  // ExternalModify/ForceModify (ver in HASHES, len in SMALL), ExternalDelete
  // (0s), sidecar create/modify (nonzero ver), sidecar delete (ver 0),
  // rename/copy sites (carry source), RestoreIndexed (from the metadata row),
  // RotateWriteSidecar (a different token, len kept).
  pure val FILESTATES: Set[FileState] =
    Set({ present: false, ver: 0, len: 0 })
      .union(
        HASHES.fold(Set(), (accH, v) =>
          SMALL.map(ln =>
            { present: true, ver: v, len: ln }
          ).union(accH)
        )
      )

  // Reachable media AssetRecord shapes. Writers: init (initialAssetRecord),
  // WorkerApplySuccess (assetId/path/contentHash from the intake item, resets
  // albumId to 0), MoveRowRewrite/ExternalRename (path rewrite), TrashToggle
  // (isTrashed flip). A media record is keyed by assetId (smallest-free id in
  // 1..PATHS.size()+1), path is always in PATHS, contentHash in HASHES, and
  // albumId is always 0.
  pure val MEDIA_ASSETRECORDS: Set[AssetRecord] =
    1.to(PATHS.size() + 1).fold(Set(), (accId, aid) =>
      PATHS.fold(accId, (accP, path) =>
        HASHES.fold(accP, (accH, ch) =>
          Set(true, false).map(tr =>
            { assetId: aid, kind: "media", path: path, contentHash: ch, albumId: 0, isTrashed: tr }
          ).union(accH)
        )
      )
    )

  // The absent sentinel and the fixed album row complete the domain.
  pure val IDRECORDS: Set[AssetRecord] =
    Set(absentAssetRecord, initialAlbumAssetRecord).union(MEDIA_ASSETRECORDS)

  // Album MetadataRecord shapes. Only id 0 carries kind "album"; every write
  // to id 0 preserves ver/fileSize/converged from initialAlbumMetadataRecord
  // (0,0,true) and moves only the aggregates (itemCount 0..PATHS.size(),
  // itemSize in ITEMSIZE_VALUES) and hasShareList.
  // The itemSize lower bound 0 is the coherent range, so a domain state that
  // subtracts below it is a genuine consecution failure.
  pure val ALBUM_METARECORDS: Set[MetadataRecord] =
    0.to(PATHS.size()).fold(Set(), (accC, ic) =>
      accC.union(
        ITEMSIZE_VALUES.fold(Set(), (accS, isz) =>
          Set(true, false).map(hs =>
            { kind: "album", ver: 0, fileSize: 0,
              converged: true, itemCount: ic, itemSize: isz, hasShareList: hs }
          ).union(accS)
        )
      )
    )

  // Media MetadataRecord shapes. Writers: init (0s) and WorkerApplySuccess
  // (ver and fileSize from the intake item); converged is always true, media
  // itemCount/itemSize stay 0, hasShareList stays false.
  pure val MEDIA_METARECORDS: Set[MetadataRecord] =
    HASHES.fold(Set(), (accH, v) =>
      SMALL.map(fs =>
        { kind: "media", ver: v, fileSize: fs,
          converged: true, itemCount: 0, itemSize: 0, hasShareList: false }
      ).union(accH)
    )

  pure val METARECORDS: Set[MetadataRecord] =
    Set(absentMetadataRecord).union(ALBUM_METARECORDS).union(MEDIA_METARECORDS)

  // Finite universe for the intake queue (a set of records, so `.in` is the
  // assignment-analysis form Apalache accepts; a forall/contains conjunct is
  // not recognised as a domain). Reachable shapes only: remove items carry
  // zero version fields, watcher/scan items carry a file snapshot (ver in
  // HASHES, len in SMALL).
  pure val INTAKE_UNIVERSE: Set[IntakeItem] =
    PATHS.fold(Set(), (accP, path) =>
      0.to(G).fold(accP, (accG, gen) =>
        accG.union(
          Set({ path: path, generation: gen, kind: "remove", ver: 0, len: 0 })
            .union(
              Set("watcher", "scan").fold(Set(), (accK, kind) =>
                HASHES.fold(accK, (accV, v) =>
                  SMALL.map(ln =>
                    { path: path, generation: gen, kind: kind, ver: v, len: ln }
                  ).union(accV)
                )
              )
            )
        )
      )
    )

  // Every state var bounded by a constant finite set. Each total map is in
  // setOfMaps form (keys x finite value set) as the inductive rewriter needs.
  val Domains: bool =
    metadataPayload.in(IDS.setOfMaps(METARECORDS)) and
    files.in(PATHS.setOfMaps(FILESTATES)) and
    generations.in(PATHS.setOfMaps(0.to(G))) and
    assetByPath.in(PATHS.setOfMaps(IDS)) and
    assetById.in(IDS.setOfMaps(IDRECORDS)) and
    dupeIndex.in(HASHES.setOfMaps(IDS.powerset())) and
    thumbPresent.in(HASHES.setOfMaps(Set(true, false))) and
    sweepPending.in(ROOTS.setOfMaps(Set(true, false))) and
    sweepBatch.in(PATHS.setOfMaps(Set(-1).union(0.to(G)))) and
    leasedPaths.in(PATHS.powerset()) and
    unprocessable.in(PATHS.powerset()) and
    intake.in(INTAKE_UNIVERSE.powerset()) and
    phase.in(0.to(2)) and
    scannedRoots.in(ROOTS.powerset()) and
    faults.in(0.to(1))

  val TypeOK: bool = Domains

  // ---------- section: round 1 — KeyCoherent ----------
  // Plan §3.2 item 2. The source predicates live in pipeline/merged.qnt in the
  // sparse encoding (`keys()`-based); each is restated here over the total maps
  // with the `kind != "absent"` / `get(p) != 0` sentinel presence tests.

  // Port of merged.qnt:2805 WithinBounds:
  //   files.keys() == PATHS, generations.keys() == PATHS,
  //   sweepPending.keys() == ROOTS, intake.forall(i => i.path.in(PATHS)).
  // All four are absorbed by Domains: `T.in(KEYS.setOfMaps(V))` is the set of
  // total maps over exactly KEYS, and INTAKE_UNIVERSE is built over PATHS, so
  // the intake path bound holds by construction. The source predicate is
  // therefore redundant under the total encoding and contributes no conjunct
  // of its own; the binding stays as the named anchor for that assessment.
  val WithinBoundsCore: bool = true

  // Port of merged.qnt:2815 AssetIdKeyCoherent to the absent sentinel. The
  // sparse form quantifies over `assetById.keys()`, i.e. present rows only;
  // here the absent slots carry assetId 0 (absentAssetRecord), so the sentinel
  // branch is explicit. Not implied by Domains: IDRECORDS admits a media
  // record at any key, so an incoherent key is a domain state. The `Core` suffix
  // avoids the QNT101 clash with the core's sparse definition, which the `.*`
  // import already brings into scope; the sparse form cannot be reused because
  // its quantification over `keys()` (= IDS) would also demand assetId == id
  // on the absent slots.
  val AssetIdKeyCoherentCore: bool =
    IDS.forall(id =>
      assetById.get(id).kind == "absent" or assetById.get(id).assetId == id
    )

  // Partner conjunct on the metadata table (plan §4.1, the kind/key
  // shape-copy hole). Reachable metadata kinds are pinned by key: id 0 carries
  // the album row (kind "album") and media rows live at path-row ids
  // 1..PATHS.size()+1. Every writer preserves the stored kind (AppSidecarEdit,
  // RestoreIndexed, TrashToggle, the album deltas, payloadAfterRetraction) or
  // wipes the row to absent (ApiRemove, RemoveIdOnly, SweepRemoveBatchApply,
  // DbLoss), so no action moves "album" off id 0 or writes "media" onto it.
  // Two implications, not a biconditional: DbLoss leaves id 0 absent, so
  // `id == 0 iff kind == "album"` is false in a reachable state. The conjunct
  // matters for consecution because WorkerApplySuccess copies the stored kind
  // at the resolved id (core:1025): an album kind on a path-row id would be
  // copied onto a media row.
  val MetadataKeyCoherent: bool =
    IDS.forall(id =>
      (metadataPayload.get(id).kind != "album" or id == 0) and
      (id != 0 or metadataPayload.get(id).kind != "media")
    )

  // Present-record budget: album row plus one row per path. The move-over-ghost
  // orphan source is closed (REQ-17 retraction, 96cf8827 + 9a73a5ec) and the
  // mapping-only prune is gone (sweep reconciliation, 24453be3), so no action
  // can create a present record without a path to own it. nextFreeId
  // (core:512) scans the media slots for the lowest absent record.
  // True on reachable states but not inductive from this group alone: a domain
  // state can satisfy the count with an unrowed path and a free media id
  // (WorkerApplySuccess then allocates and exceeds the budget), or with the
  // album absent and every media id occupied (nextFreeId returns
  // PATHS.size()+2 and the allocation writes outside IDS). Both are excluded
  // by the row-ownership conjuncts of later rounds (every present record owned
  // by a rowed path), not by a tighter count.
  val IdBudget: bool =
    IDS.fold(0, (acc, id) =>
      acc + (if (assetById.get(id).kind != "absent") 1 else 0)
    ) <= PATHS.size() + 1

  // Round-1 group; the pre-filter checks this name so a counterexample reports
  // the group, and each conjunct above is also checkable by name.
  val KeyCoherent: bool =
    WithinBoundsCore and AssetIdKeyCoherentCore and MetadataKeyCoherent and IdBudget

  // ---------- section: state-property init ----------
  // initI is the nondet init for the goal-implication check:
  //   quint verify utils/quint/inductive/inductive-proof.qnt --main inductive_proof \
  //     --init initI --step Skip --invariant CompletionInvariant --backend apalache
  // Skip stutters every var, so the check is a state property: every state
  // drawn here must satisfy CompletionInvariant.
  //
  // The draw space is the product of the finite domains above. Quint actions
  // can only bind primed vars by assignment (`x' = e`), so a domain
  // constraint on a not-yet-drawn var is written as a draw from its set.
  // Each draw target is the right-hand set of the matching Domains conjunct;
  // the two must change together when a domain changes. A conjunct that
  // narrows the draw beyond a single var's domain cannot be read back from
  // the primed state inside the action; such a guard must be evaluated on
  // the local draw bindings (or folded into the checked invariant).
  action initI: bool = {
    nondet metadataPayload0 = IDS.setOfMaps(METARECORDS).oneOf()
    nondet files0 = PATHS.setOfMaps(FILESTATES).oneOf()
    nondet generations0 = PATHS.setOfMaps(0.to(G)).oneOf()
    nondet assetByPath0 = PATHS.setOfMaps(IDS).oneOf()
    nondet assetById0 = IDS.setOfMaps(IDRECORDS).oneOf()
    nondet dupeIndex0 = HASHES.setOfMaps(IDS.powerset()).oneOf()
    nondet thumbPresent0 = HASHES.setOfMaps(Set(true, false)).oneOf()
    nondet sweepPending0 = ROOTS.setOfMaps(Set(true, false)).oneOf()
    nondet sweepBatch0 = PATHS.setOfMaps(Set(-1).union(0.to(G))).oneOf()
    nondet leasedPaths0 = PATHS.powerset().oneOf()
    nondet unprocessable0 = PATHS.powerset().oneOf()
    nondet intake0 = INTAKE_UNIVERSE.powerset().oneOf()
    nondet phase0 = 0.to(2).oneOf()
    nondet scannedRoots0 = ROOTS.powerset().oneOf()
    nondet faults0 = 0.to(1).oneOf()
    all {
      metadataPayload' = metadataPayload0,
      files' = files0,
      generations' = generations0,
      assetByPath' = assetByPath0,
      assetById' = assetById0,
      dupeIndex' = dupeIndex0,
      thumbPresent' = thumbPresent0,
      sweepPending' = sweepPending0,
      sweepBatch' = sweepBatch0,
      leasedPaths' = leasedPaths0,
      unprocessable' = unprocessable0,
      intake' = intake0,
      phase' = phase0,
      scannedRoots' = scannedRoots0,
      faults' = faults0,
    }
  }

  // ---------- section: saturated-comparison witness (plan §2.3) ----------
  // Eight submits on one path: the generation token saturates at G on the
  // seventh bump, so the eighth item carries token G while eight logical
  // generations have passed, and the Set collapses it onto the seventh
  // item (same path, token, kind and file snapshot). This is the aliasing
  // the polarity argument (SCOPE-18) must cover: the saturated item reads
  // fresh (== comparison), i.e. extra work that the backstop re-derives,
  // never a fresh item reading older.
  run saturatedComparisonWitness = initCompletion
    .then(Stay(SubmitScan("p1")))
    .then(Stay(SubmitScan("p1")))
    .then(Stay(SubmitScan("p1")))
    .then(Stay(SubmitScan("p1")))
    .then(Stay(SubmitScan("p1")))
    .then(Stay(SubmitScan("p1")))
    .then(Stay(SubmitScan("p1")))
    .then(Stay(SubmitScan("p1")))
    .then(all {
      assert(
        generations.get("p1") == G and
        intake.exists(i => i.path == "p1" and i.generation == G) and
        intake.size() == 7
      ),
      files' = files,
      generations' = generations,
      intake' = intake,
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      unprocessable' = unprocessable,
      sweepBatch' = sweepBatch,
      leasedPaths' = leasedPaths,
      phase' = phase,
      scannedRoots' = scannedRoots,
      faults' = faults,
    })
}
