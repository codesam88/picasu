import type { Page, Locator } from '@playwright/test'
import { expect } from '@playwright/test'
import { UiWhenItem, UiThenItem } from './types'
import { GivenContext } from './executeGiven'

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
      await resolveLocator(page, step.fill, ctx.vars).fill(
        interpolate(step.value, ctx.vars)
      )
    } else if ('select' in step) {
      await resolveLocator(page, step.select, ctx.vars).selectOption(
        interpolate(step.option, ctx.vars)
      )
    } else if ('submit' in step) {
      await page.keyboard.press('Enter')
    } else {
      throw new Error(
        `Unknown when verb in step ${JSON.stringify(step)}. ` +
          `Expected one of: navigate, click, fill, select, submit`
      )
    }
  }
}

export async function executeThen(
  page: Page,
  then: UiThenItem[],
  ctx: GivenContext
): Promise<void> {
  for (const assertion of then) {
    if ('ui.visible' in assertion) {
      await expect(
        resolveLocator(page, assertion['ui.visible'], ctx.vars)
      ).toBeVisible()
    } else if ('ui.hidden' in assertion) {
      await expect(
        resolveLocator(page, assertion['ui.hidden'], ctx.vars)
      ).not.toBeVisible()
    } else if ('ui.text' in assertion && 'contains' in assertion) {
      await expect(
        resolveLocator(page, assertion['ui.text'], ctx.vars)
      ).toContainText(interpolate(assertion.contains, ctx.vars))
    } else if ('ui.route' in assertion) {
      await expect(page).toHaveURL(
        new RegExp(interpolate(assertion['ui.route'], ctx.vars))
      )
    } else if ('ui.modal' in assertion) {
      const dialog = page.getByRole('dialog')
      if (assertion['ui.modal'] === 'open') {
        await expect(dialog).toBeVisible()
      } else {
        await expect(dialog).not.toBeVisible()
      }
    } else if ('ui.toast' in assertion) {
      const toastSpec = assertion['ui.toast']
      const snackbar = page.getByRole('status').or(page.locator('.v-snackbar'))
      await expect(snackbar.first()).toBeVisible({ timeout: 5000 })
      await expect(snackbar.first()).toContainText(
        interpolate(toastSpec.contains, ctx.vars)
      )
    } else if ('ui.aria_snapshot' in assertion) {
      await expect(page.locator('body')).toMatchAriaSnapshot({
        name: assertion['ui.aria_snapshot']
      })
    } else {
      throw new Error(
        `Unknown then verb in assertion ${JSON.stringify(assertion)}. ` +
          `Expected one of: ui.visible, ui.hidden, ui.text, ui.route, ui.modal, ui.toast, ui.aria_snapshot`
      )
    }
  }
}

function interpolate(value: string, vars: Record<string, string>): string {
  return value.replace(/\$\{(\w+)\}/g, (_, key) => vars[key] ?? '')
}
