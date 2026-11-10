// Index pipeline shared state: the filesystem projection, the
// per-path submission versions, the intake queue, the redb row
// projection, and the initial state every run starts from.
// Imported by specs/eventual-consistency-assumptions.qnt (predicates
// over this state) and specs/eventual-consistency.qnt (actions and runs).
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

  type AssetRecord = {
    assetId: int,
    assetHash: int,
    modified: int,
    fileSize: int,
    sidecar: int,
    converged: bool,
    thumb: bool
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

  var files: str -> FileState
  var generations: str -> int
  var intake: Set[IntakeItem]
  var rows: str -> AssetRecord
  var sweepPending: str -> bool
  var nextAssetId: int
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
  pure def initialRow(p: str): AssetRecord = {
    assetId: initialId(p),
    assetHash: 0,
    modified: 0,
    fileSize: 0,
    sidecar: 0,
    converged: true,
    thumb: true
  }

  // @scope SCOPE-1: initial converged state; runs diverge from here.
  action init: bool = all {
    files' = PATHS.mapBy(p => initialFile(p)),
    generations' = PATHS.mapBy(_ => 0),
    intake' = Set(),
    rows' = PATHS.mapBy(p => initialRow(p)),
    sweepPending' = ROOTS.mapBy(_ => false),
    nextAssetId' = 6,
    unprocessable' = Set(),
  }
}
