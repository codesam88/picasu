// Canonical pipeline model: the index pipeline protocol, its shared
// state, assumptions, convergence goal, asset-aspect slice, and the
// three-phase completion machine in one module.
//
// The sections below were merged from the per-subsystem modules and
// keep their headers, comments and annotation tags: shared state and
// bounds, assumptions, the convergence goal, the protocol events, the
// asset-aspect slice, and the completion machine. All declarations
// share one namespace, so entry points are thin modules that import
// this one (entry-canonical.qnt for the canonical bounds).
//
// The steps coverage gate sees one module: `step` and `stepCompletion`
// carry `@step-universe` tags naming the actions each must cover;
// their branch lists overlap, their universes differ.
module pipeline_merged {
  // ---------- section: shared state, bounds, initial state ----------
  // Index pipeline shared state: the filesystem projection, the
  // per-path submission versions, the intake queue, the per-asset aspect
  // tables (identity, path map, dupe index, metadata payload, thumbnail
  // presence, album pending), and the initial state every run starts from.
  // The assumption predicates and the protocol actions below build on it.

  // @scope SCOPE-2: finite bounds; every run stays inside them.
  const PATHS: Set[str]
  // @scope SCOPE-2
  const HASHES: Set[int]
  // @scope SCOPE-2
  const SMALL: Set[int]

  // @scope SCOPE-2
  pure val ROOTS = Set("r1", "r2")
  // Instance data: which paths belong to the second root (r2). Not derivable
  // from unordered sets; override this val for a reduced instance.
  // @scope SCOPE-2
  const SECOND_ROOT_PATHS: Set[str]
  // @scope SCOPE-2
  pure def rootOf(p: str): str = if (p.in(SECOND_ROOT_PATHS)) "r2" else "r1"

  type FileState = {
    present: bool,
    assetHash: int,
    mtime: int,
    len: int,
    sidecar: int
  }

  // AssetRecord mirrors backend/src/model/asset.rs AssetRecord identity
  // fields. Only the fields the operations need are modeled: path,
  // contentHash, albumId, kind (media|album), isTrashed. ext and scanTime
  // are dropped: no action in the model reads or writes them, and they
  // carry no convergence or hazard semantics (scan_time is an ordering
  // tiebreak, excluded from Converged per the convergence goal section).
  type AssetRecord = {
    assetId: int,
    kind: str,
    path: str,
    contentHash: int,
    albumId: int,
    isTrashed: bool
  }

  // MetadataRecord mirrors backend/src/model/metadata_record.rs
  // MetadataRecord. Only a coarse subset is modeled: for media records the
  // content-snapshot fields (assetHash, modified, fileSize, sidecar,
  // converged) that the convergence goal compares against the filesystem;
  // for album records itemCount, itemSize, hasShareList. Full metadata
  // fields (tags, description, ...) are out of scope (no operation in this
  // model mutates them).
  type MetadataRecord = {
    kind: str,
    assetHash: int,
    modified: int,
    fileSize: int,
    sidecar: int,
    converged: bool,
    itemCount: int,
    itemSize: int,
    hasShareList: bool
  }

  type IntakeItem = {
    path: str,
    generation: int,
    kind: str,
    mtime: int,
    len: int,
    sidecar: int,
    assetHash: int
  }

  // files maps to the filesystem under the data roots (media bytes + sidecar bytes).
  var files: str -> FileState
  // generations maps to tasks/path_epoch.rs GENERATIONS via next_generation/current_generation.
  var generations: str -> int
  // intake maps to the IntakeItem set in tasks/queue/intake.rs.
  var intake: Set[IntakeItem]
  // assetById maps to storage/asset_store.rs ASSET_BY_ID (aspect (d) identity).
  var assetById: int -> AssetRecord
  // assetByPath maps to storage/asset_store.rs ASSET_BY_PATH (aspect (e)).
  var assetByPath: str -> int
  // dupeIndex maps to storage/asset_store.rs DUPE_INDEX (aspect (g), hash-keyed).
  var dupeIndex: int -> Set[int]
  // metadataPayload maps to storage/asset_store.rs METADATA_TABLE (aspects (f)+(i)).
  var metadataPayload: int -> MetadataRecord
  // thumbPresent maps to the thumbnail store paths (aspect (c), content-hash keyed).
  var thumbPresent: int -> bool
      // sweepPending maps to the per-root sweep pending flag set by ScanRoot/the scan job in tasks/actor/album_index.rs (sweep_stale_asset_paths runs after the walk).
  var sweepPending: str -> bool
  // sweepBatch maps to sweep_stale_asset_paths survivors -> WriteOp::SweepRemoveBatch.
  var sweepBatch: str -> int
  // leasedPaths maps to EXACT_LEASES/SUBTREE_LEASES in tasks/path_epoch.rs.
  var leasedPaths: Set[str]
  // nextAssetId models asset-id allocation via generate_random_hash in model/asset.rs (AssetRecord::new_media/new_album) and flush_tree.rs; code uses random hashes, not a counter.
  var nextAssetId: int
  // unprocessable is model-only: no persistent code counterpart; ingest failures are logged via note_file_failed/handle_error, not stored as a set.
  var unprocessable: Set[str]

  // @scope SCOPE-1
  pure def initialId(p: str): int =
    if (p == "p1") 1 else (if (p == "p2") 2 else (if (p == "p3") 3 else (if (p == "p4") 4 else 5)))

  // @scope SCOPE-1
  pure def initialFile(p: str): FileState = {
    present: true,
    assetHash: 0,
    mtime: 0,
    len: 0,
    sidecar: 0
  }

  // @scope SCOPE-1
  pure def initialAssetRecord(p: str): AssetRecord = {
    assetId: initialId(p),
    kind: "media",
    path: p,
    contentHash: 0,
    albumId: 0,
    isTrashed: false
  }

  // @scope SCOPE-1
  pure def initialMetadataRecord(p: str): MetadataRecord = {
    kind: "media",
    assetHash: 0,
    modified: 0,
    fileSize: 0,
    sidecar: 0,
    converged: true,
    itemCount: 0,
    itemSize: 0,
    hasShareList: false
  }

  // @scope SCOPE-1
  pure def initialAlbumAssetRecord: AssetRecord = {
    assetId: 0,
    kind: "album",
    path: "",
    contentHash: -1,
    albumId: -1,
    isTrashed: false
  }

  // @scope SCOPE-1
  pure def initialAlbumMetadataRecord: MetadataRecord = {
    kind: "album",
    assetHash: 0,
    modified: 0,
    fileSize: 0,
    sidecar: 0,
    converged: true,
    itemCount: PATHS.size(),
    itemSize: 0,
    hasShareList: false
  }

  // @scope SCOPE-1: initial converged state; runs diverge from here.
  action init: bool = all {
    files' = PATHS.mapBy(p => initialFile(p)),
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
  }
  // ---------- section: assumptions ----------
  // Index pipeline assumptions: each assumption beside its formal
  // counterpart. The prose states what the model takes as given; the
  // formal element — or an explicit pointer to where it lives — shows
  // how the model relies on it. An assumption with no formal element
  // of its own says so instead of implying coverage.
  // utils/quint/pipeline/assumptions.json keeps the index of IDs with owner and status;
  // model refs come from the generated traceability map. Purpose, scope,
  // and writing rules live in utils/quint/README.md.

  // ASS-1: The asset is its path plus its content.
  //
  // The asset file path is the asset's unique id (assetID). Each asset
  // has actual file content (image, video), optional embedded metadata,
  // and an optional XMP sidecar located at the same file path with xmp
  // extension. The combination of these makes the asset. Two paths never
  // share an identity, even with identical content. Path identity is
  // exact: no case-folding or Unicode normalization; identity is the
  // exact path plus the content.
  //
  // Formalized by construction: files, rows, and generations are keyed
  // by path (see the shared state section). Asserted per distinct
  // identity by run copyDistinctIdsTest. No predicate here: identity IS
  // the keying.

  // ASS-3: The filesystem is the source of truth.
  //
  // The index (rows plus derived artifacts) is a rebuildable cache over
  // the watched trees. On any disagreement between the index and the
  // filesystem, the filesystem wins and the index converges to it. Rows
  // converge to the observed state; metadata that existed only in a
  // removed sidecar is lost.
  //
  // Formalized as direction: Converged constrains rows to files, never
  // the reverse (see the convergence goal section); SweepStalePaths
  // keeps only present files; ExternalSidecarDelete zeroes the sidecar
  // (see the protocol section); run sidecarDeleteConvergesTest.

  // ASS-4: The derived store is pipeline-private.
  //
  // Derived artifacts are produced by the pipeline and read during
  // serving. A missing artifact is rebuilt by processing: the modeled
  // skip action requires the thumbnail present, and only reprocessing
  // restores it. Walk roots are disjoint from the
  // derived store, so pipeline writes cannot resubmit themselves;
  // see the intake header and WorkerApplySuccess in the
  // protocol section; derived bytes (thumbnails) are written by
  // workflow::process_file (backend/src/process/index.rs + thumbnail.rs)
  // and rotate_image.rs (generate_thumbnail_for_image), driven by the
  // intake worker.

  // ASS-5: Content hash is the authority; mtime and size are hints.
  //
  // Equal modification time never implies equal bytes: synced and
  // copied files routinely preserve times. Identity comparisons rest on
  // the content hash; time and size only shortcut the comparison.
  //
  // Snapshot match: file still has the bytes+identity the intake item
  // recorded. Maps to is_stale's comparison (inverted).
  // @assumes ASS-5
  pure def snapshotMatches(f: FileState, ii: IntakeItem): bool = {
    f.mtime == ii.mtime and f.len == ii.len and
    f.sidecar == ii.sidecar and f.assetHash == ii.assetHash
  }

