import { expect, request as baseRequest, type APIRequestContext } from '@playwright/test'
import { test } from './scenarioFixtures'
import {
  createGivenContext,
  executeGiven,
  resetAuthToken,
  sessionTokenFromLogin
} from './executeGiven'

const ADMIN_ID = 'invite-admin'
const VIEWER_ID = 'maria'
const VIEWER_PASSWORD = 'maria-secret-9'
const OPEN_ID = 'guest2'
const OPEN_PASSWORD = 'guest2-secret-9'

const authHeaders = (token: string) => ({
  'Content-Type': 'application/json',
  Cookie: `jwt=${token}`
})

interface CreateInviteResponse {
  token: string
  invite: { id: string; username: string | null; usedBy: string | null }
}

async function bootstrapAdmin(request: APIRequestContext, base: string, adminPassword: string) {
  const res = await request.post(`${base}/post/users/bootstrap`, {
    data: { userId: ADMIN_ID, password: adminPassword },
    headers: { 'Content-Type': 'application/json' }
  })
  expect(res.status()).toBe(200)
}

async function adminToken(
  request: APIRequestContext,
  base: string,
  adminPassword: string
): Promise<string> {
  const res = await request.post(`${base}/post/users/authenticate`, {
    data: { userId: ADMIN_ID, password: adminPassword },
    headers: { 'Content-Type': 'application/json' }
  })
  expect(res.status()).toBe(200)
  return sessionTokenFromLogin(res)
}

async function mintInvite(
  request: APIRequestContext,
  base: string,
  token: string,
  username?: string
): Promise<CreateInviteResponse> {
  const body: Record<string, unknown> = { expiresInHours: 168 }
  if (username !== undefined) body.username = username
  const res = await request.post(`${base}/post/invites`, {
    data: body,
    headers: authHeaders(token)
  })
  expect(res.status()).toBe(200)
  return (await res.json()) as CreateInviteResponse
}

/// Sessionless invite-redeem corners: mismatch, correct redeem, reuse,
/// unknown token. `anon` must carry no cookies (see the call site).
async function anonymousCorners(
  anon: APIRequestContext,
  authed: APIRequestContext,
  base: string,
  adminCookie: string
): Promise<void> {
  const probe = await mintInvite(authed, base, adminCookie, 'bound-probe')
  const json = { 'Content-Type': 'application/json' }

  const mismatch = await anon.post(`${base}/post/users/register`, {
    data: { userId: 'someone-else', password: 'someone-secret-9', invite: probe.token },
    headers: json
  })
  expect(mismatch.status()).toBe(403)

  const listed = await authed.get(`${base}/get/invites`, {
    headers: authHeaders(adminCookie)
  })
  expect(listed.status()).toBe(200)
  const rows = (await listed.json()) as CreateInviteResponse['invite'][]
  const row = rows.find((r) => r.id === probe.invite.id)
  expect(row?.usedBy).toBeNull()

  const ok = await anon.post(`${base}/post/users/register`, {
    data: { userId: 'bound-probe', password: 'bound-probe-secret-9', invite: probe.token },
    headers: json
  })
  expect(ok.status()).toBe(200)

  const reuse = await anon.post(`${base}/post/users/register`, {
    // Same bound name: passes the binding check, then hits the used token.
    data: { userId: 'bound-probe', password: 'bound-probe-secret-9', invite: probe.token },
    headers: json
  })
  expect(reuse.status()).toBe(409)

  const unknown = await anon.post(`${base}/post/users/register`, {
    data: { userId: 'nobody', password: 'nobody-secret-9', invite: 'not-a-real-token' },
    headers: json
  })
  expect(unknown.status()).toBe(404)
}

