import * as fs from 'fs'
import * as path from 'path'
import { createHash } from 'crypto'
import { spawnSync } from 'child_process'
import { REPO_ROOT } from './paths'

/**
 * Placement of the pinned binary fixtures the UI scenarios upload.
 *
 * `executeGiven.ts` generates its source files through `snapfab batch`, whose
 * `format` is a `jpeg | png` enum, so the formats snapfab cannot encode
 * (TIFF, WebP, MP4, MOV) cannot be placed through the UI harness at all. This
 * is the binary-safe counterpart of that generator: a scenario names an entry
 * of the capability manifest and the checked-in bytes are copied instead, so
 * no binary payload is ever embedded in a scenario. It mirrors the backend
 * harness's `fixture` given step (`backend/src/tests/backend_api.rs`), the only
 * difference being the destination: the UI flow uploads the file through the
 * browser file chooser, so the bytes have to sit *outside* `IMAGE_HOME` or the
 * item would be indexed before the scenario ever uploads it.
 */

const MANIFEST_PATH = path.join(REPO_ROOT, 'utils', 'snapfab', 'capabilities.json')

interface ManifestFixture {
  id: string
  path: string
  sha256: string
}

interface Manifest {
  fixtures: ManifestFixture[]
  formats: {
    format: string
    extensions: string[]
    pinnedFixtures?: string[]
    metadataFields?: Record<string, string[]>
  }[]
}

function readManifest(): Manifest {
  let raw: string
  try {
    raw = fs.readFileSync(MANIFEST_PATH, 'utf-8')
  } catch (error) {
    throw new Error(
      `Cannot read the capability manifest at ${MANIFEST_PATH}. ` +
        `The pinned-fixture given step needs it to resolve a fixture id to bytes.`,
      { cause: error }
    )
  }
  return JSON.parse(raw) as Manifest
}

/** The manifest format entry that pins `id`. */
function pinnedFormat(
  manifest: Manifest,
  id: string
): { format: string; extensions: string[]; metadataFields?: Record<string, string[]> } {
  const format = manifest.formats.find((f) => (f.pinnedFixtures ?? []).includes(id))
  if (!format) {
    throw new Error(
      `Fixture id \`${id}\` is not pinned by any format in the capability manifest ` +
        `(${MANIFEST_PATH}), so the extensions it may be uploaded under are unknown.`
    )
  }
  return format
}

/**
 * Fail with a toolchain diagnostic when a video fixture is placed without a
 * working ffmpeg/ffprobe.
 *
 * The manifest is the source of truth for the dependency: a format whose
 * `metadataFields.container` is `probe` is read by the backend by shelling out
 * to ffprobe (dimensions, the metadata map) and ffmpeg (thumbnail). Neither has
 * a pure-Rust fallback: `process::video` spawns `ffprobe` for dimensions and
 * the metadata map and `ffmpeg` for the thumbnail. Without them the upload is
 * rejected and the scenario fails several steps later with "could not be
 * decoded as an image or video", which reads as a product bug rather than a
 * missing binary.
 */
function requireVideoToolchain(format: string): void {
  for (const tool of ['ffmpeg', 'ffprobe']) {
    const result = spawnSync(tool, ['-version'], { encoding: 'utf-8' })
    if (result.error || result.status !== 0) {
      const detail = result.error
        ? result.error.message
        : `\`${tool} -version\` exited with status ${result.status}`
      throw new Error(
        `\`${tool}\` is not usable, and a \`${format}\` fixture needs it: the video ` +
          `pipeline shells out to ffmpeg for the thumbnail and ffprobe for the container ` +
          `metadata this scenario asserts in the UI. ${detail}. Install ffmpeg (it ships ` +
          `both binaries) and re-run.`
      )
    }
  }
}

/**
 * Copy the checked-in bytes of manifest fixture `id` to `destination`.
 *
 * The recorded SHA-256 is verified against the bytes before they are written.
 * `cargo test -p snapfab` already pins that digest, but this harness reads the
 * manifest file directly rather than going through snapfab's loader, and a
 * truncated or corrupted fixture (a partial checkout, a disk-full write) would
 * otherwise surface as an unexplained upload or decode failure much later.
 *
 * The destination extension must be one the pinned format declares, so a
 * scenario cannot upload TIFF bytes named `.jpg` and have the format assertions
 * pass on the wrong extension.
 */
export function copyPinnedFixture(id: string, destination: string): string {
  const manifest = readManifest()
  const fixture = manifest.fixtures.find((f) => f.id === id)
  if (!fixture) {
    throw new Error(
      `Fixture id \`${id}\` is not declared in the capability manifest (${MANIFEST_PATH}). ` +
        `Declared ids: ${manifest.fixtures.map((f) => f.id).join(', ')}.`
    )
  }

  const source = path.join(REPO_ROOT, fixture.path)
  const bytes = fs.readFileSync(source)
  const digest = createHash('sha256').update(bytes).digest('hex')
  if (digest !== fixture.sha256) {
    throw new Error(
      `Fixture \`${id}\` at ${source} does not match the SHA-256 recorded in the ` +
        `capability manifest (recorded ${fixture.sha256}, found ${digest}). The checked-in ` +
        `bytes were modified or truncated; re-pin the fixture in utils/snapfab/capabilities.json.`
    )
  }

  const format = pinnedFormat(manifest, id)
  const extension = path.extname(destination).slice(1).toLowerCase()
  if (!format.extensions.includes(extension)) {
    throw new Error(
      `Fixture \`${id}\` is a ${format.extensions.join('/')} file, but the scenario uploads it as ` +
        `\`${destination}\` (extension \`${extension}\`). Give the source file an extension ` +
        `the pinned format declares.`
    )
  }
  if ((format.metadataFields?.container ?? []).includes('probe')) {
    requireVideoToolchain(format.format)
  }

  fs.mkdirSync(path.dirname(destination), { recursive: true })
  fs.writeFileSync(destination, bytes)
  return destination
}
