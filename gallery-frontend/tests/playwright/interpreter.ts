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
    } else {
      throw new Error(
        `Unknown when verb in step ${JSON.stringify(step)}. ` +
          `Expected one of: navigate, click, fill, select, submit, wait.ms`
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
    } else {
      throw new Error(
        `Unknown assert verb in assertion ${JSON.stringify(assertion)}. ` +
          `Expected one of: ui.visible, ui.hidden, ui.text, ui.route, ui.modal, ui.toast, ui.aria_snapshot, api.response`
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
