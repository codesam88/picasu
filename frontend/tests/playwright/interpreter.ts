import * as http from 'http'
import * as fs from 'fs'
import * as path from 'path'
import type { Page, Locator } from '@playwright/test'
import { expect } from '@playwright/test'
import { UiWhenItem, UiAssertItem, UiStep } from './types'
import { GivenContext } from './executeGiven'
import { CoverageTracer, assertionTarget } from './tracer'

/** Ping the backend health endpoint and throw a descriptive error if it is unreachable. */
async function assertBackendAlive(backendUrl: string): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const req = http.get(`${backendUrl}/get/prefetch`, (res) => {
      res.resume()
      // Any response (even 4xx) means the server is alive.
      resolve()
    })
    req.on('error', (err) => {
      reject(
        new Error(
          `Backend at ${backendUrl} is not reachable (${err.message}). ` +
            `The process may have crashed — check logs for details.`
        )
      )
    })
    req.setTimeout(3000, () => {
      req.destroy()
      reject(new Error(`Backend at ${backendUrl} did not respond within 3 s.`))
    })
    req.end()
  })
}

function resolveLocator(page: Page, roleLabel: string, vars: Record<string, string>): Locator {
  const resolved = interpolate(roleLabel, vars)
  const slashIdx = resolved.indexOf('/')
  const role = resolved.slice(0, slashIdx) as any
  const name = slashIdx === -1 ? undefined : resolved.slice(slashIdx + 1) || undefined
  return name ? page.getByRole(role, { name }) : page.getByRole(role)
}

