import { test, expect, type Response } from '@playwright/test'
import { createPaths } from './paths'
import { startBackend } from './backendLauncher'
import { createGivenContext, executeGiven } from './executeGiven'

test.describe('login throttle', () => {
  test('failure burst is throttled with a countdown banner and recovers after the ban', async ({
    browser,
    request
  }) => {
    const ADMIN_ID = 'throttle-admin'
    const WRONG_PASSWORD = 'wrong-password-1'

    const paths = await createPaths()
    // Shrink the first IP-short ban to ~2s so the spec can wait it out.
    // All logins run at the launcher's reduced PBKDF effort; no iteration
    // knob is set here.
    const backend = await startBackend(paths, {
      extraEnv: { PICASU_LOGIN_IP_BASE_BAN_SECS: '2' }
    })
    const context = await browser.newContext({ baseURL: paths.BACKEND_URL })
    const page = await context.newPage()
    try {
      // One photo so the empty-library onboarding dialog never covers the
      // login form on the final successful login.
      await executeGiven(
        request,
        [{ dir_album: '/library' }, { photo: '/library/photo.jpg' }],
        createGivenContext('e2e_login_throttle'),
        undefined,
        paths
      )

      // Enroll the first admin through the open bootstrap window.
      const enroll = await request.post(`${paths.BACKEND_URL}/post/users/bootstrap`, {
        data: { userId: ADMIN_ID, password: paths.ADMIN_PASSWORD },
        headers: { 'Content-Type': 'application/json' }
      })
      expect(enroll.status()).toBe(200)

      await page.goto('/login')
      await expect(page.getByPlaceholder('User ID')).toBeVisible()

      const authResponses: Response[] = []
      page.on('response', (res) => {
        if (res.url().endsWith('/post/authenticate')) {
          authResponses.push(res)
        }
      })

      const loginButton = page.getByRole('button', { name: 'Login' })
      await page.getByPlaceholder('User ID').fill(ADMIN_ID)
      await page.getByPlaceholder('Password', { exact: true }).fill(WRONG_PASSWORD)

      // Four failures: three pass, the 4th opens the (~2s) IP-short ban but
      // still answers 401. Every attempt stays on /login.
      for (let i = 0; i < 4; i++) {
        await loginButton.click()
        await expect(page).toHaveURL(/\/login/)
        await expect.poll(() => authResponses.map((r) => r.status())).toHaveLength(i + 1)
      }
      expect(authResponses.map((r) => r.status())).toEqual([401, 401, 401, 401])

      // 5th attempt lands inside the ban: 429 with a Retry-After hint.
      await loginButton.click()
      await expect
        .poll(() => authResponses.map((r) => r.status()))
        .toEqual([401, 401, 401, 401, 429])
      const throttledResponse = authResponses[4]
      expect(Number(throttledResponse?.headers()['retry-after'])).toBeGreaterThan(0)

      // The banner counts the ban down; the button is disabled and the
      // entered credentials are kept.
      const banner = page.getByText('Too many attempts', { exact: false })
      await expect(banner).toBeVisible()
      await expect(banner).toContainText(/try again in \d+s/)
      await expect(loginButton).toBeDisabled()
      await expect(page.getByPlaceholder('User ID')).toHaveValue(ADMIN_ID)
      await expect(page.getByPlaceholder('Password', { exact: true })).toHaveValue(WRONG_PASSWORD)

      // Ban elapses on its own: banner clears, button re-enables, and no
      // further authenticate request was needed to make that happen.
      await expect(banner).toHaveCount(0, { timeout: 15_000 })
      await expect(loginButton).toBeEnabled()
      expect(authResponses).toHaveLength(5)

      // Correct password lands on /timeline.
      await page.getByPlaceholder('Password', { exact: true }).fill(paths.ADMIN_PASSWORD)
      await loginButton.click()
      await expect(page).toHaveURL(/\/timeline/)
      await expect(page.locator('#nav-search-input')).toBeVisible()
    } finally {
      await context.close()
      await backend.stop()
    }
  })
})
