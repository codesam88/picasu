import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/// Share creation lives in the album context menu, not the toolbar: the
/// batch menu owns a "Share album" entry beside "Album Info" with the same
/// single-selected-album gating, and mounts the create dialog for the
/// targeted album, while GalleryBar keeps no share affordance. .vue glue
/// has no mount harness under node vitest, so the placement contract is
/// pinned structurally (same convention as userManagement.test.ts).
const batchMenu = readFileSync(
  resolve(import.meta.dirname, '../../components/Menu/BatchMenu.vue'),
  'utf-8'
)
const galleryBar = readFileSync(
  resolve(import.meta.dirname, '../../components/NavBar/GalleryBars/GalleryBar.vue'),
  'utf-8'
)
const shareItem = readFileSync(
  resolve(import.meta.dirname, '../../components/Menu/MenuItem/ItemShareAlbum.vue'),
  'utf-8'
)

describe('Share album placement', () => {
  test('batch menu offers Share album beside Album Info', () => {
    expect(batchMenu).toMatch(/ItemShareAlbum/)
    expect(batchMenu).toMatch(/CreateShareModal/)
    expect(batchMenu).toMatch(/modalStore\.shareAlbumId/)
  })

  test('the share entry is admin-only and targets the dialog at its album', () => {
    expect(shareItem).toMatch(/Share album/)
    expect(shareItem).toMatch(/mdi-share-variant/)
    expect(shareItem).toMatch(/canSeeUserPanel|isAdmin/)
    expect(shareItem).toMatch(/shareAlbumId/)
    expect(shareItem).toMatch(/showShareModal = true/)
    expect(shareItem).toMatch(/singleSelectedAlbumId|isSingleAlbumSelected/)
  })

  test('GalleryBar keeps no share button and mounts no share dialog', () => {
    expect(galleryBar).not.toMatch(/mdi-share-variant/)
    expect(galleryBar).not.toMatch(/showShareModal/)
    expect(galleryBar).not.toMatch(/CreateShareModal/)
  })
})
