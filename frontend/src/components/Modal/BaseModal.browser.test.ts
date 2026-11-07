import { describe, expect, it } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { render } from 'vitest-browser-vue'
import { defineComponent, h, ref } from 'vue'
import BaseModal from './BaseModal.vue'
import { createTestVuetify } from '@/components/testing/browserSetup'

/// BaseModal is controlled (`v-model`), so the test mounts a tiny host that
/// carries the open state. The dialog content is teleported to `document.body`
/// by Vuetify; `page` locators are body-scoped, so they cover the portal.
async function mountModal(initialOpen: boolean, extraProps: Record<string, unknown> = {}) {
  const open = ref(initialOpen)

  const Host = defineComponent({
    setup() {
      return () =>
        h(
          BaseModal,
          {
            modelValue: open.value,
            title: 'Test dialog',
            ...extraProps,
            'onUpdate:modelValue': (value: boolean) => {
              open.value = value
            }
          },
          {
            default: () => h('p', { 'data-testid': 'modal-body' }, 'Dialog body')
          }
        )
    }
  })

  return {
    open,
    result: await render(Host, {
      global: {
        plugins: [createTestVuetify()],
        // VTU stubs transitions by default; Vuetify's dialog focuses its
        // content in the `afterEnter` hook, which a stub never fires.
        stubs: { transition: false, 'transition-group': false }
      }
    })
  }
}

describe('BaseModal accessibility contract', () => {
  it('renders a visible modal dialog with aria-modal and moves focus inside', async () => {
    const { result } = await mountModal(true)

    const dialog = page.getByRole('dialog')
    await expect.element(dialog).toBeVisible()
    await expect.element(dialog).toHaveAttribute('aria-modal', 'true')
    await expect.element(page.getByText('Dialog body')).toBeVisible()

    const dialogEl = dialog.element()
    await expect
      .poll(() => {
        const active = dialogEl.ownerDocument.activeElement
        return active !== null && dialogEl.contains(active)
      })
      .toBe(true)

    await result.unmount()
  })

  it('closes from the close button and hides the modal content', async () => {
    const { open, result } = await mountModal(true)

    await expect.element(page.getByText('Dialog body')).toBeVisible()

    await page.getByTestId('modal-close').click()

    await expect.element(page.getByText('Dialog body')).not.toBeInTheDocument()
    expect(open.value).toBe(false)

    await result.unmount()
  })

  it('renders no dialog while closed', async () => {
    const { result } = await mountModal(false)

    await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()

    await result.unmount()
  })
})

describe('BaseModal Escape handling', () => {
  it('closes an open dialog on Escape', async () => {
    const { open, result } = await mountModal(true)

    await expect.element(page.getByText('Dialog body')).toBeVisible()

    await userEvent.keyboard('{Escape}')

    await expect.element(page.getByText('Dialog body')).not.toBeInTheDocument()
    expect(open.value).toBe(false)

    await result.unmount()
  })

  it('ignores Escape while loading', async () => {
    const { open, result } = await mountModal(true, { loading: true })

    await expect.element(page.getByText('Dialog body')).toBeVisible()

    await userEvent.keyboard('{Escape}')

    await expect.element(page.getByText('Dialog body')).toBeVisible()
    expect(open.value).toBe(true)

    await result.unmount()
  })

  it('ignores Escape when the close button is hidden', async () => {
    const { open, result } = await mountModal(true, { hideClose: true })

    await expect.element(page.getByText('Dialog body')).toBeVisible()

    await userEvent.keyboard('{Escape}')

    await expect.element(page.getByText('Dialog body')).toBeVisible()
    expect(open.value).toBe(true)

    await result.unmount()
  })

  it('ignores Escape while closed', async () => {
    const { open, result } = await mountModal(false)

    await userEvent.keyboard('{Escape}')

    await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()
    expect(open.value).toBe(false)

    await result.unmount()
  })
})