export async function executeWhen(
  page: Page,
  when: UiWhenItem[],
  ctx: GivenContext
): Promise<void> {
  page.on('console', (msg) => {
    if (msg.type() === 'log') console.log('[BROWSER]', msg.text())
  })
  for (const step of when) {
    if ('navigate' in step) {
      await page.goto(interpolate(step.navigate, ctx.vars))
    } else if ('click' in step) {
      await resolveLocator(page, step.click, ctx.vars).click()
    } else if ('fill' in step) {
      await resolveLocator(page, step.fill, ctx.vars).fill(interpolate(step.value, ctx.vars))
    } else if ('select' in step) {
      await resolveLocator(page, step.select, ctx.vars).selectOption(
        interpolate(step.option, ctx.vars)
      )
    } else if ('submit' in step) {
      await page.keyboard.press('Enter')
    } else if ('keyboard' in step) {
      await page.keyboard.press(step.keyboard)
    } else if ('wait.ms' in step) {
      await page.waitForTimeout(step['wait.ms'])
    } else if ('browser.back' in step) {
      await page.goBack()
    } else if ('click.text' in step) {
      const text = interpolate(step['click.text'], ctx.vars)
      await page.locator('.parent').filter({ hasText: text }).first().click()
    } else if ('click.icon' in step) {
      const iconClass = step['click.icon']
      for (let i = 0; i < 5; i++) {
        const btn = page.locator(`button:has(.${iconClass})`)
        if (await btn.isVisible({ timeout: 2000 }).catch(() => false)) {
          await btn.click()
          break
        }
        if (i < 4) {
          // Check backend health before retrying so crashes surface immediately.
          await assertBackendAlive(ctx.backendUrl)
          await page.waitForTimeout(500)
        } else {
          throw new Error(`Icon button with class "${iconClass}" not found after 5 attempts`)
        }
      }
    } else if ('click.first' in step) {
      for (let i = 0; i < 3; i++) {
        await page.locator('.desktop-small-image').first().click()
        try {
          await page.waitForURL(/\/view\//, { timeout: 3000 })
          break
        } catch {
          if (i < 2) {
            await page.waitForTimeout(500)
          } else {
            throw new Error('click.first did not navigate to photo detail view')
          }
        }
      }
    } else if ('click.testid' in step) {
      await page.getByTestId(step['click.testid']).click()
    } else if ('click.select_first' in step) {
      // .icon-hover is hidden by `.parent:not(:hover) .child { display:none }` CSS.
      // Playwright's visibility check fires before the mouse moves, so even force:true
      // fails. Dispatch a synthetic click directly to bypass the display:none guard.
      // Tiles unmount/remount during route transitions and prefetch refreshes
      // (BufferRowBlock re-keys on prefetchStore.timestamp), so wait and dispatch in
      // a single browser-side task to avoid a stale-DOM race.
      await page.evaluate(async () => {
        const deadline = Date.now() + 10000
        let icon: HTMLElement | null = null
        while (Date.now() < deadline) {
          icon = document.querySelector<HTMLElement>('.parent .icon-hover')
          if (icon) break
          await new Promise((resolve) => setTimeout(resolve, 100))
        }
        if (!icon) throw new Error('.icon-hover not found in DOM')
        icon.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
      })
      // Wait for the edit-mode toolbar (three-dots menu button) to appear
      await page.getByTestId('batch-menu').waitFor({ state: 'visible', timeout: 5000 })
    } else if ('upload.files' in step) {
      const spec = step['upload.files']
      const files = spec.files.map((f) => interpolate(f, ctx.vars))
      const chooserPromise = page.waitForEvent('filechooser')
      await clickTrigger(page, spec.trigger, ctx.vars)
      const chooser = await chooserPromise
      await chooser.setFiles(files)
    } else if ('set.auto_rename' in step) {
      const desired = step['set.auto_rename']
      const dialog = page.locator('#upload-options-modal')
      const switchInput = dialog.locator('input[type="checkbox"]')
      await dialog.waitFor({ state: 'visible', timeout: 5000 })
      const current = await switchInput.isChecked()
      if (current !== desired) {
        await page.getByTestId('upload-auto-rename').click()
      }
    } else {
      throw new Error(
        `Unknown when verb in step ${JSON.stringify(step)}. ` +
          `Expected one of: navigate, click, fill, select, submit, keyboard, wait.ms, browser.back, click.text, click.icon, click.first, click.testid, click.select_first, upload.files, set.auto_rename`
      )
    }
  }
}

/**
 * Click the element referenced by a `trigger` spec. Supports the same
 * reference forms as the click verbs: `icon/<class>` (retries up to 5×),
 * `testid/<id>`, and `role/name` (role only if no name given).
 */
async function clickTrigger(
  page: Page,
  trigger: string,
  vars: Record<string, string>
): Promise<void> {
  if (trigger.startsWith('icon/')) {
    const iconClass = trigger.slice('icon/'.length)
    for (let i = 0; i < 5; i++) {
      const btn = page.locator(`button:has(.${iconClass})`)
      if (await btn.isVisible({ timeout: 2000 }).catch(() => false)) {
        await btn.click()
        return
      }
      if (i < 4) {
        await page.waitForTimeout(500)
      }
    }
    throw new Error(`Icon button with class "${iconClass}" not found after 5 attempts`)
  }
  if (trigger.startsWith('testid/')) {
    await page.getByTestId(trigger.slice('testid/'.length)).click()
    return
  }
  await resolveLocator(page, trigger, vars).click()
}

export async function executeAssert(
  page: Page,
  assert: UiAssertItem[],
  ctx: GivenContext,
  tracer?: CoverageTracer
): Promise<void> {
  for (const assertion of assert) {
    const target = assertionTarget(assertion)
    if ('ui.visible' in assertion) {
      tracer?.recordUI('ui.visible', target)
      await expect(resolveLocator(page, assertion['ui.visible'], ctx.vars)).toBeVisible()
    } else if ('ui.hidden' in assertion) {
      tracer?.recordUI('ui.hidden', target)
      await expect(resolveLocator(page, assertion['ui.hidden'], ctx.vars)).not.toBeVisible()
    } else if ('ui.text' in assertion && 'contains' in assertion) {
      tracer?.recordUI('ui.text', target)
      await expect(resolveLocator(page, assertion['ui.text'], ctx.vars)).toContainText(
        interpolate(assertion.contains, ctx.vars)
      )
    } else if ('ui.route' in assertion) {
      tracer?.recordUI('ui.route', target)
      await expect(page).toHaveURL(new RegExp(interpolate(assertion['ui.route'], ctx.vars)))
    } else if ('ui.modal' in assertion) {
      tracer?.recordUI('ui.modal', target)
      const dialog = page.getByRole('dialog')
      if (assertion['ui.modal'] === 'open') {
        await expect(dialog).toBeVisible()
      } else {
        await expect(dialog).not.toBeVisible()
      }
    } else if ('ui.toast' in assertion) {
      tracer?.recordUI('ui.toast', target)
      const toastSpec = assertion['ui.toast']
      const snackbar = page.getByRole('status').or(page.locator('.v-snackbar'))
      await expect(snackbar.first()).toBeVisible({ timeout: 15000 })
      await expect(snackbar.first()).toContainText(interpolate(toastSpec.contains, ctx.vars))
    } else if ('ui.aria_snapshot' in assertion) {
      tracer?.recordUI('ui.aria_snapshot', target)
      await expect(page.locator('body')).toMatchAriaSnapshot({
        name: assertion['ui.aria_snapshot']
      })
    } else if ('api.response' in assertion) {
      const spec = assertion['api.response']
      const url = interpolate(spec.url, ctx.vars)
      const response = await page.request.fetch(url)
      const expected = Array.isArray(spec.status) ? spec.status : [spec.status]
      expect(expected).toContain(response.status())
    } else if ('ui.text_visible' in assertion) {
      tracer?.recordUI('ui.text_visible', target)
      await expect(
        page.getByText(interpolate(assertion['ui.text_visible'], ctx.vars)).first()
      ).toBeVisible()
    } else if ('ui.count' in assertion) {
      tracer?.recordUI('ui.count', target)
      await expect(page.locator(assertion['ui.count'])).toHaveCount(assertion.equals)
    } else if ('ui.sidebar_visible' in assertion) {
      tracer?.recordUI('ui.sidebar_visible', target)
      await expect(page.locator('#abstractData-col')).toContainText(
        interpolate(assertion['ui.sidebar_visible'], ctx.vars)
      )
    } else if ('ui.chip_visible' in assertion) {
      tracer?.recordUI('ui.chip_visible', target)
      await expect(
        page
          .locator('[id="album-chip"], [id="filename-chip"]')
          .filter({ hasText: interpolate(assertion['ui.chip_visible'], ctx.vars) })
          .first()
      ).toBeVisible()
    } else if ('ui.input_value' in assertion) {
      tracer?.recordUI('ui.input_value', target)
      await expect(resolveLocator(page, assertion['ui.input_value'], ctx.vars)).toHaveValue(
        new RegExp(interpolate(assertion.contains, ctx.vars))
      )
    } else if ('file.contains' in assertion) {
      const filePath = path.join(ctx.imageHome, interpolate(assertion['file.contains'], ctx.vars))
      const needle = interpolate(assertion.text, ctx.vars)
      const content = fs.readFileSync(filePath, 'utf-8')
      if (!content.includes(needle)) {
        throw new Error(
          `file.contains: "${filePath}" does not contain "${needle}".\nFile content:\n${content}`
        )
      }
    } else {
      throw new Error(
        `Unknown assert verb in assertion ${JSON.stringify(assertion)}. ` +
          `Expected one of: ui.visible, ui.hidden, ui.text, ui.route, ui.modal, ui.toast, ui.aria_snapshot, api.response, ui.text_visible, ui.count, ui.sidebar_visible, ui.chip_visible, ui.input_value, file.contains`
      )
    }
  }
}

export async function executeSteps(
  page: Page,
  steps: UiStep[],
  ctx: GivenContext,
  tracer?: CoverageTracer
): Promise<void> {
  for (const step of steps) {
    await executeWhen(page, step.when, ctx)
    await executeAssert(page, step.assert, ctx, tracer)
  }
}

function interpolate(value: string, vars: Record<string, string>): string {
  return value.replace(/\$\{(\w+)\}/g, (_, key) => vars[`$${key}`] ?? vars[key] ?? '')
}
