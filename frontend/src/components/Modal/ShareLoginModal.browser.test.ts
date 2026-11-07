import { describe, expect, it, vi } from 'vitest'
import { page, userEvent } from 'vitest/browser'
import { render } from 'vitest-browser-vue'
import { defineComponent, h } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import ShareLoginModal from './ShareLoginModal.vue'
import { createTestVuetify } from '@/components/testing/browserSetup'
import { useModalStore } from '@/store/modalStore'
import { useShareStore } from '@/store/shareStore'

/// Expired links must offer an exit (Close/X/Escape) that lands on `/`;
/// the password prompt stays must-complete with no close affordance and
/// no navigation. Dialog content is teleported to document.body by
/// Vuetify; `page` locators are body-scoped.
const RouteView = defineComponent({ render: () => h('div') })

const sharePath = '/share/album1-share1'

function createTestRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', name: 'timeline', component: RouteView },
      { path: '/share/:albumId-:shareId', name: 'share', component: RouteView }
    ]
  })
}

async function mountModal(expired: boolean) {
  const pinia = createPinia()
  setActivePinia(pinia)

  const modalStore = useModalStore('mainId')
  const shareStore = useShareStore('mainId')
  shareStore.isLinkExpired = expired
  modalStore.showShareLoginModal = true

  const router = createTestRouter()
  await router.push(sharePath)
  await router.isReady()

  const result = await render(ShareLoginModal, {
    global: {
      plugins: [createTestVuetify(), pinia, router],
      // VTU stubs transitions by default; Vuetify's dialog focuses its
      // content in the `afterEnter` hook, which a stub never fires.
      stubs: { transition: false, 'transition-group': false }
    }
  })

  return { modalStore, router, result }
}

describe('ShareLoginModal expired state', () => {
  it('offers a Close button that closes the dialog and returns home', async () => {
    const { modalStore, router, result } = await mountModal(true)

    await expect.element(page.getByRole('dialog')).toBeVisible()

    await page.getByRole('button', { name: 'Close' }).click()

    await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()
    expect(modalStore.showShareLoginModal).toBe(false)
    await vi.waitFor(() => {
      expect(router.currentRoute.value.path).toBe('/')
    })

    await result.unmount()
  })

  it('shows the header close button and closes on Escape back to `/`', async () => {
    const { modalStore, router, result } = await mountModal(true)

    await expect.element(page.getByTestId('modal-close')).toBeVisible()

    await userEvent.keyboard('{Escape}')

    await expect.element(page.getByRole('dialog')).not.toBeInTheDocument()
    expect(modalStore.showShareLoginModal).toBe(false)
    await vi.waitFor(() => {
      expect(router.currentRoute.value.path).toBe('/')
    })

    await result.unmount()
  })
})

describe('ShareLoginModal password state', () => {
  it('stays must-complete: no close button, no header X, Escape is swallowed, no navigation', async () => {
    const { modalStore, router, result } = await mountModal(false)

    await expect.element(page.getByRole('dialog')).toBeVisible()
    await expect.element(page.getByRole('button', { name: 'Unlock' })).toBeVisible()
    await expect.element(page.getByTestId('modal-close')).not.toBeInTheDocument()
    await expect.element(page.getByRole('button', { name: 'Close' })).not.toBeInTheDocument()

    await userEvent.keyboard('{Escape}')

    await expect.element(page.getByRole('dialog')).toBeVisible()
    expect(modalStore.showShareLoginModal).toBe(true)
    expect(router.currentRoute.value.path).toBe(sharePath)

    await result.unmount()
  })
})
