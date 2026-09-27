# Goals and Design

## Basic Principles

### Galleries come and go - photos do not

- option to keep original photos totally untouched (R/O)
- keep added metadata in interoperable formats (adjust folders, add xmp sidecars,...?)
- sufficient to backup the photo repo, no worry about DB consistency/versioning

### Design around specific use cases and support them well

- personal photo gallery and backup
  - upload pre-processed photos directly to target albums (file sharing with global write access)
  - upload to configured ingress folder (automated via app or filesharing, sort/manage via gallery, no global write API)

- shared gallery with trusted family and friends
  - manage users and private/shared album access, including to ingress folder

- potentially extend to social streaming/sharing or even federated sharing?

### Keep it robust, low footprint, modular

- best practice devops: memory safe languages, static checks, code smell, depency audits
- backend fully self-contained, API supports scripting and alternate frontends
- frontend should focus on major supported use cases, clean and simple

- further processing/features can be done as sidecars working on backend API
  - analyze photos, add tags, make stories

## Detailed Design

### Filesystem and identity

The filesystem below `IMAGE_PATH` is the source of truth. The media files,
their sidecars, and the directory tree are the user's photo repository. An
image or video belongs to the album represented by its containing directory,
and an ordinary move changes that tree on disk.

The database, asset records, content hashes, thumbnails, and other generated
state are an index that helps the application search, group, and present the
repository. They must be rebuildable from the filesystem. A hash match
identifies files that may contain the same bytes; it does not establish user
intent or give one file authority over another. Each indexed media file and
its sidecar remains a distinct filesystem item, even when several files share
a hash.

The distinction between these operations is important:

- **Index/ingest** discovers files and never moves or deletes existing files.
- **Move** moves the selected media file and its sidecar to another album.
  On filename conflict the file is either left in place (`skip`) or moved
  under a unique name (`rename`). No file is ever overwritten.

The backend manages metadata and thumbnails in `STATE_HOME`, and writes
user-authored photo metadata back to the repository as sidecar XMP files.
Moving or copying a photo moves or copies its sidecar with it. `.albuminfo` is
an optional, app-specific helper stored next to a directory; it customizes the
presentation of that folder but does not define the folder, its membership, or
its identity. Losing or rebuilding `.albuminfo` must not lose photos.

#### Metadata precedence and lifecycle

Metadata is resolved from the repository, never from the generated database:

1. Read metadata embedded in the raw media file.
2. Apply the paired sidecar as a partial overlay. Fields present in the sidecar
   override the raw value; fields absent from the sidecar inherit the raw value.
   An omitted field must not clear the raw value.
3. Store only the resolved merged view in the database. The database is a
   rebuildable cache and is never the source of truth.
4. Frontend metadata edits are explicit user intent. By default they create or
   update the sidecar and do not modify the raw media file.
5. Merging selected metadata back into the raw file is a separate explicit
   action. It must preserve unmanaged metadata, use an atomic replacement, and
   leave the sidecar as the authoritative overlay if the format cannot be
   written safely.

Deleting the database and reindexing reconstructs the same view from raw files
and sidecars. Deleting a sidecar removes the overlay and exposes the raw
metadata again; it does not delete metadata from the raw file. External raw
changes are visible for every field not overridden by the sidecar.

Metadata the app does not model is a category of its own, not a fallback:
`furtherMetadata` on the detail response holds it as `Group:Tag` key/value
pairs — read-only, with no edit path, because the app does not know what those
fields mean well enough to write one back. The sidebar renders it as one
section; which `ExifTool` groups reach it is documented on
`process::xmp::map_further_fields`.

Database updates and filesystem operations are not one atomic transaction. A
journal records the intended filesystem operation and its progress; startup
recovery and indexing rebuild or reconcile generated state from the repository.
No operation may report success until its required filesystem changes are
durable, and a partial operation must remain visible for repair rather than
silently deleting user files.

#### Metadata extraction pipeline

Image metadata is read by exactly one engine, `ExifTool`, run as an external
process. No in-process metadata reader is left — the EXIF-only Rust crate and
the hand-rolled XMP byte scan are gone — and no hand-written metadata parser
may come back: `ExifTool` locates the container (`APP1`, `APP13`, PNG text
chunks, `zTXt`/`iTXt` compression included) and the backend's job is to decide
which file to read, invoke the tool, and map what it reports. Video keeps the
separate `ffprobe`/`ffmpeg` path.

`ExifTool` is pure Perl and its startup costs an order of magnitude more than
the read it performs, so the engine runs as a persistent `-stay_open` session,
one per calling thread. That count is bounded by the index worker pool, because
the indexer is the only caller. A session whose child dies is replaced and the
read retried once; a session that cannot be started at all is never cached, so
a missing binary is reported on every read instead of latching as a dead
session. A failed read is absorbed into an empty map — a damaged image must
still index — so each of those failures is logged with the install remedy, and
the toolchain's presence is a test precondition rather than a silent skip.

**One read per file.** The single grouped record (`-G1`, so every tag arrives
under its family, with a fixed date format) is the currency, and it is projected
three ways: the `exifVec` map keeps the EXIF family alone, the natively modelled
fields take the XMP, IIM and text families, and the read-only bucket is the
complement of that second projection. Reading a file twice would double the
metadata cost of indexing it, which at library scale is the dominant cost.

