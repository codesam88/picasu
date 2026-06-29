import type { Page, Locator } from '@playwright/test'
import { expect } from '@playwright/test'
import { UiWhenItem, UiAssertItem, UiStep } from './types'
import { GivenContext } from './executeGiven'
import { CoverageTracer, assertionTarget } from './tracer'

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
    } else if ('wait.ms' in step) {
      await page.waitForTimeout(step['wait.ms'])
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
    } else if ('click.select_first' in step) {
      // .icon-hover is hidden by `.parent:not(:hover) .child { display:none }` CSS.
      // Playwright's visibility check fires before the mouse moves, so even force:true
      // fails. Dispatch a synthetic click directly to bypass the display:none guard.
      await page.locator('.parent').first().waitFor({ state: 'visible', timeout: 10000 })
      await page.evaluate(() => {
        const icon = document.querySelector<HTMLElement>('.parent .icon-hover')
        if (!icon) throw new Error('.icon-hover not found in DOM')
        icon.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }))
      })
      // Wait for the edit-mode toolbar (three-dots menu button) to appear
      await page
        .locator('button:has(.mdi-dots-vertical)')
        .waitFor({ state: 'visible', timeout: 5000 })
    } else {
      throw new Error(
        `Unknown when verb in step ${JSON.stringify(step)}. ` +
          `Expected one of: navigate, click, fill, select, submit, wait.ms, click.text, click.icon, click.first, click.select_first`
      )
    }
  }
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
      await expect(snackbar.first()).toBeVisible({ timeout: 5000 })
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
    } else {
      throw new Error(
        `Unknown assert verb in assertion ${JSON.stringify(assertion)}. ` +
          `Expected one of: ui.visible, ui.hidden, ui.text, ui.route, ui.modal, ui.toast, ui.aria_snapshot, api.response, ui.text_visible, ui.count, ui.sidebar_visible, ui.chip_visible`
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