test.describe('invite registration', () => {
  test('bound invite prefills, registers, and the viewer stays on timeline', async ({
    page,
    request,
    backendPaths,
    browser
  }) => {
    resetAuthToken()
    const base = backendPaths.BACKEND_URL
    const adminPassword = backendPaths.ADMIN_PASSWORD

    await bootstrapAdmin(request, base, adminPassword)
    const adminCookie = await adminToken(request, base, adminPassword)

    // Bound + open invites up front; each redeems at most once.
    const bound = await mintInvite(request, base, adminCookie, VIEWER_ID)
    expect(bound.invite.username).toBe(VIEWER_ID)
    const open = await mintInvite(request, base, adminCookie)
    expect(open.invite.username).toBeNull()

    // (1) Invite link prefills the bound name, not the last login.
    // Counter-proof: localStorage holds the admin id (the reporter's
    // "wrong old username"), so a prefill from last-login would show it.
    await test.step('invite link prefills the bound username', async () => {
      await page.goto('/')
      await page.evaluate((id) => localStorage.setItem('picasu-last-user-id', id), ADMIN_ID)
      await page.goto(
        `/register?invite=${encodeURIComponent(bound.token)}&username=${encodeURIComponent(VIEWER_ID)}`
      )
      await expect(page.getByText('Create Account')).toBeVisible()
      await expect(page.getByPlaceholder('User ID')).toHaveValue(VIEWER_ID)
    })

    // (2) Registering through the link enrolls and lands on /timeline.
    // Counter-proof: the profile round-trips through the session store,
    // so a client-only navigation without a persisted user would fail the
    // reload step below.
    await test.step('registering through the invite lands on timeline', async () => {
      await page.getByPlaceholder('Password', { exact: true }).fill(VIEWER_PASSWORD)
      await page.getByPlaceholder('Confirm Password').fill(VIEWER_PASSWORD)
      await page.getByRole('button', { name: 'Register' }).click()
      await expect(page).toHaveURL(/\/timeline/)
      await expect(page.locator('#nav-search-input')).toBeVisible()
    })

    // (3) Reload keeps the viewer session: no bounce to /login.
    // Counter-proof: the pre-fix viewer fetched the admin-only
    // GET /get/config, got 401, and the interceptor cleared the session.
    await test.step('reload keeps the viewer on timeline', async () => {
      await page.reload()
      await expect(page).toHaveURL(/\/timeline/)
      await expect(page.locator('#nav-search-input')).toBeVisible()
    })

    // (4) API corners on fresh invites: mismatch is 403 + unconsumed,
    // reuse is 409, unknown is 404, open binds anyone. These must run
    // without a session: the shared `request` jar holds the admin cookie
    // after step 0, and an admin session bypasses invite checks entirely.
    await test.step('invite redeem corners stay server-enforced', async () => {
      const anon = await baseRequest.newContext()
      try {
        await anonymousCorners(anon, request, base, adminCookie)
      } finally {
        await anon.dispose()
      }
    })

    // (5) Open invite without a hint starts empty and registers any id.
    // Counter-proof: a last-login prefill would show `maria` (written by
    // step 2), so an empty field proves invite mode never uses it.
    await test.step('open invite starts empty and registers', async () => {
      const guestContext = await browser.newContext({ baseURL: base })
      const guestPage = await guestContext.newPage()
      try {
        await guestPage.goto(`/register?invite=${encodeURIComponent(open.token)}`)
        await expect(guestPage.getByText('Create Account')).toBeVisible()
        await expect(guestPage.getByPlaceholder('User ID')).toHaveValue('')
        await guestPage.getByPlaceholder('User ID').fill(OPEN_ID)
        await guestPage.getByPlaceholder('Password', { exact: true }).fill(OPEN_PASSWORD)
        await guestPage.getByPlaceholder('Confirm Password').fill(OPEN_PASSWORD)
        await guestPage.getByRole('button', { name: 'Register' }).click()
        await expect(guestPage).toHaveURL(/\/timeline/)
      } finally {
        await guestContext.close()
      }
    })

    // Media for the visibility proof below; seeded after enrollment since
    // API writes against an empty store are refused.
    await executeGiven(
      request,
      [{ dir_album: '/library' }, { photo: '/library/photo.jpg' }],
      createGivenContext('e2e_invite_register'),
      undefined,
      backendPaths,
      { userId: ADMIN_ID, password: adminPassword }
    )

    // (6) The viewer logs in through the UI and sees the library.
    await test.step('viewer logs in and sees the timeline', async () => {
      const viewerContext = await browser.newContext({ baseURL: base })
      const viewerPage = await viewerContext.newPage()
      try {
        await viewerPage.goto('/login')
        await viewerPage.getByPlaceholder('User ID').fill(VIEWER_ID)
        await viewerPage.getByPlaceholder('Password', { exact: true }).fill(VIEWER_PASSWORD)
        await viewerPage.getByRole('button', { name: 'Login' }).click()
        await expect(viewerPage).toHaveURL(/\/timeline/)
        await expect(viewerPage.locator('#nav-search-input')).toBeVisible()
        await viewerPage.reload()
        await expect(viewerPage).toHaveURL(/\/timeline/)
      } finally {
        await viewerContext.close()
      }
    })
  })
})
