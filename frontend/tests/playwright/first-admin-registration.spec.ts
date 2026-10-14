import { expect } from '@playwright/test'
import { test } from './scenarioFixtures'
import { createPaths } from './paths'
import { startBackend } from './backendLauncher'
import { createGivenContext, executeGiven } from './executeGiven'

// Keep in sync with RACE_LOST_NOTICE in src/script/utils/registration.ts.
// It is inlined (not imported) because the Playwright project
// (tsconfig.node.json) only lists tests/** and cannot reference src files.
const RACE_LOST_NOTICE = 'An administrator account already exists. Please log in instead.'

interface UserSummary {
  userId: string
  admin: boolean
}

const authHeaders = (token: string) => ({
  Authorization: `Bearer ${token}`,
  'Content-Type': 'application/json',
  Cookie: `jwt=${token}`
})

test.describe('first admin registration', () => {
  test('open install enrolls, session survives reload, logout sticks, duplicate reveals nothing, closed stays shut', async ({
    page,
    request,
    backendPaths,
    browser
  }) => {
    const ADMIN_ID = 'first-admin'
    const ADMIN_PASSWORD = backendPaths.ADMIN_PASSWORD
    const WRONG_PASSWORD = 'wrong-password-1'
    const base = backendPaths.BACKEND_URL

    // One photo so the empty-library onboarding dialog never appears: this
    // spec is about auth, and the modal would cover the account menu that
    // the logout step needs to click. Media presence does not affect the
    // enrollment probe, which only reads the user store.
    await executeGiven(
      request,
      [{ dir_album: '/library' }, { photo: '/library/photo.jpg' }],
      createGivenContext('e2e_first_admin'),
      undefined,
      backendPaths
    )

    // (1) A fresh backend boots to /register, with no timeline content.
    // Counter-proof: the timeline shell (GalleryBar search) is absent, so a
    // broken boot guard that rendered the app instead would fail here.
    await test.step('fresh install boots to register', async () => {
      await page.goto('/')
      await expect(page).toHaveURL(/\/register/)
      await expect(page.getByText('Create Admin Account')).toBeVisible()
      await expect(page.locator('#nav-search-input')).toHaveCount(0)
    })

    // (2) Mismatched confirm submits nothing and stays on /register.
    // Counter-proof: the open-window probe still lists an empty store, so a
    // submit path that created the user anyway would fail here. (Object-login
    // cannot prove this: in the open window ANY parseable login object mints
    // a token, even for unknown ids.)
    await test.step('mismatched confirm creates nothing', async () => {
      await page.getByPlaceholder('User ID').fill(ADMIN_ID)
      await page.getByPlaceholder('Password', { exact: true }).fill(ADMIN_PASSWORD)
      await page.getByPlaceholder('Confirm Password').fill(`${ADMIN_PASSWORD}-different`)
      await page.getByRole('button', { name: 'Register' }).click()
      await expect(page).toHaveURL(/\/register/)
      await expect(page.getByText('Create Admin Account')).toBeVisible()

      const probe = await request.get(`${base}/get/users`)
      expect(probe.status()).toBe(200)
      expect(await probe.json()).toEqual([])
    })

    // (3) Matching submit enrolls and lands on /timeline.
    // Counter-proof: an authenticated GET /get/users lists exactly the new
    // admin, so client-only routing without a persisted user would fail here.
    await test.step('matching submit enrolls and lands on timeline', async () => {
      await page.getByPlaceholder('Confirm Password').fill(ADMIN_PASSWORD)
      await page.getByRole('button', { name: 'Register' }).click()
      await expect(page).toHaveURL(/\/timeline/)
      await expect(page.locator('#nav-search-input')).toBeVisible()

      const loginRes = await request.post(`${base}/post/authenticate`, {
        data: { userId: ADMIN_ID, password: ADMIN_PASSWORD },
        headers: { 'Content-Type': 'application/json' }
      })
      expect(loginRes.status()).toBe(200)
      const token = String(await loginRes.json())
      const usersRes = await request.get(`${base}/get/users`, {
        headers: authHeaders(token)
      })
      expect(usersRes.status()).toBe(200)
      const users = (await usersRes.json()) as UserSummary[]
      expect(users).toHaveLength(1)
      expect(users[0]).toMatchObject({ userId: ADMIN_ID, admin: true })
    })

    // (4) Reload keeps the timeline session: the cookie survived, and the
    // re-probed non-empty store no longer forces /register. Counter-proof:
    // without a persisted session cookie the reload would bounce to /login.
    await test.step('reload keeps the timeline session', async () => {
      await page.reload()
      await expect(page).toHaveURL(/\/timeline/)
      await expect(page.locator('#nav-search-input')).toBeVisible()
    })

    // (5) Logout via UI lands on /login and sticks across reload.
    // Counter-proof: the session cookie is actually gone (fails if it
    // survives), and the dead session cannot render /timeline anymore. The
    // bare reload-URL check alone cannot bite: an authenticated boot guard
    // leaves /login in place either way.
    await test.step('logout sticks across reload', async () => {
      await page.locator('button:has(.mdi-account-circle)').click()
      await page.getByText('Log out').click()
      await expect(page).toHaveURL(/\/login/)

      await expect.poll(() => page.evaluate(() => document.cookie)).not.toContain('jwt=')

      await page.reload()
      await expect(page).toHaveURL(/\/login/)

      await page.goto('/timeline')
      await expect(page).toHaveURL(/\/login/)
    })

    // (6) Wrong password stays on /login with a clean unauthorized error.
    // Counter-proof: the snackbar surfaces a real message (fails on silent
    // failure) containing no HTML blob and no 422 (fails on raw error pages
    // or the legacy bare-string shape).
    await test.step('wrong password stays on login with a clean error', async () => {
      await page.getByPlaceholder('User ID').fill(ADMIN_ID)
      await page.getByPlaceholder('Password', { exact: true }).fill(WRONG_PASSWORD)
      await page.getByRole('button', { name: 'Login' }).click()
      await expect(page).toHaveURL(/\/login/)

      const snackbar = page.locator('.v-snackbar__content').first()
      await expect(snackbar).toBeVisible()
      await expect(snackbar).not.toContainText('<')
      await expect(snackbar).not.toContainText('422')
    })

    // (7) Correct login reaches /timeline.
    await test.step('correct login reaches timeline', async () => {
      await page.getByPlaceholder('User ID').fill(ADMIN_ID)
      await page.getByPlaceholder('Password', { exact: true }).fill(ADMIN_PASSWORD)
      await page.getByRole('button', { name: 'Login' }).click()
      await expect(page).toHaveURL(/\/timeline/)
      await expect(page.locator('#nav-search-input')).toBeVisible()
    })

    // (8) Logged-out duplicate reveals nothing: the store is non-empty
    // and there is no session, so the backend answers 401 — the 409 race
    // path needs an authenticated or open-window caller. The UI lands on
    // /login WITHOUT confirming the id exists (no user enumeration).
    // Counter-proof: the race-lost notice is absent; a leak would show it.
    await test.step('logged-out duplicate reveals nothing', async () => {
      const raceContext = await browser.newContext({ baseURL: base })
      const racePage = await raceContext.newPage()
      try {
        await racePage.goto('/register')
        await expect(racePage).toHaveURL(/\/register/)
        await racePage.getByPlaceholder('User ID').fill(ADMIN_ID)
        await racePage.getByPlaceholder('Password', { exact: true }).fill(ADMIN_PASSWORD)
        await racePage.getByPlaceholder('Confirm Password').fill(ADMIN_PASSWORD)
        await racePage.getByRole('button', { name: 'Register' }).click()
        await expect(racePage).toHaveURL(/\/login/)
        await expect(racePage.getByText(RACE_LOST_NOTICE)).toHaveCount(0)
      } finally {
        await raceContext.close()
      }
    })

    // (9) Closed install: a backend started WITHOUT the bootstrap flag boots
    // to /login, never /register. Counter-proof: anonymous enrollment is
    // refused with 401, so an open window would fail here.
    await test.step('closed install boots to login, never register', async () => {
      const closedPaths = await createPaths()
      const closed = await startBackend(closedPaths, {
        extraEnv: { PICASU_BOOTSTRAP_ADMIN: undefined }
      })
      const closedContext = await browser.newContext({ baseURL: closedPaths.BACKEND_URL })
      const closedPage = await closedContext.newPage()
      try {
        await closedPage.goto('/')
        await expect(closedPage).toHaveURL(/\/login/)
        await expect(closedPage.getByText('Create Admin Account')).toHaveCount(0)

        const createRes = await request.post(`${closedPaths.BACKEND_URL}/post/users/create`, {
          data: { userId: 'sneaky', password: ADMIN_PASSWORD, admin: true },
          headers: { 'Content-Type': 'application/json' }
        })
        expect(createRes.status()).toBe(401)
      } finally {
        await closedContext.close()
        await closed.stop()
      }
    })
  })
})