  // ASS-6: Correctness is quiescent-state.
  //
  // Invariants hold after submission plus processing converge, not in
  // between. Liveness — every change eventually submitted, every item
  // eventually processed — is assumed under fairness and is not checked
  // by the spec. The two safety properties below are asserted only at
  // settled states, never mid-flight.

  // No lost upload: every queued item is current, superseded by a newer
  // queued item, already reflected in a matching row, or moot because its
  // path was fully deleted (no row left). Unprocessable paths are excluded
  // (logged, never indexed).
  // @assumes ASS-6
  val NoLostUpload = intake.forall(ii =>
    ii.path.in(unprocessable) or (
    (ii.generation == generations.get(ii.path)) or
    (intake.exists(jj => (jj.path == ii.path) and (jj.generation > ii.generation))) or
    ((files.get(ii.path).present) and (assetByPath.keys().contains(ii.path)) and
     (metadataPayload.keys().contains(assetByPath.get(ii.path)) and
     (metadataPayload.get(assetByPath.get(ii.path)).assetHash == files.get(ii.path).assetHash) and
     (metadataPayload.get(assetByPath.get(ii.path)).modified == files.get(ii.path).mtime) and
     (metadataPayload.get(assetByPath.get(ii.path)).fileSize == files.get(ii.path).len) and
     (metadataPayload.get(assetByPath.get(ii.path)).sidecar == files.get(ii.path).sidecar))) or
    ((not(files.get(ii.path).present)) and (not(assetByPath.keys().contains(ii.path))))
    )
  )

  // No permanent staleness: a present file either matches its row, has work
  // queued; an absent file either has no row, has removal queued, or awaits
  // the root's sweep. Unprocessable paths are excluded (logged, never indexed).
  // @assumes ASS-6
  val NoPermanentStale = PATHS.forall(p =>
    p.in(unprocessable) or
    if (files.get(p).present) {
      if (assetByPath.keys().contains(p)) {
        (metadataPayload.keys().contains(assetByPath.get(p)) and metadataPayload.get(assetByPath.get(p)).assetHash == files.get(p).assetHash and metadataPayload.get(assetByPath.get(p)).modified == files.get(p).mtime and metadataPayload.get(assetByPath.get(p)).fileSize == files.get(p).len and metadataPayload.get(assetByPath.get(p)).sidecar == files.get(p).sidecar) or
        intake.exists(i => i.path == p)
      } else {
        intake.exists(i => i.path == p)
      }
    } else {
      (not(assetByPath.keys().contains(p))) or
      intake.exists(i => i.path == p and i.kind == "remove") or
      sweepPending.get(rootOf(p))
    }
  )
  // ---------- section: convergence goal ----------
  // The index pipeline goal: eventual convergence of the redb
  // projection to the filesystem. This is the subsystem-specific part
  // of the spec — the shared state and assumption predicates are
  // generic and reusable across models. The protocol runs below assert
  // it per diverged shape.

