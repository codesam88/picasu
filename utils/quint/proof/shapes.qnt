// Shape catalog for the G1 covering lemma.
//
// Each shape is one negated conjunct of `Converged and AspectsAligned`,
// projected over the reduced instance (3 paths / 2 roots). Shapes do not
// mention `generations`: the goal never reads generation values, and the
// heal module fixes generations separately.
//
// Notation: `id = assetByPath.get(p)` when the path row exists and
// `h = assetById.get(id).contentHash` when the asset record exists. Reads are
// guarded the same way `Converged` and `AspectAligned` guard theirs.
module shapes {
  import pipeline_merged(
    PATHS = Set("p1", "p2", "p3"),
    HASHES = 0.to(1),
    SMALL = 0.to(1),
    SECOND_ROOT_PATHS = Set("p3"),
    SUBTREE_PAIRS = [{ src: "p1", dst: "p3" }],
  ).* from "../pipeline/merged"

  // MissingRow: the file is present but no path row exists.
  def MissingRow(p: str): bool =
    files.get(p).present and not(assetByPath.keys().contains(p))

  // GhostRow: the path row outlives an absent file.
  def GhostRow(p: str): bool =
    not(files.get(p).present) and assetByPath.keys().contains(p)

  // TornRow: a present file has a path row whose asset record is gone.
  def TornRow(p: str): bool =
    files.get(p).present and
    assetByPath.keys().contains(p) and
    not(assetById.keys().contains(assetByPath.get(p)))

  // WrongPath: the asset record exists but names a different path.
  def WrongPath(p: str): bool =
    files.get(p).present and
    assetByPath.keys().contains(p) and
    assetById.keys().contains(assetByPath.get(p)) and
    assetById.get(assetByPath.get(p)).path != p

  // StaleContent: the record's content hash differs from the file bytes.
  def StaleContent(p: str): bool =
    files.get(p).present and
    assetByPath.keys().contains(p) and
    assetById.keys().contains(assetByPath.get(p)) and
    assetById.get(assetByPath.get(p)).contentHash != files.get(p).assetHash

  // MissingMeta: the path row has no metadata payload.
  def MissingMeta(p: str): bool =
    assetByPath.keys().contains(p) and
    not(metadataPayload.keys().contains(assetByPath.get(p)))

  // StaleMetaHash: metadata content snapshot disagrees with the file bytes.
  def StaleMetaHash(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    metadataPayload.get(assetByPath.get(p)).assetHash != files.get(p).assetHash

  // StaleMtime: metadata mtime disagrees with the file mtime.
  def StaleMtime(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    metadataPayload.get(assetByPath.get(p)).modified != files.get(p).mtime

  // StaleSize: metadata file size disagrees with the file length.
  def StaleSize(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    metadataPayload.get(assetByPath.get(p)).fileSize != files.get(p).len

  // StaleSidecar: metadata sidecar id disagrees with the file sidecar.
  def StaleSidecar(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    metadataPayload.get(assetByPath.get(p)).sidecar != files.get(p).sidecar

  // NotConvergedFlag: the metadata converged flag is still unset.
  def NotConvergedFlag(p: str): bool =
    assetByPath.keys().contains(p) and
    metadataPayload.keys().contains(assetByPath.get(p)) and
    not(metadataPayload.get(assetByPath.get(p)).converged)

  // MissingThumb: a media record (contentHash >= 0) has no thumbnail.
  def MissingThumb(p: str): bool =
    assetByPath.keys().contains(p) and
    assetById.keys().contains(assetByPath.get(p)) and
    assetById.get(assetByPath.get(p)).contentHash >= 0 and
    (not(thumbPresent.keys().contains(assetById.get(assetByPath.get(p)).contentHash)) or
      not(thumbPresent.get(assetById.get(assetByPath.get(p)).contentHash)))

  // MissingDupe: a media record (contentHash >= 0) is absent from its
  // content-hash bucket in the dupe index.
  def MissingDupe(p: str): bool =
    assetByPath.keys().contains(p) and
    assetById.keys().contains(assetByPath.get(p)) and
    assetById.get(assetByPath.get(p)).contentHash >= 0 and
    (not(dupeIndex.keys().contains(assetById.get(assetByPath.get(p)).contentHash)) or
      not(dupeIndex.get(assetById.get(assetByPath.get(p)).contentHash).contains(assetByPath.get(p))))

  // QueuedIntake: the intake queue is not empty.
  val QueuedIntake: bool = intake.size() > 0

  // ShapeCatalog: some catalog shape holds for a processable path, or work is
  // still queued. PendingSweep and LeaseHeld are not goal shapes; they belong
  // to the Quiescent antecedent.
  val ShapeCatalog: bool =
    QueuedIntake or
    PATHS.exists(p =>
      not(p.in(unprocessable)) and (
        MissingRow(p) or
        GhostRow(p) or
        TornRow(p) or
        WrongPath(p) or
        StaleContent(p) or
        MissingMeta(p) or
        StaleMetaHash(p) or
        StaleMtime(p) or
        StaleSize(p) or
        StaleSidecar(p) or
        NotConvergedFlag(p) or
        MissingThumb(p) or
        MissingDupe(p)
      )
    )

  // Covering: every violation of the goal is explained by the catalog.
  val Covering: bool =
    not(Converged and AspectsAligned) implies ShapeCatalog
}
