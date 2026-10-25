import { expect } from '@playwright/test'
import { test } from './scenarioFixtures'
import { createPaths } from './paths'
import { startBackend } from './backendLauncher'
import {
  createGivenContext,
  executeGiven,
  resetAuthToken,
  sessionTokenFromLogin
} from './executeGiven'

// Keep in sync with RACE_LOST_NOTICE in src/script/utils/registration.ts.
// It is inlined (not imported) because the Playwright project
// (tsconfig.node.json) only lists tests/** and cannot reference src files.
const RACE_LOST_NOTICE = 'An administrator account already exists. Please log in instead.'

interface ServerInfoShape {
  enrolled: boolean
  enrollmentOpen: boolean
  captcha: null
}

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
    // Private fresh backend per test: drop any leftover worker-cached
    // session token (signed with a previous backend's authKey) before the
    // state-building requests below. Same reset discipline as the other
    // launcher-isolated specs.
    resetAuthToken()
    const ADMIN_ID = 'first-admin'
    const ADMIN_PASSWORD = backendPaths.ADMIN_PASSWORD
    const WRONG_PASSWORD = 'wrong-password-1'
    const base = backendPaths.BACKEND_URL

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
    // Counter-proof: anonymous server-info still reports an empty store with
    // the window open, so a submit path that created the user anyway (which
    // would flip `enrolled` to true) would fail here.
    await test.step('mismatched confirm creates nothing', async () => {
      await page.getByPlaceholder('User ID').fill(ADMIN_ID)
      await page.getByPlaceholder('Password', { exact: true }).fill(ADMIN_PASSWORD)
      await page.getByPlaceholder('Confirm Password').fill(`${ADMIN_PASSWORD}-different`)
      await page.getByRole('button', { name: 'Register' }).click()
      await expect(page).toHaveURL(/\/register/)
      await expect(page.getByText('Create Admin Account')).toBeVisible()

      const signal = await request.get(`${base}/get/server-info`)
      expect(signal.status()).toBe(200)
      expect((await signal.json()) as ServerInfoShape).toEqual({
        enrolled: false,
        enrollmentOpen: true,
        captcha: null
      })
    })

    // (3) Matching submit enrolls and lands on /timeline.
    // Counter-proof: an authenticated GET /get/users lists exactly the new
    // admin, so client-only routing without a persisted user would fail here.
    await test.step('matching submit enrolls and lands on timeline', async () => {
      await page.getByPlaceholder('Confirm Password').fill(ADMIN_PASSWORD)
      await page.getByRole('button', { name: 'Register' }).click()
      await expect(page).toHaveURL(/\/timeline/)
      await expect(page.locator('#nav-search-input')).toBeVisible()

      const loginRes = await request.post(`${base}/post/users/authenticate`, {
        data: { userId: ADMIN_ID, password: ADMIN_PASSWORD },
        headers: { 'Content-Type': 'application/json' }
      })
      expect(loginRes.status()).toBe(200)
      const token = sessionTokenFromLogin(loginRes)
      const usersRes = await request.get(`${base}/get/users`, {
        headers: authHeaders(token)
      })
      expect(usersRes.status()).toBe(200)
      const users = (await usersRes.json()) as UserSummary[]
      expect(users).toHaveLength(1)
      expect(users[0]).toMatchObject({ userId: ADMIN_ID, admin: true })
    })

    // Media seeding happens here, inside the (now non-empty) install:
    // API writes against an empty running store are refused (this spec
    // enrolls online; offline CLI provisioning is not part of the
    // harness), and step 1 needs the empty store to back its
    // register-signal proof. One photo so the empty-library onboarding
    // dialog never appears: the modal would cover the account menu that
    // the logout step needs to click. Media presence does not affect the
    // enrollment signal, which only reads the user store.
    await executeGiven(
      request,
      [{ dir_album: '/library' }, { photo: '/library/photo.jpg' }],
      createGivenContext('e2e_first_admin'),
      undefined,
      backendPaths,
      { userId: ADMIN_ID, password: ADMIN_PASSWORD }
    )

    // (4) Reload keeps the timeline session: the cookie survived, and the
    // guard's profile fast path still allows /timeline. Counter-proof:
    // without a persisted session cookie the reload would bounce to /login.
    await test.step('reload keeps the timeline session', async () => {
      await page.reload()
      await expect(page).toHaveURL(/\/timeline/)
      await expect(page.locator('#nav-search-input')).toBeVisible()
    })

    // (5) Logout via UI lands on /login and sticks across reload.
    // Counter-proof: the server-side cookie jar no longer holds the session
    // (fails if it survives — `document.cookie` cannot see HttpOnly cookies,
    // so the jar is asserted instead), and the dead session cannot render
    // /timeline anymore. The bare reload-URL check alone cannot bite: an
    // authenticated boot guard leaves /login in place either way.
    await test.step('logout sticks across reload', async () => {
      await page.locator('button:has(.mdi-account-circle)').click()
      await page.getByText('Log out').click()
      await expect(page).toHaveURL(/\/login/)

      await expect
        .poll(async () => (await page.context().cookies()).find((c) => c.name === 'jwt'))
        .toBeUndefined()

      await page.reload()
      await expect(page).toHaveURL(/\/login/)

      await page.goto('/timeline')
      await expect(page).toHaveURL(/\/login/)
    })

    // (6) Wrong password stays on /login with no snackbar at all. Tier
    // taxonomy: a failed login 401 is caller-owned by the login page (its
    // static info banner), not session destruction — the interceptor's
    // authenticate carve-out neither clears the profile nor redirects, and
    // snackbar display belongs to its non-401/non-throttle branches only.
    // Counter-proof: the login page renders intact (banner visible), and
    // the main content shows no HTML blob and no 422 (fails on raw error
    // pages or the legacy bare-string shape).
    await test.step('wrong password stays on login with a clean error', async () => {
      await page.getByPlaceholder('User ID').fill(ADMIN_ID)
      await page.getByPlaceholder('Password', { exact: true }).fill(WRONG_PASSWORD)
      await page.getByRole('button', { name: 'Login' }).click()
      await expect(page).toHaveURL(/\/login/)

      await expect(page.locator('.v-snackbar__content')).toHaveCount(0)
      await expect(page.getByText('Please log in to continue')).toBeVisible()

      const main = page.locator('#app')
      await expect(main).not.toContainText('<')
      await expect(main).not.toContainText('422')
    })

    // (7) Correct login reaches /timeline.
    await test.step('correct login reaches timeline', async () => {
      await page.getByPlaceholder('User ID').fill(ADMIN_ID)
      await page.getByPlaceholder('Password', { exact: true }).fill(ADMIN_PASSWORD)
      await page.getByRole('button', { name: 'Login' }).click()
      await expect(page).toHaveURL(/\/timeline/)
      await expect(page.locator('#nav-search-input')).toBeVisible()
    })

    // (8) Logged-out /register visit leaves immediately: the store is
    // non-empty and there is no session, so server-info reports enrolled and
    // the guard redirects to /login without ever showing the enrollment
    // form — no duplicate can be submitted, and nothing confirms or denies
    // the id (no user enumeration). Counter-proof: the register copy and the
    // race-lost notice are both absent; a leak would show one of them.
    await test.step('logged-out register visit leaves without a form', async () => {
      const raceContext = await browser.newContext({ baseURL: base })
      const racePage = await raceContext.newPage()
      try {
        await racePage.goto('/register')
        await expect(racePage).toHaveURL(/\/login/)
        await expect(racePage.getByText('Create Admin Account')).toHaveCount(0)
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

        const createRes = await request.post(`${closedPaths.BACKEND_URL}/post/users/bootstrap`, {
          data: { userId: 'sneaky', password: ADMIN_PASSWORD },
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
