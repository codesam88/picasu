import { expect } from '@playwright/test'
import { test } from './scenarioFixtures'
import { executeGiven, createGivenContext, resetAuthToken } from './executeGiven'
import type { GivenItem } from './types'

test.describe('Grid stale after sub-album assign_album mutation', () => {
  test('dataStore retains stale entries after refreshGalleryAfterMutation (child album view)', async ({
    page,
    request,
    backendPaths
  }) => {
    resetAuthToken()
    const ctx = createGivenContext('stale_subalbum_assign')

    // The photo lives in a child album (/subparent/child). We navigate to
    // this child album and move the photo to a sibling (/subsibling).
    const givenItems: GivenItem[] = [
      { dir_album: '/subparent' },
      { dir_album: '/subparent/child' },
      { photo: '/subparent/child/photo.jpg' },
      { dir_album: '/subsibling', id_as: '$target_album' }
    ]

    const seeded = await executeGiven(request, givenItems, ctx, undefined, backendPaths)
    const targetAlbum = seeded.vars['$target_album']

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

    // Discover album IDs from the API.
    const albumsRes = await request.get(`${backendPaths.BACKEND_URL}/get/get-albums`, { headers })
    const albums = await albumsRes.json()
    const childAlbum = albums.find(
      (a: any) => a.dirPath && a.dirPath.endsWith('subparent/child')
    )?.albumId
    expect(childAlbum).toBeTruthy()

    // Fetch the photo hash via API.
    const photoHash = await (async (): Promise<string> => {
      const pfRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
        data: JSON.stringify({
          And: [{ Trashed: false }, { Or: [{ Album: childAlbum }, { ParentAlbum: childAlbum }] }]
        }),
        headers
      })
      const pf = await pfRes.json()
      const dRes = await request.get(
        `${backendPaths.BACKEND_URL}/get/get-data?timestamp=${pf.prefetch.timestamp}&start=0&end=1`,
        { headers: { Authorization: `Bearer ${pf.token}` } }
      )
      const d = await dRes.json()
      return d[0]?.abstractData?.id as string
    })()

    // Navigate to child album where the photo lives.
    await page.goto(`/album/${childAlbum}`)
    await page.waitForSelector('.desktop-small-image', { timeout: 15000 })
    await page.waitForTimeout(1000)

    expect(await page.locator('.desktop-small-image').count()).toBe(1)

    const baseline = await page.evaluate(() => {
      const pinia = (document.querySelector('#app') as Record<string, any>).__vue_app__.config
        .globalProperties.$pinia
      const ds = pinia.state.value['DataStoremainId']
      const pf = pinia.state.value['prefetchStoremainId']
      return {
        dataSize: ds?.data?.size ?? -1,
        batchFetchedSize: ds?.batchFetched?.size ?? -1,
        dataLength: pf?.dataLength ?? -1
      }
    })
    expect(baseline.dataSize).toBe(1)
    expect(baseline.batchFetchedSize).toBeGreaterThan(0)
    expect(baseline.dataLength).toBe(1)

    // Navigate to photo detail view.
    for (let i = 0; i < 3; i++) {
      await page.locator('.desktop-small-image').first().click()
      try {
        await page.waitForURL(/\/view\//, { timeout: 3000 })
        break
      } catch {
        if (i < 2) await page.waitForTimeout(500)
        else throw new Error('Failed to navigate to photo detail view')
      }
    }

    // Open info pane.
    for (let i = 0; i < 5; i++) {
      const btn = page.locator('button:has(.mdi-information-outline)')
      if (await btn.isVisible({ timeout: 2000 }).catch(() => false)) {
        await btn.click()
        break
      }
      if (i < 4) await page.waitForTimeout(500)
      else throw new Error('Info button not found')
    }

    // Open Assign Album dialog.
    await page.getByTestId('photo-menu').click()
    await page.getByRole('option', { name: 'Assign Album' }).click()

    // Select the sibling target in the album tree.
    await page.getByRole('option', { name: /Subsibling/i }).click()
    await page.getByRole('button', { name: 'Move' }).click()

    // Wait for success toast and refreshGalleryAfterMutation.
    const snackbar = page.getByRole('status').or(page.locator('.v-snackbar'))
    await expect(snackbar.first()).toBeVisible({ timeout: 15000 })
    await expect(snackbar.first()).toContainText('Moved to album')
    await page.waitForTimeout(3000)

    // Check Pinia state after the mutation flow.
    const after = await page.evaluate(() => {
      const pinia = (document.querySelector('#app') as Record<string, any>).__vue_app__.config
        .globalProperties.$pinia
      const ds = pinia.state.value['DataStoremainId']
      const pf = pinia.state.value['prefetchStoremainId']
      return {
        dataSize: ds?.data?.size ?? -1,
        batchFetchedSize: ds?.batchFetched?.size ?? -1,
        dataLength: pf?.dataLength ?? -1
      }
    })

    // --- Server-side verification ---
    const childFilter = {
      And: [{ Trashed: false }, { Or: [{ Album: childAlbum }, { ParentAlbum: childAlbum }] }]
    }
    const childPrefetchRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
      data: JSON.stringify(childFilter),
      headers
    })
    expect(childPrefetchRes.status()).toBe(200)
    const childPrefetch = await childPrefetchRes.json()
    expect(childPrefetch.prefetch.dataLength).toBe(0)

    const targetFilter = {
      And: [{ Trashed: false }, { Or: [{ Album: targetAlbum }, { ParentAlbum: targetAlbum }] }]
    }
    const targetPrefetchRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
      data: JSON.stringify(targetFilter),
      headers
    })
    expect(targetPrefetchRes.status()).toBe(200)
    const targetPrefetch = await targetPrefetchRes.json()
    expect(targetPrefetch.prefetch.dataLength).toBe(2)

    const targetDataRes = await request.get(
      `${backendPaths.BACKEND_URL}/get/get-data?timestamp=${targetPrefetch.prefetch.timestamp}&start=0&end=2`,
      { headers: { Authorization: `Bearer ${targetPrefetch.token}` } }
    )
    expect(targetDataRes.status()).toBe(200)
    const targetData = await targetDataRes.json()
    const photoInTarget = targetData.some((item: any) => item.abstractData?.id === photoHash)
    expect(photoInTarget).toBeTruthy()

    // --- Fault verification ---
    // refreshGalleryAfterMutation correctly updates prefetchStore.dataLength.
    expect(after.dataLength).toBe(0)

    // BUG: dataStore.data and dataStore.batchFetched are never cleared.
    // These assertions FAIL with the current buggy code.
    expect(after.dataSize).toBe(0)
    expect(after.batchFetchedSize).toBe(0)
  })
})
