// The index pipeline goal: eventual convergence of the redb
// projection to the filesystem. This is the subsystem-specific part
// of the spec — the state (eventual-consistency-state.qnt) and the
// assumption predicates (eventual-consistency-assumptions.qnt) are
// generic and reusable across models. Imported by
// utils/quint/pipeline/eventual-consistency.qnt, whose runs assert it per diverged
// shape.
module eventual_consistency_goal {
  import eventual_consistency_state.* from "./eventual-consistency-state"

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
  ) and intake.size() == 0 and albumPending.size() == 0
}