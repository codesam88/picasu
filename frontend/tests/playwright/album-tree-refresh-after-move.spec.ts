import { expect } from '@playwright/test'
import { test } from './scenarioFixtures'
import { executeGiven, createGivenContext, resetAuthToken } from './executeGiven'
import type { GivenItem } from './types'

test.describe('Album tree refresh after move', () => {
  test('album tree and grid update after assign_album move via API', async ({
    page,
    request,
    backendPaths
  }) => {
    resetAuthToken()
    const ctx = createGivenContext('e2e_tree_refresh')

    const givenItems: GivenItem[] = [
      { dir_album: '/source/child', id_as: '$child_album' },
      { photo: '/source/child/photo.jpg', id_as: '$photo' },
      { dir_album: '/target', id_as: '$target_album' },
      { dir_album: '/source', id_as: '$source_album' }
    ]

    const seeded = await executeGiven(request, givenItems, ctx, undefined, backendPaths)
    const child_album = seeded.vars['$child_album']
    const target_album = seeded.vars['$target_album']
    const source_album = seeded.vars['$source_album']

    // Authenticate
    const authRes = await request.post(`${backendPaths.BACKEND_URL}/post/authenticate`, {
      data: JSON.stringify(backendPaths.ADMIN_PASSWORD),
      headers: { 'Content-Type': 'application/json' }
    })
    const authToken = String(await authRes.json())
    const headers = {
      Authorization: `Bearer ${authToken}`,
      'Content-Type': 'application/json',
      Cookie: `jwt=${authToken}`
    }

    // Verify: child parentAlbumId is source_album before move
    const albumsBefore = await (
      await request.get(`${backendPaths.BACKEND_URL}/get/get-albums`, { headers })
    ).json()
    const childBefore = albumsBefore.find((a: any) => a.albumId === child_album)
    expect(childBefore).toBeTruthy()
    expect(childBefore.parentAlbumId).toBe(source_album)

    // Verify via prefetch+get-data that the child appears in the source
    // album's query result before the move.
    // Use the same combined expression as the frontend's AlbumContentsPage:
    // and(trashed:false, or(album:"X", parent_album:"X"))
    const sourceFilter = {
      And: [{ Trashed: false }, { Or: [{ Album: source_album }, { ParentAlbum: source_album }] }]
    }
    const preSourcePrefetchRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
      data: JSON.stringify(sourceFilter),
      headers
    })
    expect(preSourcePrefetchRes.status()).toBe(200)
    const preSourcePrefetch = await preSourcePrefetchRes.json()
    const preSourceDataRes = await request.get(
      `${backendPaths.BACKEND_URL}/get/get-data?timestamp=${preSourcePrefetch.prefetch.timestamp}&start=0&end=${preSourcePrefetch.prefetch.dataLength}`,
      { headers: { Authorization: `Bearer ${preSourcePrefetch.token}` } }
    )
    const preSourceData = await preSourceDataRes.json()

    // The child album should appear as an album-type item in the source result
    // get-data returns DataBaseTimestampReturn with abstractData nested under the camelCase key.
    // For album items, the hash/id is in abstractData.id (from AlbumMetadata.id, not albumId).
    const childInSourceBefore = preSourceData.some(
      (item: any) => item.abstractData?.type === 'album' && item.abstractData?.id === child_album
    )
    expect(childInSourceBefore).toBeTruthy()

    // Move the child album under target via API
    const moveRes = await request.put(`${backendPaths.BACKEND_URL}/put/assign_album`, {
      data: JSON.stringify({ hash: child_album, albumId: target_album }),
      headers
    })
    expect(moveRes.status()).toBe(200)

    // Verify via get-albums that parentAlbumId is updated
    const albumsAfter = await (
      await request.get(`${backendPaths.BACKEND_URL}/get/get-albums`, { headers })
    ).json()
    const childAfter = albumsAfter.find((a: any) => a.albumId === child_album)
    expect(childAfter).toBeTruthy()
    expect(childAfter.parentAlbumId).toBe(target_album)

    // Verify via prefetch+get-data that the child is NO LONGER in the source album query
    const postSourcePrefetchRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
      data: JSON.stringify(sourceFilter),
      headers
    })
    expect(postSourcePrefetchRes.status()).toBe(200)
    const postSourcePrefetch = await postSourcePrefetchRes.json()
    const postSourceDataRes = await request.get(
      `${backendPaths.BACKEND_URL}/get/get-data?timestamp=${postSourcePrefetch.prefetch.timestamp}&start=0&end=${postSourcePrefetch.prefetch.dataLength}`,
      { headers: { Authorization: `Bearer ${postSourcePrefetch.token}` } }
    )
    const postSourceData = await postSourceDataRes.json()
    const childInSourceAfter = postSourceData.some(
      (item: any) => item.abstractData?.type === 'album' && item.abstractData?.id === child_album
    )
    expect(childInSourceAfter).toBeFalsy()

    // Verify via prefetch+get-data that the child IS in the target album query
    const targetFilter = {
      And: [{ Trashed: false }, { Or: [{ Album: target_album }, { ParentAlbum: target_album }] }]
    }
    const targetPrefetchRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
      data: JSON.stringify(targetFilter),
      headers
    })
    expect(targetPrefetchRes.status()).toBe(200)
    const targetPrefetch = await targetPrefetchRes.json()
    const targetDataRes = await request.get(
      `${backendPaths.BACKEND_URL}/get/get-data?timestamp=${targetPrefetch.prefetch.timestamp}&start=0&end=${targetPrefetch.prefetch.dataLength}`,
      { headers: { Authorization: `Bearer ${targetPrefetch.token}` } }
    )
    const targetData = await targetDataRes.json()
    const childInTarget = targetData.some(
      (item: any) => item.abstractData?.type === 'album' && item.abstractData?.id === child_album
    )
    expect(childInTarget).toBeTruthy()

    // UI smoke test: navigate to album pages — no crash
    await page.goto(`/album/${source_album}`)
    await page.waitForTimeout(1500)

    await page.goto(`/album/${target_album}`)
    await page.waitForTimeout(1500)
  })
})
