import type { APIRequestContext, Page } from '@playwright/test'
import { expect } from '@playwright/test'
import { ADMIN_PASSWORD, type WorkerPaths } from './paths'
import type { UiScenario } from './types'

export interface SessionCreds {
  userId: string
  password: string
}

/// The credentials a harness-managed scenario logs in with: the first `user:`
/// given entry when the scenario declares one, else the harness's default
/// admin account.
export function resolveSessionCreds(scenario: UiScenario): SessionCreds {
  const userGiven = scenario.given?.find((item) => 'user' in item && item.user) as
    { user: { user_id: string; password: string } } | undefined
  if (userGiven) {
    return { userId: userGiven.user.user_id, password: userGiven.user.password }
  }
  return { userId: 'admin', password: ADMIN_PASSWORD }
}

/// Enroll the resolved account for harness-managed scenarios that declare no
/// `user:` given. Runs before `executeGiven` so a `config:` given authenticates
/// against a real user instead of the open-mode path, and so the first UI
/// navigation is not bounced to `/register`. `session: none` scenarios and
/// scenarios that enroll their own `user:` are left to `executeGiven`.
export async function enrollIfNeeded(
  request: APIRequestContext,
  scenario: UiScenario,
  backendPaths: WorkerPaths
): Promise<void> {
  if (scenario.session === 'none') return
  const hasUserGiven = scenario.given?.some((item) => 'user' in item && item.user)
  if (hasUserGiven) return

  const creds = resolveSessionCreds(scenario)
  const res = await request.post(`${backendPaths.BACKEND_URL}/post/users/bootstrap`, {
    data: { userId: creds.userId, password: creds.password },
    headers: { 'Content-Type': 'application/json' }
  })
  if (!res.ok()) {
    throw new Error(`Enrollment failed: ${res.status()} ${await res.text()}`)
  }
}

/// Drive the real UI login for the scenario's session: open `/login`, fill the
/// resolved credentials, submit, and require a landing on `/timeline`. On
/// timeout the error carries the final URL and a page-state hint — a landing on
/// `/register` means enrollment did not happen.
export async function uiLogin(page: Page, creds: SessionCreds): Promise<void> {
  try {
    await page.goto('/login')
    await page.getByPlaceholder('User ID').fill(creds.userId)
    await page.getByPlaceholder('Password', { exact: true }).fill(creds.password)
    await page.getByRole('button', { name: 'Login' }).click()
    await expect(page).toHaveURL(/\/timeline/)
  } catch (error) {
    const url = page.url()
    const hint = /\/register/.test(url)
      ? 'the page is on /register: enrollment did not happen'
      : `the page is on ${url}`
    throw new Error(`UI login did not reach /timeline (${hint})`, { cause: error })
  }
}
