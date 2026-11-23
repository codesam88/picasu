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

  // Every reachable FileState: a present file over HASHES x SMALL^3, or the
  // absent shape (ExternalDelete/rename zero every field). Writers: init (0s),
  // ExternalModify/ForceModify (HASHES x SMALL), ExternalDelete (0s),
  // rename/copy sites (carry source), AppSidecarEdit (sidecar 1),
  // RotateWriteSidecar (sidecar flips 1<->2), RestoreIndexed (from the
  // metadata row).
  pure val FILESTATES: Set[FileState] =
    Set({ present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 })
      .union(
        HASHES.fold(Set(), (accH, ah) =>
          SMALL.fold(accH, (accM, mt) =>
            SMALL.fold(accM, (accL, ln) =>
              accL.union(
                SMALL.map(sc =>
                  { present: true, assetHash: ah, mtime: mt, len: ln, sidecar: sc }
                )
              )
            )
          )
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
  // to id 0 preserves assetHash/modified/fileSize/sidecar/converged from
  // initialAlbumMetadataRecord (0,0,0,0,true) and moves only the aggregates
  // (itemCount 0..PATHS.size(), itemSize in ITEMSIZE_VALUES) and hasShareList.
  // The itemSize lower bound 0 is the coherent range, so a domain state that
  // subtracts below it is a genuine consecution failure.
  pure val ALBUM_METARECORDS: Set[MetadataRecord] =
    0.to(PATHS.size()).fold(Set(), (accC, ic) =>
      accC.union(
        ITEMSIZE_VALUES.fold(Set(), (accS, isz) =>
          Set(true, false).map(hs =>
            { kind: "album", assetHash: 0, modified: 0, fileSize: 0, sidecar: 0,
              converged: true, itemCount: ic, itemSize: isz, hasShareList: hs }
          ).union(accS)
        )
      )
    )

  // Media MetadataRecord shapes. Writers: init (0s) and WorkerApplySuccess
  // (assetHash/modified/fileSize/sidecar from the intake item); converged is
  // always true, media itemCount/itemSize stay 0, hasShareList stays false.
  pure val MEDIA_METARECORDS: Set[MetadataRecord] =
    HASHES.fold(Set(), (accH, ah) =>
      SMALL.fold(accH, (accM, md) =>
        SMALL.fold(accM, (accL, fs) =>
          SMALL.map(sc =>
            { kind: "media", assetHash: ah, modified: md, fileSize: fs, sidecar: sc,
              converged: true, itemCount: 0, itemSize: 0, hasShareList: false }
          ).union(accL)
        )
      )
    )

  pure val METARECORDS: Set[MetadataRecord] =
    Set(absentMetadataRecord).union(ALBUM_METARECORDS).union(MEDIA_METARECORDS)

  // Finite universe for the intake queue (a set of records, so `.in` is the
  // assignment-analysis form Apalache accepts; a forall/contains conjunct is
  // not recognised as a domain). Reachable shapes only: remove items carry
  // zero content fields, watcher/scan items carry a file snapshot
  // (mtime/len/sidecar in SMALL, assetHash in HASHES).
  pure val INTAKE_UNIVERSE: Set[IntakeItem] =
    PATHS.fold(Set(), (accP, path) =>
      0.to(G).fold(accP, (accG, gen) =>
        accG.union(
          Set({ path: path, generation: gen, kind: "remove", mtime: 0, len: 0, sidecar: 0, assetHash: 0 })
            .union(
              Set("watcher", "scan").fold(Set(), (accK, kind) =>
                SMALL.fold(accK, (accM, mt) =>
                  SMALL.fold(accM, (accL, ln) =>
                    SMALL.fold(accL, (accS, sc) =>
                      HASHES.map(ah =>
                        { path: path, generation: gen, kind: kind, mtime: mt, len: ln, sidecar: sc, assetHash: ah }
                      ).union(accS)
                    )
                  )
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
}
