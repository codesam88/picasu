// Index pipeline assumptions: each assumption beside its formal
// counterpart. The prose states what the model takes as given; the
// formal element — or an explicit pointer to where it lives — shows
// how the model relies on it. An assumption with no formal element
// of its own says so instead of implying coverage.
// utils/quint/pipeline/assumptions.json keeps the index of IDs with owner and status;
// model refs come from the generated traceability map. Purpose, scope,
// and writing rules live in utils/quint/README.md.
module eventual_consistency_assumptions {
  import eventual_consistency_state.* from "./eventual-consistency-state"

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
  // by path (see eventual-consistency-state.qnt). Asserted per distinct
  // identity by run copyDistinctIdsTest. No predicate here: identity IS
  // the keying.

  // ASS-2: Content has its own identifier.
  //
  // The asset's original content is the file content (image, video). A
  // blake128 content hash makes a unique content identifier assetHash.
  // The combined content is uniquely identified by its assetHash.
  //
  // Compared, never assumed equal: assetHash is compared field-by-field
  // in snapshotMatches below, in WorkerSkipUnchanged, and in Converged
  // (see eventual-consistency-goal.qnt). Uniqueness beyond finite
  // bounds is unstatable here: the small HASHES domain admits
  // collisions, so uniqueness rests on the hash function, not the model.

  // ASS-3: The filesystem is the source of truth.
  //
  // The index (rows plus derived artifacts) is a rebuildable cache over
  // the watched trees. On any disagreement between the index and the
  // filesystem, the filesystem wins and the index converges to it. Rows
  // converge to the observed state; metadata that existed only in a
  // removed sidecar is lost.
  //
  // Formalized as direction: Converged constrains rows to files, never
  // the reverse (see eventual-consistency-goal.qnt); SweepStalePaths
  // keeps only present files; ExternalSidecarDelete zeroes the sidecar
  // (see eventual-consistency.qnt); run sidecarDeleteConvergesTest.

  // ASS-4: The derived store is pipeline-private.
  //
  // Derived artifacts are produced by the pipeline and read during
  // serving. A missing artifact is rebuilt by processing: the modeled
  // skip action requires the thumbnail present, and only reprocessing
  // restores it. Walk roots are disjoint from the
  // derived store, so pipeline writes cannot resubmit themselves;
  // see the intake header and WorkerApplySuccess in
  // eventual-consistency.qnt; derived bytes (thumbnails) are written by
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
    ((files.get(ii.path).present) and (rows.keys().contains(ii.path)) and
     (rows.get(ii.path).assetHash == files.get(ii.path).assetHash) and
     (rows.get(ii.path).modified == files.get(ii.path).mtime) and
     (rows.get(ii.path).fileSize == files.get(ii.path).len) and
     (rows.get(ii.path).sidecar == files.get(ii.path).sidecar)) or
    ((not(files.get(ii.path).present)) and (not(rows.keys().contains(ii.path))))
    )
  )

  // No permanent staleness: a present file either matches its row, has work
  // queued; an absent file either has no row, has removal queued, or awaits
  // the root's sweep. Unprocessable paths are excluded (logged, never indexed).
  // @assumes ASS-6
  val NoPermanentStale = PATHS.forall(p =>
    p.in(unprocessable) or
    if (files.get(p).present) {
      if (rows.keys().contains(p)) {
        (rows.get(p).assetHash == files.get(p).assetHash and rows.get(p).modified == files.get(p).mtime and rows.get(p).fileSize == files.get(p).len and rows.get(p).sidecar == files.get(p).sidecar) or
        intake.exists(i => i.path == p)
      } else {
        intake.exists(i => i.path == p)
      }
    } else {
      (not(rows.keys().contains(p))) or
      intake.exists(i => i.path == p and i.kind == "remove") or
      sweepPending.get(rootOf(p))
    }
  )
}
