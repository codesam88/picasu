import { expect } from '@playwright/test'
import { test } from './scenarioFixtures'
import { executeGiven, createGivenContext, resetAuthToken } from './executeGiven'
import type { GivenItem } from './types'

test.describe('Grid stale after delete/trash mutation', () => {
  test('dataStore retains stale entries after trashing the only photo in an album', async ({
    page,
    request,
    backendPaths
  }) => {
    resetAuthToken()
    const ctx = createGivenContext('stale_delete')

    const givenItems: GivenItem[] = [
      { dir_album: '/deletealbum' },
      { photo: '/deletealbum/photo.jpg' }
    ]

    await executeGiven(request, givenItems, ctx, undefined, backendPaths)

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

    const albumsRes = await request.get(`${backendPaths.BACKEND_URL}/get/get-albums`, { headers })
    const albums = await albumsRes.json()
    const albumId = albums.find((a: any) => a.dirPath && a.dirPath.endsWith('deletealbum'))?.albumId
    expect(albumId).toBeTruthy()

    // Navigate to album, wait for grid to render.
    await page.goto(`/album/${albumId}`)
    await page.waitForSelector('.desktop-small-image', { timeout: 15000 })
    await page.waitForTimeout(1000)

    expect(await page.locator('.desktop-small-image').count()).toBe(1)

    // Baseline Pinia state: 1 photo, 1 prefetch entry.
    const baseline = await page.evaluate(() => {
      const pinia = (document.querySelector('#app') as Record<string, any>).__vue_app__.config
        .globalProperties.$pinia
      const ds = pinia.state.value['DataStoremainId']
      const pf = pinia.state.value['prefetchStoremainId']
      return {
        dataSize: ds?.data?.size ?? -1,
        dataLength: pf?.dataLength ?? -1
      }
    })
    expect(baseline.dataSize).toBe(1)
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

    // Open the three-dot menu and click Delete.
    await page.getByTestId('photo-menu').click()
    await page.getByRole('option', { name: 'Delete' }).click()

    // Wait for success toast (trash API completed).
    const snackbar = page.getByRole('status').or(page.locator('.v-snackbar'))
    await expect(snackbar.first()).toBeVisible({ timeout: 15000 })
    await expect(snackbar.first()).toContainText('Successfully updated')
    await page.waitForTimeout(2000)

    // Check Pinia state after trash.
    const after = await page.evaluate(() => {
      const pinia = (document.querySelector('#app') as Record<string, any>).__vue_app__.config
        .globalProperties.$pinia
      const ds = pinia.state.value['DataStoremainId']
      const pf = pinia.state.value['prefetchStoremainId']
      const firstEntry = ds?.data?.values()?.next()?.value
      return {
        dataSize: ds?.data?.size ?? -1,
        dataLength: pf?.dataLength ?? -1,
        isTrashed: firstEntry?.isTrashed ?? null
      }
    })

    // --- Server-side verification ---
    // The photo is actually trashed on the server.
    const notTrashedFilter = {
      And: [{ Trashed: false }, { Or: [{ Album: albumId }, { ParentAlbum: albumId }] }]
    }
    const notTrashedRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
      data: JSON.stringify(notTrashedFilter),
      headers
    })
    const notTrashed = await notTrashedRes.json()
    expect(notTrashed.prefetch.dataLength).toBe(0)

    // The photo appears in the trash.
    const trashedFilter = {
      And: [{ Trashed: true }, { Or: [{ Album: albumId }, { ParentAlbum: albumId }] }]
    }
    const trashedRes = await request.post(`${backendPaths.BACKEND_URL}/get/prefetch`, {
      data: JSON.stringify(trashedFilter),
      headers
    })
    const trashed = await trashedRes.json()
    expect(trashed.prefetch.dataLength).toBe(1)

    // --- Fault verification ---
    // The optimistic update correctly set isTrashed.
    expect(after.isTrashed).toBe(true)

    // BUG: refreshGalleryAfterMutation is never called after trash/delete.
    // prefetchStore.dataLength should be 0 (the album has 0 non-trashed photos)
    // but remains at 1 because no refresh happened.  dataStore.data keeps the
    // old entry for the same reason.
    //
    // These assertions FAIL with the current buggy code.
    expect(after.dataLength).toBe(0)
    expect(after.dataSize).toBe(0)
  })
})
