// Shared state slice for the local protocol proofs (`.plan/quint-proof-lane-kimi.md` §9).
//
// Action-free module: record types, state variables, local instance bounds,
// and predicates copied from `pipeline/merged.qnt` (module `pipeline_merged`).
// Each copy carries a `// @mirrors-merged` comment with its source line.
// Action modules import this module and add their own init/step/invariants.
module local_common {
  // @mirrors-merged merged.qnt:37-45 FileState
  type FileState = {
    present: bool,
    ver: int,
    len: int
  }

  // @mirrors-merged merged.qnt:55-66 AssetRecord
  type AssetRecord = {
    assetId: int,
    kind: str,
    path: str,
    contentHash: int,
    albumId: int,
    isTrashed: bool
  }

  // @mirrors-merged merged.qnt:71-79 MetadataRecord
  type MetadataRecord = {
    kind: str,
    ver: int,
    fileSize: int,
    converged: bool,
    itemCount: int,
    itemSize: int,
    hasShareList: bool
  }

  // @mirrors-merged merged.qnt:81-86 IntakeItem
  type IntakeItem = {
    path: str,
    generation: int,
    kind: str,
    ver: int,
    len: int
  }

  // @mirrors-merged merged.qnt:91 files
  var files: str -> FileState
  // @mirrors-merged merged.qnt:93 generations
  var generations: str -> int
  // @mirrors-merged merged.qnt:95 intake
  var intake: Set[IntakeItem]
  // @mirrors-merged merged.qnt:97 assetById
  var assetById: int -> AssetRecord
  // @mirrors-merged merged.qnt:99 assetByPath
  var assetByPath: str -> int
  // @mirrors-merged merged.qnt:101 dupeIndex
  var dupeIndex: int -> Set[int]
  // @mirrors-merged merged.qnt:103 metadataPayload
  var metadataPayload: int -> MetadataRecord
  // @mirrors-merged merged.qnt:105 thumbPresent
  var thumbPresent: int -> bool
  // @mirrors-merged merged.qnt:107 sweepPending
  var sweepPending: str -> bool
  // @mirrors-merged merged.qnt:109 sweepBatch
  var sweepBatch: str -> int
  // @mirrors-merged merged.qnt:111 leasedPaths
  var leasedPaths: Set[str]
  // @mirrors-merged merged.qnt:113 nextAssetId
  var nextAssetId: int
  // @mirrors-merged merged.qnt:115 unprocessable
  var unprocessable: Set[str]

  // Local instance bounds: three paths on two roots (p1 on r1, p2/p3 on r2).
  pure val PATHS = Set("p1", "p2", "p3")
  pure val SECOND_ROOT_PATHS = Set("p2", "p3")
  pure val ROOTS = Set("r1", "r2")
  // @mirrors-merged merged.qnt:37 rootOf
  pure def rootOf(p: str): str = if (p.in(SECOND_ROOT_PATHS)) "r2" else "r1"

  // @mirrors-merged merged.qnt:118-119 initialId
  pure def initialId(p: str): int =
    if (p == "p1") 1 else (if (p == "p2") 2 else (if (p == "p3") 3 else (if (p == "p4") 4 else 5)))

  // @mirrors-merged merged.qnt:129-137 initialAssetRecord
  pure def initialAssetRecord(p: str): AssetRecord = {
    assetId: initialId(p),
    kind: "media",
    path: p,
    contentHash: 0,
    albumId: 0,
    isTrashed: false
  }

  // @mirrors-merged merged.qnt:139-147 initialMetadataRecord
  pure def initialMetadataRecord(p: str): MetadataRecord = {
    kind: "media",
    ver: 0,
    fileSize: 0,
    converged: true,
    itemCount: 0,
    itemSize: 0,
    hasShareList: false
  }

  // @mirrors-merged merged.qnt:150-158 initialAlbumAssetRecord
  pure def initialAlbumAssetRecord: AssetRecord = {
    assetId: 0,
    kind: "album",
    path: "",
    contentHash: -1,
    albumId: -1,
    isTrashed: false
  }

  // @mirrors-merged merged.qnt:160-169 initialAlbumMetadataRecord
  pure def initialAlbumMetadataRecord: MetadataRecord = {
    kind: "album",
    ver: 0,
    fileSize: 0,
    converged: true,
    itemCount: PATHS.size(),
    itemSize: 0,
    hasShareList: false
  }

  // No permanent staleness: a present file either matches its row or has work
  // queued; an absent file either has no row, has removal queued, or awaits
  // the root's sweep. Unprocessable paths are excluded (logged, never indexed).
  // @mirrors-merged merged.qnt:281-299 NoPermanentStale
  val NoPermanentStale = PATHS.forall(p =>
    p.in(unprocessable) or
    if (files.get(p).present) {
      if (assetByPath.keys().contains(p)) {
        (metadataPayload.keys().contains(assetByPath.get(p)) and metadataPayload.get(assetByPath.get(p)).ver == files.get(p).ver and metadataPayload.get(assetByPath.get(p)).fileSize == files.get(p).len) or
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
}