  // The convergence goal: every existing asset path has exactly one
  // correct row (across the split aspect tables), and every row's asset
  // exists at its path — no missing, no ghosts, nothing left queued or
  // pending. Excluded by design: scan_time (ordering tiebreak, not
  // correctness) and assetId values (move preservation is a move
  // property, not a convergence property).
  // @assumes ASS-3
  // @assumes ASS-6
  val Converged = PATHS.forall(p =>
    p.in(unprocessable) or (
    (files.get(p).present ==
      assetByPath.keys().contains(p))
    and (not(files.get(p).present) or (
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
         thumbPresent.get(assetById.get(assetByPath.get(p)).contentHash)))
    ))
    )
  ) and intake.size() == 0
  // ---------- section: protocol events, actions, runs ----------
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
  // watcher is enabled and the user-enabled periodic backstop (ASS-7).
  // The model checks bounded convergence under those triggers, not that they run.
  //
  // Every action/val/def/run carries @assumes/@scope/@exempt, or an
  // @requires tag resolving against utils/quint/pipeline/requirements.json.

  // @scope SCOPE-7
  // Instance data: which subtrees the dir-move action moves; override for a reduced instance.
  const SUBTREE_PAIRS: List[{ src: str, dst: str }]

  // Derived view of the old `rows` map, reconstructed from the split
  // aspect tables. Kept as a read-only projection so existing run
  // assertions and guards that reference `rows` keep their meaning.
  // The projection is total: guards evaluate `rows.keys()` in torn states,
  // where a path entry outlives its record (RemoveIdOnly drops ASSET_BY_ID
  // and METADATA_TABLE before ASSET_BY_PATH, N5), so the record reads fall
  // back to the no-metadata values instead of raising on a missing key.
  // @exempt observer
  val rows = assetByPath.keys().mapBy(p =>
    {
      assetId: assetByPath.get(p),
      assetHash: if (metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).assetHash } else { if (assetById.keys().contains(assetByPath.get(p))) { assetById.get(assetByPath.get(p)).contentHash } else { 0 } },
      modified: if (metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).modified } else { 0 },
      fileSize: if (metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).fileSize } else { 0 },
      sidecar: if (metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).sidecar } else { 0 },
      converged: if (metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).converged } else { false },
      thumb: (assetById.keys().contains(assetByPath.get(p)) and assetById.get(assetByPath.get(p)).contentHash >= 0 and thumbPresent.keys().contains(assetById.get(assetByPath.get(p)).contentHash) and thumbPresent.get(assetById.get(assetByPath.get(p)).contentHash))
    }
  )

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
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @exempt environment
  action ExternalDelete(p: str): bool = all {
    p.in(PATHS),
    files' = files.set(p, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }),
    generations' = generations,
    intake' = intake,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // Prescribed handling: old-path remove plus new-path create, submitted in
  // the same watcher reaction.
  // @exempt environment
  // @assumes ASS-6
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
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }

  // @assumes ASS-1
  // @scope SCOPE-5
  // @scope SCOPE-7
  action ExternalRenameDir: bool = all {
    SUBTREE_PAIRS.foldl(true, (acc, pr) => acc and files.get(pr.src).present and not(files.get(pr.dst).present)),
    // collect_dir_move_plans re-reads each descendant record and rejects the
    // whole move when a path row has no record (N5 residue), so this branch is
    // not enabled in that state.
    SUBTREE_PAIRS.foldl(true, (acc, pr) => acc and (if (assetByPath.keys().contains(pr.src)) { assetById.keys().contains(assetByPath.get(pr.src)) } else { true })),
    files' = SUBTREE_PAIRS.foldl(files, (acc, pr) =>
      acc.set(pr.src, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 })
         .set(pr.dst, { present: true, assetHash: files.get(pr.src).assetHash, mtime: files.get(pr.src).mtime, len: files.get(pr.src).len, sidecar: files.get(pr.src).sidecar })
    ),
    generations' = generations,
    intake' = intake,
    assetById' = SUBTREE_PAIRS.foldl(assetById, (acc, pr) =>
      if (assetByPath.keys().contains(pr.src)) {
        acc.put(assetByPath.get(pr.src), { assetId: assetByPath.get(pr.src), kind: acc.get(assetByPath.get(pr.src)).kind, path: pr.dst, contentHash: acc.get(assetByPath.get(pr.src)).contentHash, albumId: acc.get(assetByPath.get(pr.src)).albumId, isTrashed: acc.get(assetByPath.get(pr.src)).isTrashed })
      } else {
        acc
      }
    ),
    assetByPath' = SUBTREE_PAIRS.foldl(assetByPath, (acc, pr) =>
      if (acc.keys().contains(pr.src)) {
        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k)).put(pr.dst, acc.get(pr.src))
      } else {
        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k))
      }
    ),
    dupeIndex' = dupeIndex,
    metadataPayload' = metadataPayload,
    thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @exempt environment
  // @assumes ASS-6
  action ExternalSidecarCreate(p: str): bool = {
    nondet sc = SMALL.exclude(Set(0)).oneOf()
    all {
      p.in(PATHS),
      files.get(p).present,
      files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: sc }),
      generations' = generations.set(p, generations.get(p) + 1),
      intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: sc, assetHash: files.get(p).assetHash })),
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @exempt environment
  // @assumes ASS-6
  action ExternalSidecarModify(p: str): bool = {
    nondet sc = SMALL.exclude(Set(0)).oneOf()
    all {
      p.in(PATHS),
      files.get(p).present,
      files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: sc }),
      generations' = generations.set(p, generations.get(p) + 1),
      intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: sc, assetHash: files.get(p).assetHash })),
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @exempt environment
  // @assumes ASS-3
  // @assumes ASS-6
  action ExternalSidecarDelete(p: str): bool = all {
      p.in(PATHS),
      files.get(p).present,
      files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: 0 }),
      generations' = generations.set(p, generations.get(p) + 1),
      intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: 0, assetHash: files.get(p).assetHash })),
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }

  // @exempt environment
  action ForceModify(p: str): bool = {
    nondet cv = HASHES.exclude(Set(files.get(p).assetHash)).oneOf()
    nondet mt = SMALL.exclude(Set(files.get(p).mtime)).oneOf()
    nondet ln = SMALL.exclude(Set(files.get(p).len)).oneOf()
    all {
      p.in(PATHS),
      files.get(p).present,
      files' = files.set(p, { present: true, assetHash: cv, mtime: mt, len: ln, sidecar: files.get(p).sidecar }),
    generations' = generations,
    intake' = intake,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @exempt environment
  action MarkUnprocessable(p: str): bool = all {
    p.in(PATHS),
    unprocessable' = unprocessable.union(Set(p)),
    files' = files,
    generations' = generations,
    intake' = intake,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    nextAssetId' = nextAssetId,
  }

  // @assumes ASS-5
  action RestoreIndexed(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    files' = files.set(p, { present: true, assetHash: rows.get(p).assetHash, mtime: rows.get(p).modified, len: rows.get(p).fileSize, sidecar: rows.get(p).sidecar }),
    generations' = generations,
    intake' = intake,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @exempt environment
  // @scope SCOPE-6
  action ProcessCrash: bool = all {
    intake' = Set(),
    sweepBatch' = Map(),
    leasedPaths' = Set(),
    files' = files,
    generations' = generations,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
  }

  // @exempt environment
  // @scope SCOPE-6
  action LoseThumbs: bool = all {
    thumbPresent' = HASHES.mapBy(h => false),
    assetById' = assetById,
    assetByPath' = assetByPath,
    dupeIndex' = dupeIndex,
    metadataPayload' = metadataPayload,
    files' = files,
    generations' = generations,
    intake' = intake,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @exempt environment
  // @scope SCOPE-6
  action DbLoss: bool = all {
    assetById' = Map(),
    assetByPath' = Map(),
    dupeIndex' = HASHES.mapBy(h => Set()),
    metadataPayload' = Map(),
    thumbPresent' = HASHES.mapBy(h => false),

    generations' = PATHS.mapBy(_ => 0),
    nextAssetId' = 1,
    intake' = Set(),
    sweepPending' = ROOTS.mapBy(_ => false),
    sweepBatch' = Map(),
    leasedPaths' = Set(),
    files' = files,
    unprocessable' = unprocessable,
  }

  // ---------- API / direct writes ----------

  // @scope SCOPE-5
  action AppSidecarEdit(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    // The edit path composes the current record from ASSET_BY_ID and
    // METADATA_TABLE and skips an asset that does not compose, so a torn path
    // row (N5) cannot apply an edit.
    assetById.keys().contains(assetByPath.get(p)),
    metadataPayload.keys().contains(assetByPath.get(p)),
    files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: 1 }),
    generations' = generations.set(p, generations.get(p) + 1),
    metadataPayload' = metadataPayload.put(assetByPath.get(p), { kind: metadataPayload.get(assetByPath.get(p)).kind, assetHash: files.get(p).assetHash, modified: files.get(p).mtime, fileSize: files.get(p).len, sidecar: 1, converged: true, itemCount: metadataPayload.get(assetByPath.get(p)).itemCount, itemSize: metadataPayload.get(assetByPath.get(p)).itemSize, hasShareList: metadataPayload.get(assetByPath.get(p)).hasShareList }),
    assetById' = assetById,
    assetByPath' = assetByPath,
    dupeIndex' = dupeIndex,
    thumbPresent' = thumbPresent,
    intake' = intake,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @operation OP-6
  // @scope SCOPE-5
  action RotateWriteSidecar(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    files' = files.setBy(p, f => { present: f.present, assetHash: f.assetHash, mtime: f.mtime, len: f.len, sidecar: if (files.get(p).sidecar == 1) 2 else 1 }),
    generations' = generations.set(p, generations.get(p) + 1),
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    intake' = intake,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths.union(Set(p)),
  }

  // @operation OP-6
  // @scope SCOPE-5
  action RotateFinish(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    files.get(p).sidecar != rows.get(p).sidecar,
    // REQ-11: rotate persists the sidecar, then the thumbnail, then the DB
    // record, holding one path lease for the request's write phase. A crash
    // before finish drops the lease and the next scan heals. Finish therefore
    // requires the lease it releases; the sidecar marker above is also
    // produced by environment sidecar events, so it cannot stand in for the
    // lease.
    leasedPaths.contains(p),
    // rotate_image resolves the composed record and answers 404 when either
    // table row is gone, so a torn path row (N5) cannot finish a rotation.
    assetById.keys().contains(assetByPath.get(p)),
    metadataPayload.keys().contains(assetByPath.get(p)),
    metadataPayload' = metadataPayload.put(assetByPath.get(p), { kind: metadataPayload.get(assetByPath.get(p)).kind, assetHash: files.get(p).assetHash, modified: files.get(p).mtime, fileSize: files.get(p).len, sidecar: files.get(p).sidecar, converged: true, itemCount: metadataPayload.get(assetByPath.get(p)).itemCount, itemSize: metadataPayload.get(assetByPath.get(p)).itemSize, hasShareList: metadataPayload.get(assetByPath.get(p)).hasShareList }),
    assetById' = assetById,
    assetByPath' = assetByPath,
    dupeIndex' = dupeIndex,
    thumbPresent' = thumbPresent,
    files' = files,
    generations' = generations,
    intake' = intake,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths.exclude(Set(p)),
  }

  // @assumes ASS-6
  action ApiUpload(p: str): bool = {
    nondet cv = HASHES.oneOf()
    nondet mt = SMALL.oneOf()
    nondet ln = SMALL.oneOf()
    nondet sc = SMALL.oneOf()
    all {
      p.in(PATHS),
      files' = files.set(p, { present: true, assetHash: cv, mtime: mt, len: ln, sidecar: sc }),
      generations' = generations.set(p, generations.get(p) + 1),
      intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: mt, len: ln, sidecar: sc, assetHash: cv })),
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @scope SCOPE-5
  action ApiRemove(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    rows.keys().contains(p),
    // process_deletes resolves the record by asset id and answers 404 when it
    // is gone, so a path row without its record (N5) cannot be deleted.
    assetById.keys().contains(assetByPath.get(p)),
    files' = files.set(p, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }),
    generations' = generations,
    intake' = intake,
    assetById' = assetById.keys().exclude(Set(assetByPath.get(p))).mapBy(k => assetById.get(k)),
    assetByPath' = assetByPath.keys().exclude(Set(p)).mapBy(k => assetByPath.get(k)),
    metadataPayload' = metadataPayload.keys().exclude(Set(assetByPath.get(p))).mapBy(k => metadataPayload.get(k)),
    dupeIndex' = if (assetById.get(assetByPath.get(p)).contentHash >= 0 and dupeIndex.keys().contains(assetById.get(assetByPath.get(p)).contentHash)) { dupeIndex.put(assetById.get(assetByPath.get(p)).contentHash, dupeIndex.get(assetById.get(assetByPath.get(p)).contentHash).exclude(Set(assetByPath.get(p)))) } else { dupeIndex },
    thumbPresent' = if (assetById.get(assetByPath.get(p)).contentHash >= 0 and dupeIndex.keys().contains(assetById.get(assetByPath.get(p)).contentHash)) { thumbPresent.put(assetById.get(assetByPath.get(p)).contentHash, dupeIndex.get(assetById.get(assetByPath.get(p)).contentHash).exclude(Set(assetByPath.get(p))).size() > 0) } else { thumbPresent },
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @assumes ASS-1
  // @scope SCOPE-5
  action ApiMove(p: str, q: str): bool = all {
    p.in(PATHS),
    q.in(PATHS),
    not(p == q),
    files.get(p).present,
    not(files.get(q).present),
    rows.keys().contains(p),
    // move_asset_into_album resolves the record by path and re-reads it under
    // the lease ("Asset not found" when it is gone), so a torn path row (N5)
    // cannot move.
    assetById.keys().contains(assetByPath.get(p)),
    files' = files.set(p, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 }).set(q, { present: true, assetHash: files.get(p).assetHash, mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar }),
    generations' = generations,
    intake' = intake,
    assetById' = assetById.put(assetByPath.get(p), { assetId: assetByPath.get(p), kind: assetById.get(assetByPath.get(p)).kind, path: q, contentHash: assetById.get(assetByPath.get(p)).contentHash, albumId: assetById.get(assetByPath.get(p)).albumId, isTrashed: assetById.get(assetByPath.get(p)).isTrashed }),
    assetByPath' = assetByPath.keys().exclude(Set(p)).mapBy(k => assetByPath.get(k)).put(q, assetByPath.get(p)),
    metadataPayload' = metadataPayload,
    dupeIndex' = dupeIndex,
    thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @assumes ASS-1
  // @scope SCOPE-5
  // @scope SCOPE-7
  action ApiAssignDirMove: bool = all {
    SUBTREE_PAIRS.foldl(true, (acc, pr) => acc and files.get(pr.src).present and not(files.get(pr.dst).present)),
    // Same re-resolve as ExternalRenameDir: collect_dir_move_plans rejects a
    // descendant path row whose record is gone.
    SUBTREE_PAIRS.foldl(true, (acc, pr) => acc and (if (assetByPath.keys().contains(pr.src)) { assetById.keys().contains(assetByPath.get(pr.src)) } else { true })),
    files' = SUBTREE_PAIRS.foldl(files, (acc, pr) =>
      acc.set(pr.src, { present: false, assetHash: 0, mtime: 0, len: 0, sidecar: 0 })
         .set(pr.dst, { present: true, assetHash: files.get(pr.src).assetHash, mtime: files.get(pr.src).mtime, len: files.get(pr.src).len, sidecar: files.get(pr.src).sidecar })
    ),
    generations' = generations,
    intake' = intake,
    assetById' = SUBTREE_PAIRS.foldl(assetById, (acc, pr) =>
      if (assetByPath.keys().contains(pr.src)) {
        acc.put(assetByPath.get(pr.src), { assetId: assetByPath.get(pr.src), kind: acc.get(assetByPath.get(pr.src)).kind, path: pr.dst, contentHash: acc.get(assetByPath.get(pr.src)).contentHash, albumId: acc.get(assetByPath.get(pr.src)).albumId, isTrashed: acc.get(assetByPath.get(pr.src)).isTrashed })
      } else {
        acc
      }
    ),
    assetByPath' = SUBTREE_PAIRS.foldl(assetByPath, (acc, pr) =>
      if (acc.keys().contains(pr.src)) {
        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k)).put(pr.dst, acc.get(pr.src))
      } else {
        acc.keys().exclude(Set(pr.src)).mapBy(k => acc.get(k))
      }
    ),
    dupeIndex' = dupeIndex,
    metadataPayload' = metadataPayload,
    thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // ---------- Intake submits ----------

  // @assumes ASS-6
  action SubmitScan(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    generations' = generations.set(p, generations.get(p) + 1),
    intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "scan", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar, assetHash: files.get(p).assetHash })),
    files' = files,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @assumes ASS-6
  action SubmitWatcherIndex(p: str): bool = all {
    p.in(PATHS),
    files.get(p).present,
    generations' = generations.set(p, generations.get(p) + 1),
    intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "watcher", mtime: files.get(p).mtime, len: files.get(p).len, sidecar: files.get(p).sidecar, assetHash: files.get(p).assetHash })),
    files' = files,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @assumes ASS-6
  action SubmitWatcherRemove(p: str): bool = all {
    p.in(PATHS),
    not(files.get(p).present),
    generations' = generations.set(p, generations.get(p) + 1),
    intake' = intake.union(Set({ path: p, generation: generations.get(p) + 1, kind: "remove", mtime: 0, len: 0, sidecar: 0, assetHash: 0 })),
    files' = files,
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // ---------- Worker steps ----------

  // Worker* = the intake worker (tasks/queue/worker.rs) applying an IntakeItem; the index work is workflow::process_file; IntakeKind handling in tasks/queue/intake.rs.
  // @assumes ASS-4
  // @scope SCOPE-4
  action WorkerApplySuccess: bool = {
    nondet it = oneOf(if (intake.size() > 0) intake else Set({ path: "p1", generation: 0, kind: "scan", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    // Re-index re-resolves the row by path (flush_tree.rs::existing_id) and,
    // when the path key outlived its record (N5), reuses the id with
    // old_record = None: the carried fields are empty rather than missing. The
    // record reads below therefore test resolvability instead of raising.
    all {
      intake.size() > 0,
      it.generation == generations.get(it.path),
      files.get(it.path).present,
      snapshotMatches(files.get(it.path), it),
      assetById' = (if (assetByPath.keys().contains(it.path)) { assetById } else { assetById }).put(
        if (assetByPath.keys().contains(it.path)) { assetByPath.get(it.path) } else { nextAssetId },
        { assetId: if (assetByPath.keys().contains(it.path)) { assetByPath.get(it.path) } else { nextAssetId },
          kind: if (assetByPath.keys().contains(it.path) and assetById.keys().contains(assetByPath.get(it.path))) { assetById.get(assetByPath.get(it.path)).kind } else { "media" },
          path: it.path,
          contentHash: it.assetHash,
          albumId: if (assetByPath.keys().contains(it.path) and assetById.keys().contains(assetByPath.get(it.path))) { assetById.get(assetByPath.get(it.path)).albumId } else { 0 },
          isTrashed: if (assetByPath.keys().contains(it.path) and assetById.keys().contains(assetByPath.get(it.path))) { assetById.get(assetByPath.get(it.path)).isTrashed } else { false } }),
      assetByPath' = assetByPath.put(it.path, if (assetByPath.keys().contains(it.path)) { assetByPath.get(it.path) } else { nextAssetId }),
      metadataPayload' = metadataPayload.put(
        if (assetByPath.keys().contains(it.path)) { assetByPath.get(it.path) } else { nextAssetId },
        { kind: if (assetByPath.keys().contains(it.path) and metadataPayload.keys().contains(assetByPath.get(it.path))) { metadataPayload.get(assetByPath.get(it.path)).kind } else { "media" },
          assetHash: it.assetHash,
          modified: it.mtime,
          fileSize: it.len,
          sidecar: it.sidecar,
          converged: true,
          itemCount: if (assetByPath.keys().contains(it.path) and metadataPayload.keys().contains(assetByPath.get(it.path))) { metadataPayload.get(assetByPath.get(it.path)).itemCount } else { 0 },
          itemSize: if (assetByPath.keys().contains(it.path) and metadataPayload.keys().contains(assetByPath.get(it.path))) { metadataPayload.get(assetByPath.get(it.path)).itemSize } else { 0 },
          hasShareList: if (assetByPath.keys().contains(it.path) and metadataPayload.keys().contains(assetByPath.get(it.path))) { metadataPayload.get(assetByPath.get(it.path)).hasShareList } else { false } }),
      dupeIndex' = (if (assetByPath.keys().contains(it.path) and metadataPayload.keys().contains(assetByPath.get(it.path)) and metadataPayload.get(assetByPath.get(it.path)).assetHash >= 0 and metadataPayload.get(assetByPath.get(it.path)).assetHash != it.assetHash and dupeIndex.keys().contains(metadataPayload.get(assetByPath.get(it.path)).assetHash)) { dupeIndex.put(metadataPayload.get(assetByPath.get(it.path)).assetHash, dupeIndex.get(metadataPayload.get(assetByPath.get(it.path)).assetHash).exclude(Set(assetByPath.get(it.path)))) } else { dupeIndex }).put(it.assetHash, (if (dupeIndex.keys().contains(it.assetHash)) { dupeIndex.get(it.assetHash) } else { Set() }).union(Set(if (assetByPath.keys().contains(it.path)) { assetByPath.get(it.path) } else { nextAssetId }))),
      thumbPresent' = thumbPresent.put(it.assetHash, true),
      intake' = intake.filter(i => not (i.path == it.path and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      sweepPending' = sweepPending,
      nextAssetId' = if (assetByPath.keys().contains(it.path)) { nextAssetId } else { nextAssetId + 1 },
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
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
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @assumes ASS-5
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
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @assumes ASS-4
  // @assumes ASS-5
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
      // The skip is an idempotence abstraction, not a worker branch: the worker
      // has no unchanged-skip and the hash short-circuit is deferred (REQ-1),
      // so intake applies and flush_tree.rs rewrites ASSET_BY_ID on every
      // apply. Skipping is sound only when applying would write nothing new,
      // which includes the identity record's content hash.
      assetById.keys().contains(assetByPath.get(it.path)),
      assetById.get(assetByPath.get(it.path)).contentHash == it.assetHash,
      intake' = intake.filter(i => not (i.path == it.path and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
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
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @operation OP-8
  // @assumes ASS-3
  action WorkerRemoveApply: bool = {
    nondet it = oneOf(if (intake.size() > 0) intake else Set({ path: "p1", generation: 0, kind: "remove", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    all {
      intake.size() > 0,
      it.kind == "remove",
      it.generation == generations.get(it.path),
      not(files.get(it.path).present),
      // Album-liveness re-check: apply_album_delta no-ops without the album row.
      metadataPayload.keys().contains(0),
      assetById' = if (assetByPath.keys().contains(it.path)) { assetById.keys().exclude(Set(assetByPath.get(it.path))).mapBy(k => assetById.get(k)) } else { assetById },
      assetByPath' = if (assetByPath.keys().contains(it.path)) { assetByPath.keys().exclude(Set(it.path)).mapBy(k => assetByPath.get(k)) } else { assetByPath },
      metadataPayload' = if (assetByPath.keys().contains(it.path)) { metadataPayload.keys().exclude(Set(assetByPath.get(it.path))).mapBy(k => metadataPayload.get(k)).put(0, { kind: metadataPayload.get(0).kind, assetHash: metadataPayload.get(0).assetHash, modified: metadataPayload.get(0).modified, fileSize: metadataPayload.get(0).fileSize, sidecar: metadataPayload.get(0).sidecar, converged: metadataPayload.get(0).converged, itemCount: metadataPayload.get(0).itemCount - (if (assetById.keys().contains(assetByPath.get(it.path)) and assetById.get(assetByPath.get(it.path)).albumId == 0 and not(assetById.get(assetByPath.get(it.path)).isTrashed)) { 1 } else { 0 }), itemSize: metadataPayload.get(0).itemSize - (if (assetById.keys().contains(assetByPath.get(it.path)) and assetById.get(assetByPath.get(it.path)).albumId == 0 and not(assetById.get(assetByPath.get(it.path)).isTrashed)) { (if (metadataPayload.keys().contains(assetByPath.get(it.path))) { metadataPayload.get(assetByPath.get(it.path)).fileSize } else { 0 }) } else { 0 }), hasShareList: metadataPayload.get(0).hasShareList }) } else { metadataPayload },
      dupeIndex' = if (assetByPath.keys().contains(it.path) and assetById.keys().contains(assetByPath.get(it.path)) and assetById.get(assetByPath.get(it.path)).contentHash >= 0 and dupeIndex.keys().contains(assetById.get(assetByPath.get(it.path)).contentHash)) { dupeIndex.put(assetById.get(assetByPath.get(it.path)).contentHash, dupeIndex.get(assetById.get(assetByPath.get(it.path)).contentHash).exclude(Set(assetByPath.get(it.path)))) } else { dupeIndex },
      thumbPresent' = if (assetByPath.keys().contains(it.path) and assetById.keys().contains(assetByPath.get(it.path)) and assetById.get(assetByPath.get(it.path)).contentHash >= 0 and dupeIndex.keys().contains(assetById.get(assetByPath.get(it.path)).contentHash)) { thumbPresent.put(assetById.get(assetByPath.get(it.path)).contentHash, dupeIndex.get(assetById.get(assetByPath.get(it.path)).contentHash).exclude(Set(assetByPath.get(it.path))).size() > 0) } else { thumbPresent },
      intake' = intake.filter(i => not (i.path == it.path and i.generation == it.generation)),
      files' = files,
      generations' = generations,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @assumes ASS-3
  // @assumes ASS-6
  // @assumes ASS-7
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @assumes ASS-3
  action SweepRoot(r: str): bool = all {
    r.in(ROOTS),
    sweepPending.get(r),
    assetByPath' = assetByPath.keys().filter(p => not (rootOf(p) == r and not(files.get(p).present))).mapBy(k => assetByPath.get(k)),
    assetById' = assetById,
    metadataPayload' = metadataPayload,
    dupeIndex' = dupeIndex,
    thumbPresent' = thumbPresent,
    sweepPending' = sweepPending.put(r, false),
    files' = files,
    generations' = generations,
    intake' = intake,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
  }

  // @exempt observer
  action ApiRead: bool = all {
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

  // @exempt observer
  action ReadMissing(p: str): bool = all {
    p.in(PATHS),
    (rows.keys().contains(p) and not(files.get(p).present)) or (not(rows.keys().contains(p))),
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

  // @assumes ASS-6
  // @scope SCOPE-3
  // @step-universe ExternalModify, ExternalDelete, ExternalRenameFile, ExternalRenameDir,
  // @step-universe ExternalSidecarCreate, ExternalSidecarModify, ExternalSidecarDelete,
  // @step-universe ForceModify, MarkUnprocessable, RestoreIndexed, ProcessCrash, LoseThumbs,
  // @step-universe DbLoss, AppSidecarEdit, RotateWriteSidecar, RotateFinish, ApiUpload,
  // @step-universe ApiRemove, ApiMove, ApiAssignDirMove, SubmitScan, SubmitWatcherIndex,
  // @step-universe SubmitWatcherRemove, WorkerApplySuccess, WorkerSkipSuperseded,
  // @step-universe WorkerSkipStale, WorkerSkipUnchanged, WorkerOpenFails, WorkerRemoveApply,
  // @step-universe ScanRoot, SweepRoot, ApiRead, ReadMissing
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

  // @assumes ASS-6
  run scanApplyTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @exempt no-backing
  run skipSupersededTest = init
    .then(SubmitScan("p1"))
    .then(SubmitScan("p1"))
    .then(WorkerSkipSuperseded)
    .then(all { assert(NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // Pins the FS_MODIFY_INFLIGHT gap: the in-flight item is dropped and nothing resubmits the path until the next scan, so permanent staleness is expected here.
  // @assumes ASS-5
  run skipStaleTest = init
    .then(SubmitScan("p1"))
    .then(ForceModify("p1"))
    .then(WorkerSkipStale)
    .then(all { assert(NoLostUpload and not(NoPermanentStale)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-5
  run skipUnchangedTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(SubmitScan("p1"))
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-5
  run restoreSkipsTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ForceModify("p1"))
    .then(RestoreIndexed("p1"))
    .then(SubmitScan("p1"))
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-1
  run copyDistinctIdsTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(SubmitScan("p2"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p1").assetHash == rows.get("p2").assetHash and rows.get("p1").assetId != rows.get("p2").assetId), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run sidecarDeleteConvergesTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalSidecarModify("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalSidecarDelete("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p1").sidecar == 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run renameFileTest = init
    .then(ExternalDelete("p2"))
    .then(SubmitWatcherRemove("p2"))
    .then(WorkerRemoveApply)
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalRenameFile("p1", "p2"))
    .then(WorkerRemoveApply)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and not(rows.keys().contains("p1")) and rows.keys().contains("p2")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  run uploadTest = init
    .then(ApiUpload("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run removeTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-1
  // @assumes ASS-6
  run apiMoveSkipsRescanTest = init
    .then(ExternalDelete("p2"))
    .then(SubmitWatcherRemove("p2"))
    .then(WorkerRemoveApply)
    .then(ApiUpload("p1"))
    .then(WorkerApplySuccess)
    .then(ApiMove("p1", "p2"))
    .then(SubmitWatcherIndex("p2"))
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p2").assetId == 1), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @scope SCOPE-5
  run apiRemoveTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ApiRemove("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run scanOnlyConvergenceTest = init
    .then(ExternalDelete("p1"))
    .then(all { assert(rows.keys().contains("p1") and not(Converged) and not(NoPermanentStale)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(all { assert(NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SweepRoot("r1"))
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run scanHealsMissingRowTest = init
    .then(ExternalDelete("p1"))
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(ExternalModify("p1"))
    .then(all { assert(not(rows.keys().contains("p1")) and not(Converged)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerSkipSuperseded)
    .then(WorkerSkipSuperseded)
    .then(WorkerSkipSuperseded)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run scanHealsStaleRowTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ForceModify("p1"))
    .then(all { assert(not(Converged)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run perRootSweepTest = init
    .then(ExternalDelete("p5"))
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(all { assert(not(Converged) and not(NoPermanentStale)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r2"))
    .then(all { assert(NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SweepRoot("r2"))
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  run fsCreateTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalModify("p1"))
    .then(SubmitWatcherIndex("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  run fsModifyTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalModify("p1"))
    .then(SubmitWatcherIndex("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-5
  run fsModifyInflightTest = init
    .then(SubmitScan("p1"))
    .then(ForceModify("p1"))
    .then(WorkerSkipStale)
    .then(all { assert(not(Converged) and intake.size() == 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run fsAtomicReplaceTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalModify("p1"))
    .then(SubmitWatcherIndex("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run sidecarCreateTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalSidecarCreate("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p1").sidecar == files.get("p1").sidecar and rows.get("p1").sidecar != 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run sidecarModifyTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalSidecarModify("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-1
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
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and not(rows.keys().contains("p1")) and not(rows.keys().contains("p2")) and rows.keys().contains("p3") and rows.keys().contains("p4") and rows.get("p3").assetId == 1 and rows.get("p4").assetId == 2), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-1
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
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and not(rows.keys().contains("p1")) and not(rows.keys().contains("p2")) and rows.keys().contains("p3") and rows.keys().contains("p4") and rows.get("p3").assetId == 1 and rows.get("p4").assetId == 2), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @scope SCOPE-5
  run rotateTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(RotateWriteSidecar("p1"))
    .then(all { assert(rows.get("p1").sidecar != files.get("p1").sidecar and not(Converged)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run watcherGapTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(ForceModify("p1"))
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // A scan that snapshots before the change drops the stale item; the next
  // backstop scan heals it, so convergence is bounded by two scan cycles
  // (one backstop interval after the change).
  // @assumes ASS-3
  // @assumes ASS-7
  run boundedConvergenceTest = init
    .then(ScanRoot("r1"))
    .then(ForceModify("p1"))
    .then(WorkerSkipStale)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(all { assert(not(Converged) and intake.size() == 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and intake.size() == 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @scope SCOPE-6
  run crashTest = init
    .then(SubmitScan("p1"))
    .then(ForceModify("p1"))
    .then(ProcessCrash)
    .then(all { assert(intake.size() == 0 and not(Converged)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-4
  run derivedLossTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(LoseThumbs)
    .then(all { assert(not(Converged)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(ScanRoot("r2"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @scope SCOPE-6
  run dbLossTest = init
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(SubmitScan("p2"))
    .then(WorkerApplySuccess)
    .then(DbLoss)
    .then(all { assert(rows.keys().size() == 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(ScanRoot("r2"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale and rows.get("p1").assetId != rows.get("p2").assetId), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(not(rows.keys().contains("p1")) and Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  run readMissingTest = init
    .then(ExternalDelete("p1"))
    .then(ReadMissing("p1"))
    .then(all { assert(not(Converged)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(WorkerSkipUnchanged)
    .then(all { assert(Converged and NoLostUpload and NoPermanentStale), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
  // ---------- section: asset-aspect slice ----------
  // Asset-aspect slice of the index pipeline model.
  //
  // Extends the eventual-consistency model with per-asset-aspect alignment:
  // an asset is aligned only when its file (media bytes + sidecar bytes),
  // derived rendition (thumbnail), and DB row agree. The predicates below
  // name that agreement; the new actions model mutation windows the main
  // model does not (directory moves that touch the filesystem only, partial
  // sidecar edits, dropped sidecar watcher events).

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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
    sweepPending' = sweepPending,
    nextAssetId' = nextAssetId,
    unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths.union(Set(p, q)),
  }

  // @operation OP-9
  // @scope SCOPE-8
  // SweepEnumerate maps to the enumeration loop of tasks/actor/album_index.rs::sweep_stale_asset_paths (candidates from ASSET_BY_PATH rows with missing files, per-candidate generation snapshot).
  // Enumeration accumulates: sweep_stale_asset_paths submits one
  // WriteOp::SweepRemoveBatch per root to the writer queue, so a root's
  // enumeration adds its candidates to the submissions already queued for the
  // other roots instead of replacing them. Paths are root-disjoint (rootOf is
  // static), so one path-keyed map holds every queued submission; the
  // enumerated root's own entries are rebuilt from this walk.
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

  // @operation OP-9
  // @scope SCOPE-9
  // SweepRecheck maps to the inline re-check loop of sweep_stale_asset_paths (generation unchanged + path_epoch::is_busy + path.exists, pre-submit).
  // The batch may hold several roots' queued submissions; the re-check is
  // per candidate (generation, lease, exists) and does not depend on which
  // root enumerated it.
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

  // @operation OP-9
  // @scope SCOPE-11
  // SweepRemoveBatchApply maps to flush_tree.rs::sweep_remove_batch, submitted as WriteOp::SweepRemoveBatch via tasks/queue/writer.rs::submit; checks path.exists skip + asset-id/record resolution only, no generation/lease check.
  // The apply drains the whole accumulated batch: one writer submission per
  // root, each applied per path in the writer queue.
  // @operation OP-8
  action SweepRemoveBatchApply: bool = all {
    sweepBatch.keys().size() > 0,
    // Album-liveness re-check: apply_album_delta no-ops without the album row.
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
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
    // Resolve-then-validate (REQ-7): the row rewrite reads the record it
    // re-resolved by path under the move lease; a missing record (N5) aborts
    // the move, so the step is not enabled.
    assetById.keys().contains(assetByPath.get(p)),
    // SCOPE-10: the move lease is held across this rewrite and released here;
    // the row follows the FS rename, so an acquired-but-unmoved lease cannot.
    leasedPaths.contains(p),
    leasedPaths.contains(q),
    not(files.get(p).present),
    files.get(q).present,
    leasedPaths' = leasedPaths.exclude(Set(p, q)),
    assetById' = assetById.put(assetByPath.get(p), { assetId: assetByPath.get(p), kind: assetById.get(assetByPath.get(p)).kind, path: q, contentHash: assetById.get(assetByPath.get(p)).contentHash, albumId: assetById.get(assetByPath.get(p)).albumId, isTrashed: assetById.get(assetByPath.get(p)).isTrashed }),
    assetByPath' = assetByPath.keys().exclude(Set(p)).mapBy(k => assetByPath.get(k)).put(q, assetByPath.get(p)),
    metadataPayload' = metadataPayload,
    dupeIndex' = dupeIndex,
    thumbPresent' = thumbPresent,
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
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
    assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
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
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsScanSupersedesEditTest = init
    .then(SubmitScan("p1"))
    .then(AppSidecarEdit("p1"))
    .then(WorkerSkipSuperseded)
    .then(all { assert(Converged and AspectsAligned and rows.get("p1").sidecar == files.get("p1").sidecar), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(not(AspectsAligned)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  // @scope SCOPE-10
  run aspectsMoveCrashHealsFromSidecarTest = init
    .then(ExternalSidecarModify("p1"))
    .then(WorkerApplySuccess)
    .then(ExternalDelete("p3"))
    .then(SubmitWatcherRemove("p3"))
    .then(WorkerRemoveApply)
    .then(ApiMoveFsOnly("p1", "p3"))
    .then(all { assert(not(AspectsAligned)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(AspectsAligned and assetByPath.keys().contains("p3") and metadataPayload.get(assetByPath.get("p3")).sidecar == files.get("p3").sidecar and files.get("p3").sidecar != 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @operation OP-7
  // @scope SCOPE-5
  // @assumes ASS-6
  run aspectsSidecarPartialEditTest = init
    .then(SidecarEditPartial("p1"))
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @exempt environment
  // @assumes ASS-6
  // @assumes ASS-7
  run aspectsDroppedSidecarTest = init
    .then(DroppedSidecarModify("p1"))
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-4
  // @assumes ASS-6
  run aspectsThumbLossTest = init
    .then(LoseThumbs)
    .then(all { assert(not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(SubmitScan("p1"))
    .then(WorkerApplySuccess)
    .then(all { assert(AspectAligned("p1")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  run aspectsWatcherRemoveTest = init
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(not(rows.keys().contains("p1")) and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(rows.keys().contains("p1") and not(files.get("p1").present)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(rows.keys().contains("p1") and not(files.get("p1").present)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(MoveRowRewrite("p1", "p3"))
    .then(all { assert(not(rows.keys().contains("p1")) and rows.keys().contains("p3")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-9
  run aspectsSupersededSweepTest = init
    .then(ExternalDelete("p1"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SubmitWatcherRemove("p1"))
    .then(SweepRecheck)
    .then(all { assert(rows.keys().contains("p1")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-9
  run aspectsCleanSweepTest = init
    .then(ExternalDelete("p1"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(SweepRemoveBatchApply)
    .then(all { assert(not(rows.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(not(rows.keys().contains("p3"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ApiMoveFsOnly("p1", "p3"))
    .then(MoveRowRewrite("p1", "p3"))
    .then(all { assert(not(rows.keys().contains("p1")) and rows.keys().contains("p3")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsRotateVsScanTest = init
    .then(SubmitScan("p1"))
    .then(RotateWriteSidecar("p1"))
    .then(WorkerSkipSuperseded)
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @scope SCOPE-5
  // @assumes ASS-6
  run aspectsRotateTest = init
    .then(RotateWriteSidecar("p1"))
    .then(RotateFinish("p1"))
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
      sweepPending' = sweepPending,
      nextAssetId' = nextAssetId,
      unprocessable' = unprocessable,
    sweepBatch' = sweepBatch,
    leasedPaths' = leasedPaths,
    }
  }

  // @operation OP-1
  // @operation OP-8
  // @assumes ASS-4
  // @assumes ASS-5
  // @scope SCOPE-4
  // UploadIndexed maps to tasks/queue/worker.rs intake application running workflow::process_file (index path).
  action UploadIndexed(p: str): bool = {
    nondet it = oneOf(if (intake.exists(i => i.path == p)) intake.filter(i => i.path == p) else Set({ path: p, generation: 0, kind: "watcher", mtime: 0, len: 0, sidecar: 0, assetHash: 0 }))
    // Re-index re-resolves the row by path (flush_tree.rs::existing_id) and
    // reuses the id with old_record = None when the path key outlived its
    // record (N5); the record reads test resolvability instead of raising.
    all {
      p.in(PATHS),
      // Album-liveness re-check: apply_album_delta no-ops without the album row.
      metadataPayload.keys().contains(0),
      intake.exists(i => i.path == p),
      it.path == p,
      it.generation == generations.get(p),
      files.get(p).present,
      snapshotMatches(files.get(p), it),
      assetById' = assetById.put(if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }, { assetId: if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }, kind: if (assetByPath.keys().contains(p) and assetById.keys().contains(assetByPath.get(p))) { assetById.get(assetByPath.get(p)).kind } else { "media" }, path: p, contentHash: it.assetHash, albumId: if (assetByPath.keys().contains(p) and assetById.keys().contains(assetByPath.get(p))) { assetById.get(assetByPath.get(p)).albumId } else { 0 }, isTrashed: if (assetByPath.keys().contains(p) and assetById.keys().contains(assetByPath.get(p))) { assetById.get(assetByPath.get(p)).isTrashed } else { false } }),
      assetByPath' = assetByPath.put(p, if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }),
      metadataPayload' = metadataPayload
        .put(if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }, { kind: if (assetByPath.keys().contains(p) and metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).kind } else { "media" }, assetHash: it.assetHash, modified: it.mtime, fileSize: it.len, sidecar: it.sidecar, converged: true, itemCount: 0, itemSize: 0, hasShareList: false })
        .put(0, { kind: metadataPayload.get(0).kind, assetHash: metadataPayload.get(0).assetHash, modified: metadataPayload.get(0).modified, fileSize: metadataPayload.get(0).fileSize, sidecar: metadataPayload.get(0).sidecar, converged: metadataPayload.get(0).converged, itemCount: metadataPayload.get(0).itemCount + (if (assetByPath.keys().contains(p)) { 0 } else { 1 }), itemSize: metadataPayload.get(0).itemSize + (it.len - (if (assetByPath.keys().contains(p) and metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).fileSize } else { 0 })), hasShareList: metadataPayload.get(0).hasShareList }),
      dupeIndex' = dupeIndex.put(it.assetHash, (if (dupeIndex.keys().contains(it.assetHash)) { dupeIndex.get(it.assetHash) } else { Set() }).union(Set(if (assetByPath.keys().contains(p)) { assetByPath.get(p) } else { nextAssetId }))),
      thumbPresent' = thumbPresent.put(it.assetHash, true),
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
      assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent,
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
    .then(all { assert(not(rows.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(UploadStoredOnly("p1"))
    .then(all { assert(UploadingWindow("p1") and not(AspectAligned("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(UploadIndexed("p1"))
    .then(all { assert(AspectsAligned and rows.keys().contains("p1")), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsUploadBeforeSweepTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ScanRoot("r1"))
    .then(SweepRoot("r1"))
    .then(all { assert(rows.keys().contains("p1") and files.get("p1").present), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(not(rows.keys().contains("p1")) and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsUploadThenSidecarEditTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ExternalSidecarModify("p1"))
    .then(WorkerSkipSuperseded)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned and rows.get("p1").sidecar == files.get("p1").sidecar), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-6
  run aspectsUploadCrashHealsTest = init
    .then(ExternalDelete("p1"))
    .then(UploadStoredOnly("p1"))
    .then(ProcessCrash)
    .then(all { assert(files.get("p1").present and intake.size() == 0), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(WorkerApplySuccess)
    .then(all { assert(Converged and AspectsAligned), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    // Album-liveness re-check: apply_album_delta no-ops without the album row.
    metadataPayload.keys().contains(0),
    p.in(PATHS),
    assetByPath.keys().contains(p),
    assetById' = assetById.keys().exclude(Set(assetByPath.get(p))).mapBy(k => assetById.get(k)),
    assetByPath' = assetByPath,
    metadataPayload' = metadataPayload.keys().exclude(Set(assetByPath.get(p))).mapBy(k => metadataPayload.get(k))
      .put(0, { kind: metadataPayload.get(0).kind, assetHash: metadataPayload.get(0).assetHash, modified: metadataPayload.get(0).modified, fileSize: metadataPayload.get(0).fileSize, sidecar: metadataPayload.get(0).sidecar, converged: metadataPayload.get(0).converged,
        itemCount: metadataPayload.get(0).itemCount - (if (assetById.keys().contains(assetByPath.get(p)) and assetById.get(assetByPath.get(p)).albumId == 0 and not(assetById.get(assetByPath.get(p)).isTrashed)) { 1 } else { 0 }),
        itemSize: metadataPayload.get(0).itemSize - (if (assetById.keys().contains(assetByPath.get(p)) and assetById.get(assetByPath.get(p)).albumId == 0 and not(assetById.get(assetByPath.get(p)).isTrashed)) { (if (metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).fileSize } else { 0 }) } else { 0 }),
        hasShareList: metadataPayload.get(0).hasShareList }),
    dupeIndex' = dupeIndex,
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

  // @operation OP-7
  // @scope SCOPE-5
  // @exempt environment
  // @assumes ASS-6
  // MemberWrite maps to a metadata/flag write to a media asset that calls
  // mark_album_for_update for the asset's containing album (flush_tree.rs /
  // assign_album.rs / create_dir_album.rs).
  action MemberWrite(p: str): bool = all {
    p.in(PATHS),
    assetByPath.keys().contains(p),

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

  // @operation OP-8
  // @exempt environment
  // @assumes ASS-6
  // AlbumShareEdit models an edit_share RMW on the album's share_list
  // (hasShareList flag) independent of aggregate deltas.
  action AlbumShareEdit: bool = all {
    metadataPayload.keys().contains(0),
    assetById.keys().contains(0),
    metadataPayload' = metadataPayload.put(0, { kind: metadataPayload.get(0).kind, assetHash: metadataPayload.get(0).assetHash, modified: metadataPayload.get(0).modified, fileSize: metadataPayload.get(0).fileSize, sidecar: metadataPayload.get(0).sidecar, converged: metadataPayload.get(0).converged, itemCount: metadataPayload.get(0).itemCount, itemSize: metadataPayload.get(0).itemSize, hasShareList: not(metadataPayload.get(0).hasShareList) }),
    assetById' = assetById,
    assetByPath' = assetByPath,
    dupeIndex' = dupeIndex,
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

  // @operation OP-7
  // @operation OP-8
  // @scope SCOPE-5
  // @exempt environment
  // @assumes ASS-6
  // TrashToggle flips isTrashed on a member and applies the aggregate
  // delta in the same step (exclusion rule I6).
  action TrashToggle(p: str): bool = all {
    p.in(PATHS),
    assetByPath.keys().contains(p),
    assetById.keys().contains(assetByPath.get(p)),
    // Album-liveness re-check: apply_album_delta no-ops without the album row.
    metadataPayload.keys().contains(0),
    assetById' = assetById.put(assetByPath.get(p), { assetId: assetByPath.get(p), kind: assetById.get(assetByPath.get(p)).kind, path: assetById.get(assetByPath.get(p)).path, contentHash: assetById.get(assetByPath.get(p)).contentHash, albumId: assetById.get(assetByPath.get(p)).albumId, isTrashed: not(assetById.get(assetByPath.get(p)).isTrashed) }),
    assetByPath' = assetByPath,
    dupeIndex' = dupeIndex,
    metadataPayload' = metadataPayload.put(0, { kind: metadataPayload.get(0).kind, assetHash: metadataPayload.get(0).assetHash, modified: metadataPayload.get(0).modified, fileSize: metadataPayload.get(0).fileSize, sidecar: metadataPayload.get(0).sidecar, converged: metadataPayload.get(0).converged,
      itemCount: metadataPayload.get(0).itemCount + (if (assetById.get(assetByPath.get(p)).isTrashed) { 1 } else { -1 }),
      itemSize: metadataPayload.get(0).itemSize + (if (assetById.get(assetByPath.get(p)).isTrashed) { (if (metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).fileSize } else { 0 }) } else { -(if (metadataPayload.keys().contains(assetByPath.get(p))) { metadataPayload.get(assetByPath.get(p)).fileSize } else { 0 }) }),
      hasShareList: metadataPayload.get(0).hasShareList })
      .put(assetByPath.get(p), metadataPayload.get(assetByPath.get(p))),
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
    .then(all { assert(assetByPath.keys().contains("p1") and not(assetById.keys().contains(assetByPath.get("p1")))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(SweepRemoveBatchApply)
    .then(all { assert(not(assetByPath.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

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
    .then(all { assert(assetByPath.keys().contains("p1") and thumbPresent.get(0)), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ExternalDelete("p1"))
    .then(SubmitWatcherRemove("p1"))
    .then(WorkerRemoveApply)
    .then(all { assert(not(assetByPath.keys().contains("p1")) and (not(thumbPresent.keys().contains(0)) or not(thumbPresent.get(0)))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-5
  run aspectsRemoveTornWindowTest = init
    .then(ExternalDelete("p1"))
    .then(RemoveIdOnly("p1"))
    .then(all { assert(assetByPath.keys().contains("p1") and not(assetById.keys().contains(assetByPath.get("p1")))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(SweepRemoveBatchApply)
    .then(all { assert(not(assetByPath.keys().contains("p1"))), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  // Delta: aggregates are exact in the initial state by construction.
  run aspectsAlbumDeltaConsistentOnInitTest = init
    .then(all { assert(AlbumStatsConsistent and AlbumStatsNoPhantom), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  // Delta: trash toggle removes the member in the same step and aggregates
  // stay exact both directions.
  run aspectsAlbumDeltaTrashToggleTest = init
    .then(TrashToggle("p1"))
    .then(all { assert(AlbumStatsConsistent and AlbumStatsNoPhantom), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })
    .then(TrashToggle("p1"))
    .then(all { assert(AlbumStatsConsistent and AlbumStatsNoPhantom), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  // Delta then share edit: both effects survive (commutation, I3).
  run aspectsAlbumDeltaThenShareEditTest = init
    .then(TrashToggle("p1"))
    .then(AlbumShareEdit)
    .then(all { assert(AlbumStatsConsistent and AlbumStatsNoPhantom and metadataPayload.get(0).hasShareList), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  // Share edit then delta: both effects survive (commutation, I3 reverse).
  run aspectsShareEditThenAlbumDeltaTest = init
    .then(AlbumShareEdit)
    .then(TrashToggle("p1"))
    .then(all { assert(AlbumStatsConsistent and AlbumStatsNoPhantom and metadataPayload.get(0).hasShareList), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  // Re-apply idempotence (I5): re-indexing an unchanged asset does not
  // double-count.
  run aspectsAlbumDeltaReapplyIdempotenceTest = init
    .then(ScanRoot("r1"))
    .then(UploadIndexed("p1"))
    .then(all { assert(AlbumStatsConsistent and AlbumStatsNoPhantom), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  // Heal composition (I8): torn residue is cleaned and aggregates converge.
  run aspectsAlbumDeltaHealCompositionTest = init
    .then(ExternalDelete("p1"))
    .then(RemoveIdOnly("p1"))
    .then(ScanRoot("r1"))
    .then(SweepEnumerate("r1"))
    .then(SweepRecheck)
    .then(SweepRemoveBatchApply)
    .then(all { assert(AlbumStatsConsistent and AlbumStatsNoPhantom), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // @scope SCOPE-5
  // Pure metadata write leaves aggregates untouched.
  run aspectsAlbumDeltaMemberWritePureTest = init
    .then(MemberWrite("p1"))
    .then(all { assert(AlbumStatsConsistent and AlbumStatsNoPhantom), files' = files, generations' = generations, intake' = intake, assetById' = assetById, assetByPath' = assetByPath, dupeIndex' = dupeIndex, metadataPayload' = metadataPayload, thumbPresent' = thumbPresent, sweepPending' = sweepPending, nextAssetId' = nextAssetId, unprocessable' = unprocessable, sweepBatch' = sweepBatch, leasedPaths' = leasedPaths })

  // @assumes ASS-6
  // Stats are exact in every reachable state: deltas are applied in the
  // same step as the membership change (no deferred cascade).
  val AlbumStatsConsistent = assetById.keys().forall(id =>
    assetById.get(id).kind == "album" implies (
      metadataPayload.keys().contains(id) and
      metadataPayload.get(id).itemCount == assetById.keys().filter(m => assetById.get(m).albumId == id and not(assetById.get(m).isTrashed)).size() and
      metadataPayload.get(id).itemSize == assetById.keys().filter(m => assetById.get(m).albumId == id and not(assetById.get(m).isTrashed)).fold(0, (acc, m) => acc + (if (metadataPayload.keys().contains(m)) metadataPayload.get(m).fileSize else 0))
    )
  )

  // @assumes ASS-6
  // Stats never claim membership assetById refutes (no phantom members).
  val AlbumStatsNoPhantom = assetById.keys().forall(id =>
    assetById.get(id).kind == "album" implies (
      metadataPayload.keys().contains(id) and
      metadataPayload.get(id).itemCount <= assetById.keys().filter(m => assetById.get(m).albumId == id and not(assetById.get(m).isTrashed)).size() and
      metadataPayload.get(id).itemSize <= assetById.keys().filter(m => assetById.get(m).albumId == id and not(assetById.get(m).isTrashed)).fold(0, (acc, m) => acc + (if (metadataPayload.keys().contains(m)) metadataPayload.get(m).fileSize else 0))
    )
  )

  // ---------- section: completion machine, proof runs ----------
  // Proof-lane step: the environment (FS events, API calls, faults) fires at
  // any time and the pipeline is the system, so the goal is as-if-serial
  // eventual consistency — a drained state agrees with the FS record (ASS-3).
  //
  // Completion machine. The environment does not stop on its own, so the
  // invariant needs an observation point: a three-phase cycle in which the
  // environment is frozen, the pipeline drains, and a scan of each root runs
  // last (G1: changes stopped *and* a full scan completed).
  //
  //   phase 0 free     environment actions and every pipeline branch may fire;
  //                    the pipeline may be arbitrarily dirty, and no fault
  //                    fires (fault-free request phase).
  //   phase 1 draining the environment is frozen; work-consuming branches,
  //                    budgeted faults, and untracked rescans fire; BeginScan
  //                    opens phase 2 once the pipeline is quiescent.
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
  // Faults: the request phase is crash-free; the drain admits at most one
  // fault per attempt (the `faults` budget), and a fault restarts the machine
  // in the request phase (unclean restart, requests resume, the drain is
  // retried). Phase 2 admits no faults, where completion is claimed. The
  // residue a fault leaves is what the retried drain's phase-2 scan repairs.
  // RemoveIdOnly (the N5 torn window) rides the same budget: it creates
  // residue only a scan rediscovers, so it is not a resolution action. The
  // request phase needs no fault branches: everything a crash clears there is
  // regenerated by the heal scan or is a generation bump the claim does not
  // observe (see the reboot actions below).
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
  //   budgeted drain faults (phase 1 only, faults < K, reboot to phase 0):
  //     CrashReboot             PROCESS_CRASH recovery (SCOPE-6)
  //     ThumbLossReboot         DERIVED_STORE_LOSS recovery
  //     DbLossReboot            DB_LOSS recovery
  //     RemoveIdOnly branch     N5 torn-window remove (scan-only residue)
  //
  //   observers (all phases): ApiRead, ReadMissing — router read paths, pure
  //   stutters, no work created.
  //
  //   completion bookkeeping (the completion machine): Stay, BeginDrain, BeginScan,
  //   CompleteScan, initCompletion, stepCompletion, Skip — phase and scan
  //   watermark only, no pipeline-state change beyond the composed action.

  // Completion-machine state. Base actions do not assign these vars, so they
  // stay unconstrained there; the machine's own actions below assign them.
  var phase: int
  var scannedRoots: Set[str]
  // Fault budget: how many faults this completion attempt has consumed. Only
  // the drain admits faults, and only below the budget, so a drain retry
  // cannot loop on crash/rescan churn.
  var faults: int

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
    faults' = faults,
  }

  // Branch bookkeeping: branches whose action leaves the phase machine alone
  // must still assign `phase'`/`scannedRoots'`/`faults'`. Quint leaves an
  // unassigned primed var unconstrained and requires every branch of an `any`
  // to update the same variable set (QNT202), so the stutter is explicit.
  // @scope SCOPE-13
  action Stay(b: bool): bool = all {
    b,
    phase' = phase,
    scannedRoots' = scannedRoots,
    faults' = faults,
  }

  // Budgeted drain faults. A fault is admitted in the drain only, at most once
  // per completion attempt (`faults < 1`, K=1), and it restarts the machine in
  // the request phase: an unclean restart, after which requests resume and the
  // drain is retried. Each action clears exactly what the base fault action
  // clears and charges the budget.
  //
  // Fault-free request phase. A request-phase crash adds no divergence the
  // drain does not already reach: intake, sweep batch and path leases are
  // volatile and are rebuilt by the heal scan, durable rows and FS bytes
  // survive, and the generation counters (the one observable difference: a
  // crash drops the bumps submitted but not yet applied) only move work
  // between "queued" and "to be rediscovered", which the scan order already
  // covers. Crash windows inside multi-part sequences are covered by the
  // existing two-phase action splits, and coarse API actions absorb their
  // windows soundly because their residue states coincide with
  // environment-event states (orphan sidecar = ExternalSidecarCreate, ghost
  // row = ExternalDelete with a missed submit).
  // @scope SCOPE-6
  // @scope SCOPE-13
  action CrashReboot: bool = all {
    phase == 1,
    faults < 1,
    ProcessCrash,
    faults' = faults + 1,
    phase' = 0,
    scannedRoots' = scannedRoots,
  }

  // @scope SCOPE-6
  // @scope SCOPE-13
  action ThumbLossReboot: bool = all {
    phase == 1,
    faults < 1,
    LoseThumbs,
    faults' = faults + 1,
    phase' = 0,
    scannedRoots' = scannedRoots,
  }

  // @scope SCOPE-6
  // @scope SCOPE-13
  action DbLossReboot: bool = all {
    phase == 1,
    faults < 1,
    DbLoss,
    faults' = faults + 1,
    phase' = 0,
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
    faults' = faults,
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
    faults' = faults,
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
    faults' = faults,
  }

  // Verify entry point: the base initial state plus an empty phase machine.
  // @scope SCOPE-1
  action initCompletion: bool = all {
    init,
    phase' = 0,
    scannedRoots' = Set(),
    faults' = 0,
  }

  // Uniform choice over phase-enabled branches; no ordering inside a phase.
  // @assumes ASS-6
  // @scope SCOPE-13
  // @step-universe ExternalModify, ExternalDelete, ExternalRenameFile, ExternalRenameDir,
  // @step-universe ExternalSidecarCreate, ExternalSidecarModify, ExternalSidecarDelete,
  // @step-universe ForceModify, MarkUnprocessable, RestoreIndexed, AppSidecarEdit,
  // @step-universe RotateWriteSidecar, RotateFinish, ApiUpload,
  // @step-universe ApiRemove, ApiMove, ApiAssignDirMove, SubmitScan, SubmitWatcherIndex,
  // @step-universe SubmitWatcherRemove, WorkerApplySuccess, WorkerSkipSuperseded,
  // @step-universe WorkerSkipStale, WorkerSkipUnchanged, WorkerOpenFails, WorkerRemoveApply,
  // @step-universe ScanRoot, SweepRoot, ApiRead, ReadMissing, DirMoveFsOnly, ApiMoveFsOnly,
  // @step-universe SweepEnumerate, SweepRecheck, SweepRemoveBatchApply, MoveLeaseAcquire,
  // @step-universe MoveRowRewrite, SidecarEditPartial, DroppedSidecarModify, UploadStoredOnly,
  // @step-universe UploadIndexed, IntakePickupFileGone, RemoveIdOnly, MemberWrite,
  // @step-universe AlbumShareEdit, TrashToggle, Skip, Stay, BeginDrain, BeginScan, CompleteScan,
  // @step-universe CrashReboot, ThumbLossReboot, DbLossReboot
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

    // phase 1: drain. Work-consuming branches, then the budgeted faults (each
    // reboots to phase 0), then the quiescence-gated phase transition.
    // Untracked ScanRoot branches model the rescan/backstop triggers mixing
    // into the drain; their intake and sweepPending must drain before BeginScan
    // is enabled.
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
    CrashReboot,
    ThumbLossReboot,
    DbLossReboot,
    // RemoveIdOnly (the N5 torn window) is pipeline residue rather than an
    // environment fault; it rides the same budget and reboot. Its literal
    // branch (rather than a helper action) keeps its step-universe membership
    // visible in this list.
    all { phase == 1, faults < 1, RemoveIdOnly("p1"), faults' = faults + 1, phase' = 0, scannedRoots' = scannedRoots },
    all { phase == 1, faults < 1, RemoveIdOnly("p2"), faults' = faults + 1, phase' = 0, scannedRoots' = scannedRoots },
    all { phase == 1, faults < 1, RemoveIdOnly("p3"), faults' = faults + 1, phase' = 0, scannedRoots' = scannedRoots },
    all { phase == 1, faults < 1, RemoveIdOnly("p4"), faults' = faults + 1, phase' = 0, scannedRoots' = scannedRoots },
    all { phase == 1, faults < 1, RemoveIdOnly("p5"), faults' = faults + 1, phase' = 0, scannedRoots' = scannedRoots },
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

  // A crash during the drain reboots the machine into the request phase and
  // drops the queued remove. The retried drain's scan rediscovers the deleted
  // path and the sweeps prune its rows, so the machine still reaches phase-2
  // quiescence with the invariant holding.
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-6
  // @scope SCOPE-13
  run completionCrashScanHealsTest = initCompletion
    .then(Stay(ExternalDelete("p3")))
    .then(Stay(SubmitWatcherRemove("p3")))
    .then(BeginDrain)
    .then(all { assert(phase == 1 and intake.size() == 1 and faults == 0), CrashReboot })
    .then(all { assert(phase == 0 and faults == 1 and intake.size() == 0 and not(Converged)), Skip })
    .then(BeginDrain)
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

  // A fault in the drain is a budgeted unclean restart: the process comes back
  // in the request phase, the environment resumes, and a second drain retries
  // the interrupted work. The crash drops the queued remove; the scan of the
  // retried drain rediscovers the deleted path and the sweep prunes its row.
  // @assumes ASS-3
  // @assumes ASS-6
  // @scope SCOPE-6
  // @scope SCOPE-13
  run drainCrashRecoveryTest = initCompletion
    .then(all { assert(phase == 0), Stay(ExternalDelete("p2")) })
    .then(all { assert(phase == 0), Stay(SubmitWatcherRemove("p2")) })
    .then(all { assert(phase == 0 and intake.size() == 1), Stay(SubmitScan("p1")) })
    .then(BeginDrain)
    .then(all { assert(phase == 1 and intake.size() == 2), Stay(WorkerSkipUnchanged) })
    .then(all { assert(phase == 1 and intake.size() == 1 and faults == 0), CrashReboot })
    .then(all { assert(phase == 0 and faults == 1 and intake.size() == 0 and not(Converged)), Skip })
    .then(BeginDrain)
    .then(BeginScan)
    .then(CompleteScan("r1"))
    .then(CompleteScan("r2"))
    .then(Stay(SweepEnumerate("r2")))
    .then(all { assert(sweepBatch.keys() == Set()), Stay(SweepEnumerate("r1")) })
    .then(all { assert(sweepBatch.keys() == Set("p2")), Stay(WorkerSkipUnchanged) })
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(WorkerSkipUnchanged))
    .then(Stay(SweepRecheck))
    .then(Stay(SweepRemoveBatchApply))
    .then(all { assert(Quiescent and Converged and AspectsAligned), Skip })

  // Two arbitrary ticks keep the state inside the declared bounds, the
  // aspect-table key coherent, and the phase machine in range.
  // @assumes ASS-1
  // @scope SCOPE-13
  run completionTwoStepTest = initCompletion
    .then(stepCompletion)
    .then(stepCompletion)
    .then(all { assert(WithinBounds and AssetIdKeyCoherent and phase >= 0 and phase <= 2 and scannedRoots.forall(r => r.in(ROOTS))), Skip })

}
