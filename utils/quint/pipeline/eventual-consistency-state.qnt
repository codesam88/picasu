// Index pipeline shared state: the filesystem projection, the
// per-path submission versions, the intake queue, the per-asset aspect
// tables (identity, path map, dupe index, metadata payload, thumbnail
// presence, album pending), and the initial state every run starts from.
// Imported by utils/quint/pipeline/eventual-consistency-assumptions.qnt (predicates
// over this state) and utils/quint/pipeline/eventual-consistency.qnt (actions and runs).
module eventual_consistency_state {
  // @scope SCOPE-2: finite bounds; every run stays inside them.
  pure val PATHS = Set("p1", "p2", "p3", "p4", "p5")
  // @scope SCOPE-2
  pure val HASHES = 0.to(2)
  // @scope SCOPE-2
  pure val SMALL = 0.to(2)

  // @scope SCOPE-2
  pure val ROOTS = Set("r1", "r2")
  // @scope SCOPE-2
  pure def rootOf(p: str): str = if (p == "p5") "r2" else "r1"

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
  // tiebreak, excluded from Converged per the goal module).
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
  // albumPending maps to PENDING_ALBUM_UPDATES (aspect (h) pending set).
  var albumPending: Set[int]
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
    itemCount: 0,
    itemSize: 0,
    hasShareList: false
  }

  // @scope SCOPE-1: initial converged state; runs diverge from here.
  action init: bool = all {
    files' = PATHS.mapBy(p => initialFile(p)),
    generations' = PATHS.mapBy(_ => 0),
    intake' = Set(),
    assetById' = Map(0 -> initialAlbumAssetRecord, 1 -> initialAssetRecord("p1"), 2 -> initialAssetRecord("p2"), 3 -> initialAssetRecord("p3"), 4 -> initialAssetRecord("p4"), 5 -> initialAssetRecord("p5")),
    assetByPath' = Map("p1" -> 1, "p2" -> 2, "p3" -> 3, "p4" -> 4, "p5" -> 5),
    dupeIndex' = Map(0 -> Set(1, 2, 3, 4, 5), 1 -> Set(), 2 -> Set()),
    metadataPayload' = Map(0 -> initialAlbumMetadataRecord, 1 -> initialMetadataRecord("p1"), 2 -> initialMetadataRecord("p2"), 3 -> initialMetadataRecord("p3"), 4 -> initialMetadataRecord("p4"), 5 -> initialMetadataRecord("p5")),
    thumbPresent' = Map(0 -> true, 1 -> false, 2 -> false),
    albumPending' = Set(),
    sweepPending' = ROOTS.mapBy(_ => false),
    sweepBatch' = Map(),
    leasedPaths' = Set(),
    nextAssetId' = 6,
    unprocessable' = Set(),
  }
}