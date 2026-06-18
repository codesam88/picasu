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

Each image or video belongs to exactly one album, which corresponds to the
directory it lives in on disk. The album hierarchy is the filesystem directory
tree under the single configured `imagePath` root.

Consequence: moving a file to a different album moves it on disk. Refreshing
the frontend after a filesystem move reflects the change without re-indexing.

Source of truth is the actual file repo at `IMAGE_PATH`. The backend will
manage metadata and thumbnails in (currently in `DATA_HOME`, soon to be
`STATE_HOME`) but also always write back added metadata to the photo repository
in form of sidecar XMP files.

Duplicate images are detected based on their original image file hash.

Consistency is ensured by locking the DB + file access change in a
common transaction, with associated journal.

### Importing Images

- Basic functions:

  - Single image indexing via `index_image(src, dst)`
    - src path relative to `IMAGE_HOME`
    - dst path is optionally assigned target folder (album) relative to `IMAGE_HOME`

  - Folder indexing via `index_path(src)`
    - src path must be relative to `IMAGE_HOME`
    - loop recursively over src path and execute `index_image(src)` on every image file

- On re-indexing, images with existing/known hash become aliases of the same
  internal image representation. Same DB entry and thumbnails. But another
  existing alias image may have an existing sidecar XMP with customized metadata
  Options:
  - [ ] ignore the duplicate file, but since it exists in the FS we would be hiding it
  - [ ] copy and synchronize the metadata...but then we must do this consistently
  - [x] ignore the possible drifted metadata and allow drifted sidecar files

- Watcher or image upload encounters image with same hash...
  So the image and same metadata are known, and the existing images may have
  additional metadata assigned and stored in their sidecar files

  Options:
  - [/] discard the file as duplicate - on interactive use, not useful for watcher?
  - [x] add the file, thumb/db will refer to same existing hash, allow tags/album to drift
  - [ ] add the file and synchronize metadata between sidecar files (bit crazy?)

- Managing duplicate/drifted files?
  - Keep a list of detected duplicates, report to user
  - Button to globally merge aliased files in same album (unique per album)
  - Button to globally remove aliased files without sidecar (keep modified)
  - Button to globally remove duplicates with no or duplicate sidecar (keep oldest unique)

### Moving / Deleting

- User may reassign image or selection of images to another album
  - also moves the underlying original file to the respective dir under `IMAGE_PATH`
  - option to auto-rename files if target already exists but has different hash (else skip)
  - option to auto-replace files if target already exists and has same hash (else skip)

- User may delete files via API
  - we know the image and hash, can remove associated context if its the last alias

- Directory indexing or watcher do not move files

- Directory indexing or watcher may also encounter deleted files.
  When the last image alias is removed, the thumbnails should be removed. But if the
  last alias is removed by file access, we cannot compute its hash anymore.
  - the watcher may notice delete operations and can lookup the file in DB,
    delete the associated metadata and thumbnails

  - On manual indexing, consult the DB for the selected target path and check
    if all known files exist. Delete thumbnails and metadata for any removed files.
    This cleanup sweep should be done after scanning for any new files, so that
    moved/renamed files only result in alias remapping and not require recomputing
    all thumbnails. note that sidecar files are not transferred between aliases as by above.
  - A semi-regular cleanup sweep could be done on schedule. Could also verify hashes.

  - Discovery of deleted files or changed hashes should be logged...may point
    to corruption/loss...

  - use occasional sweeps to ensure images are there...could also verify hashes

  - for any accessed thumbnail, use DB to test existance of the original image?

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
  - favorite
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
  - merge metadata back into originals
