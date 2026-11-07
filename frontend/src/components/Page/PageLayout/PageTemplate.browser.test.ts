import { describe, expect, it } from 'vitest'
import { page } from 'vitest/browser'
import { render } from 'vitest-browser-vue'
import { defineComponent, h } from 'vue'
import { createPinia, setActivePinia } from 'pinia'
import { createMemoryHistory, createRouter } from 'vue-router'
import PageTemplate from './PageTemplate.vue'
import { createTestVuetify } from '@/components/testing/browserSetup'
import { useConfigStore } from '@/store/configStore'
import type { AppConfig } from '@/api/config'

const RouteView = defineComponent({ render: () => h('div') })

const parentPage = { name: 'timeline', params: {}, query: {} }

function pageMeta(
  baseName: 'timeline' | 'albums' | 'my-albums' | 'album' | 'tags' | 'shares' | 'config' | 'trashed'
) {
  return {
    baseName,
    level: 1,
    getParentPage: () => parentPage,
    getChildPage: () => parentPage
  }
}

function createTestRouter() {
  return createRouter({
    history: createMemoryHistory(),
    routes: [
      { path: '/', name: 'timeline', component: RouteView, meta: pageMeta('timeline') },
      { path: '/albums', name: 'albums', component: RouteView, meta: pageMeta('albums') },
      {
        path: '/albums/:albumId',
        name: 'album',
        component: RouteView,
        meta: pageMeta('album')
      },
      {
        path: '/my-albums',
        name: 'my-albums',
        component: RouteView,
        meta: pageMeta('my-albums')
      },
      { path: '/tags', name: 'tags', component: RouteView, meta: pageMeta('tags') },
      { path: '/shares', name: 'shares', component: RouteView, meta: pageMeta('shares') },
      {
        path: '/config',
        name: 'config',
        component: RouteView,
        meta: pageMeta('config')
      },
      {
        path: '/trashed',
        name: 'trashed',
        component: RouteView,
        meta: pageMeta('trashed')
      }
    ]
  })
}

const testConfig: AppConfig = {
  limits: {},
  readOnlyMode: false,
  fsNotifyWatcher: false,
  authKey: null,
  uploadFolder: null,
  maxUploadSize: '0'
}

async function mountTemplate(ready: boolean) {
  const pinia = createPinia()
  setActivePinia(pinia)
  // Seeding the config keeps NavPanel's onMounted config fetch out of the test.
  useConfigStore('mainId').config = testConfig

  const router = createTestRouter()
  await router.push('/')
  await router.isReady()

  const slotMarker = (testId: string) =>
    h('span', { 'data-testid': testId }, testId.replace('-slot', ' slot'))

  return await render(PageTemplate, {
    props: { ready, containerId: 'page-container' },
    slots: {
      overlay: () => slotMarker('overlay-slot'),
      content: () => slotMarker('content-slot'),
      loading: () => slotMarker('loading-slot')
    },
    global: {
      plugins: [createTestVuetify(), pinia, router],
      stubs: { transition: false, 'transition-group': false }
    }
  })
}

describe('PageTemplate slot contract', () => {
  it('renders the overlay slot outside and the content slot inside the page container', async () => {
    await mountTemplate(true)

    const content = page.getByTestId('content-slot')
    const overlay = page.getByTestId('overlay-slot')

    await expect.element(content).toBeVisible()
    await expect.element(overlay).toBeVisible()

    expect(content.element().closest('#page-container')).not.toBeNull()
    expect(overlay.element().closest('#page-container')).toBeNull()
  })

  it('replaces the content slot with the loading slot while not ready', async () => {
    await mountTemplate(false)

    await expect.element(page.getByTestId('loading-slot')).toBeVisible()
    await expect.element(page.getByTestId('content-slot')).not.toBeInTheDocument()
    await expect.element(page.getByTestId('overlay-slot')).toBeVisible()
  })

  it('does not render the loading slot once ready', async () => {
    await mountTemplate(true)

    await expect.element(page.getByTestId('content-slot')).toBeVisible()
    await expect.element(page.getByTestId('loading-slot')).not.toBeInTheDocument()
  })
})