A sidecar is the only second read, because it is a second file. Its
_existence_ takes the XMP source away from the image whether or not it parses:
the sidecar is where the app writes metadata back, so falling back to the
packet still inside the image would undo the edit. Its XMP replaces the image's
own packet, while the image's IPTC and text chunks keep filling what that
packet left empty. A value that is present but blank does not count as supplied,
so a cleared field cannot shadow a family that still carries it.

Within the native fields the first non-empty value wins in the order
**XMP > IPTC IIM > PNG text**; the per-field order and the keyword union are
documented on `process::xmp::map_native_fields`.

#### Identity and serving invariants

An `assetId` identifies one physical indexed file and is used for asset
records, storage lookups, and asset-token maps. A content hash identifies the
bytes and is used for content-addressed compressed objects and hash-bound
serving URLs. They are both stable strings at the HTTP boundary but are not
interchangeable values. A content hash must never be substituted with an
`assetId` when constructing a compressed-object URL or `GuardHash` claim.

The public HTTP API is an interface for the backend and alternate frontends,
not only an implementation detail of the shipped SPA. Its generated OpenAPI
description, mounted routes, request/response schemas, security behavior, and
error responses must describe the same interface. Changes to one of these
surfaces require the others to be regenerated or checked together.

### Importing and filesystem synchronization

- Basic functions:

  - Single image indexing via `index_image(src, dst)`
    - src path relative to `IMAGE_HOME`
    - dst path is optionally assigned target folder (album) relative to `IMAGE_HOME`

  - Folder indexing via `index_path(src)`
    - src path must be relative to `IMAGE_HOME`
    - loop recursively over src path and execute `index_image(src)` on every image file

- On re-indexing, images with an existing hash may be grouped in the generated
  index and share thumbnails. Every physical file remains visible in its
  actual album. Its sidecar remains paired with that file; indexing must not
  overwrite one file's sidecar or metadata with another file's metadata.

- A watcher or upload that encounters an existing hash still adds the physical
  file by default. It must not silently discard a file merely because it is a
  duplicate: filesystem sync tools can intentionally maintain copies and can
  re-send a file after a temporary disappearance.

- Missing or changed files discovered by a watcher are reconciled after a
  debounce period. A sync tool may temporarily remove or replace a path while
  transferring it. A changed media file is treated as removal of the old
  indexed item followed by indexing the new file; it is never an in-place
  mutation of the existing record. The event is recorded as a filesystem
  change and surfaced to the user; it is not treated as permission to delete
  another indexed file or its metadata.

- The duplicate view groups files by hash and shows their paths, albums,
  sidecars, and metadata differences. Per-group cleanup actions are an explicit,
  separate operation with preview and recovery; they must never run as a side
  effect of moving or uploading files.

### Moving / Deleting

- User may move a selected file or selection of files to another album. The
  selected physical files and their sidecars move under `IMAGE_PATH`; other
  same-hash files remain untouched. A normal move never deduplicates or
  deletes another file as a side effect.

- Filename conflicts never overwrite bytes or sidecars. `skip` leaves both
  source and destination untouched. `rename` moves the source under a
  deterministic unique name (`photo-001.jpg`, then `photo-002.jpg`) and reports
  the final path.

- A selected file may be deleted explicitly. The file and its sidecar are
  removed only after confirmation. Generated records and thumbnails may remain
  only as temporary cleanup state; the filesystem is authoritative.

- Moving a directory into a target with the same child name is a single
  `fs::rename` operation. `skip` leaves both source and target untouched.
  `rename` moves the source directory under a unique sibling name. No recursive
  directory merge or file flattening occurs. The source directory is always
  moved as one unit.

- Directory indexing and the watcher do not move files. They reconcile the
  index with the filesystem and report changes for explicit operations.

- When an indexed file is removed, watcher removal handling resolves the
  recorded path through the path index and prunes the asset record, duplicate
  membership, and derived state. It does not compute a content hash to find a
  replacement record and does not remove another same-hash file.

- Manual indexing performs the same stale-path reconciliation under the
  indexed directory after scanning for new files. A moved or renamed file is
  reconciled by path without recomputing unrelated thumbnails. The sidecar is
  part of the physical file and stays paired with it.

- Scheduled reconciliation and hash verification remain useful operational
  safeguards, but they are separate from watcher and manual-index behavior.

### Album Properties

- Initial album names are derived from the respective path names
- Users may move albums, set the pretty name and set the album image
- Album properties are saved per directory in a file .albuminfo:
  albumimage = path/to/image
  albumname = pretty name
  albumnotes = {markdown text?}

### Photo Properties

- photo properties are managed in sidecar files: {basename}.{ext}.xmp
- sidecar files are are moved together with the original file
- Customizable data/dialogs:
  - tags/labels
  - description
  - rating
  - ...?

## Further ideas

- photo app should work as local gallery with cache
- integrate/interoperate port knocking

- stories - generate virtual albums (tags?) based on similar location/date
- streams - support social streaming and sharing endpoints (activitypub? chatbots?)

- storage and transfer optimizations
  - push compressed images on mobile, replace with high-res original later
  - report storage per album, keep track of raw vs post-processed/compressed
  - offer some reasonable default compression ratios
  - detect duplicate / redundant, offer to select best
