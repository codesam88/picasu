import { expect } from '@playwright/test'
import { test } from './scenarioFixtures'
import { executeGiven, createGivenContext, resetAuthToken } from './executeGiven'
import type { GivenItem } from './types'

test.describe('Grid stale after assign_album mutation', () => {
  test('dataStore retains stale entries after refreshGalleryAfterMutation', async ({
    page,
    request,
    backendPaths
  }) => {
    resetAuthToken()
    const ctx = createGivenContext('stale_grid_assign')

    // Only dir_album entries WITH id_as trigger an auto-generated placeholder
    // photo.  We omit id_as on the source dir_album so the source album
    // contains exactly 1 photo (photo.jpg) and there's no ambiguity when
    // clicking the first grid item.
    const givenItems: GivenItem[] = [
      { dir_album: '/stalesource' },
      { photo: '/stalesource/photo.jpg', id_as: '$photo' },
      { dir_album: '/staletarget', id_as: '$target_album' }
    ]

    const seeded = await executeGiven(request, givenItems, ctx, undefined, backendPaths)
    const targetAlbum = seeded.vars['$target_album']

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

    // Discover the source album ID from the API.
    const albumsRes = await request.get(`${backendPaths.BACKEND_URL}/get/get-albums`, { headers })
    const albums = await albumsRes.json()
    const sourceAlbum = albums.find(
      (a: any) => a.dirPath && a.dirPath.endsWith('stalesource')
    )?.albumId
    expect(sourceAlbum).toBeTruthy()

    // Fetch the source album prefetch to get the photo hash.
    const fetchPhotoHash = async (): Promise<string> => {
      const pfRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
        data: JSON.stringify({
          And: [{ Trashed: false }, { Or: [{ Album: sourceAlbum }, { ParentAlbum: sourceAlbum }] }]
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
    }
    const photoHash = await fetchPhotoHash()

    // Navigate to source album — this triggers Gallery mount and usePrefetch,
    // which sets prefetchStore.filterBasicString and populates the grid.
    await page.goto(`/album/${sourceAlbum}`)
    await page.waitForSelector('.desktop-small-image', { timeout: 15000 })
    await page.waitForTimeout(1000)

    // Sanity: the grid shows exactly one image.
    const gridImagesBefore = await page.locator('.desktop-small-image').count()
    expect(gridImagesBefore).toBe(1)

    // Record baseline Pinia state.
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

    // Navigate to photo detail view so we can reach the Assign Album dialog.
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

    // Open the info side pane (prerequisite for the three-dot menu).
    for (let i = 0; i < 5; i++) {
      const btn = page.locator('button:has(.mdi-information-outline)')
      if (await btn.isVisible({ timeout: 2000 }).catch(() => false)) {
        await btn.click()
        break
      }
      if (i < 4) await page.waitForTimeout(500)
      else throw new Error('Info button not found')
    }

    // Open the three-dot menu on the photo.
    await page.getByTestId('photo-menu').click()
    await page.getByRole('option', { name: 'Assign Album' }).click()

    // Select the target album in the dialog's album tree.
    await page.getByRole('option', { name: /Staletarget/i }).click()

    // Click Move — this triggers handleSubmit in AssignAlbumModal:
    //   1. assignAlbum() API (shows success toast, optimistic dataStore.setAlbum)
    //   2. refreshGalleryAfterMutation()
    //   3. Dialog closes
    await page.getByRole('button', { name: 'Move' }).click()

    // Wait for the success toast (signals assignAlbum API completed).
    // refreshGalleryAfterMutation runs after the toast.
    const snackbar = page.getByRole('status').or(page.locator('.v-snackbar'))
    await expect(snackbar.first()).toBeVisible({ timeout: 15000 })
    await expect(snackbar.first()).toContainText('Moved to album')

    // Wait for refreshGalleryAfterMutation to complete.
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

    // --- Server-side verification: photo was correctly moved ---
    const sourceFilter = {
      And: [{ Trashed: false }, { Or: [{ Album: sourceAlbum }, { ParentAlbum: sourceAlbum }] }]
    }
    const sourcePrefetchRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
      data: JSON.stringify(sourceFilter),
      headers
    })
    expect(sourcePrefetchRes.status()).toBe(200)
    const sourcePrefetch = await sourcePrefetchRes.json()
    expect(sourcePrefetch.prefetch.dataLength).toBe(0)

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
    // refreshGalleryAfterMutation calls processPrefetchChain which updates
    // prefetchStore.dataLength (this part works correctly) to the new
    // count of 0 (the only photo was moved, source is now empty).
    expect(after.dataLength).toBe(0)

    // BUG: dataStore.data and dataStore.batchFetched are never cleared.
    // processPrefetchChain prefetches the new (empty) data set successfully,
    // but the subsequent clearAll() on queue/row/offset/img stores at the end
    // of refreshGalleryAfterMutation empties the queue before the debounced
    // row fetch runs.  dataStore.$reset() is never called, so old entries
    // remain.
    //
    // These assertions FAIL with the current buggy code.  When the bug is
    // fixed they will pass — do not relax them back.
    expect(after.dataSize).toBe(0)
    expect(after.batchFetchedSize).toBe(0)
  })
})
