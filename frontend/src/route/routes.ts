// src/router.ts

import { createRouter, createWebHistory, RouteRecordRaw } from 'vue-router'
import type { RouteLocationRaw } from 'vue-router'
import 'vue-router'

import TimelineMain from '@/components/Page/TimelinePage.vue'
import TrashedPage from '@/components/Page/TrashedPage.vue'
import AlbumsPage from '@/components/Page/AlbumsPage.vue'
import AlbumContentsPage from '@/components/Page/AlbumContentsPage.vue'
import VideosPage from '@/components/Page/VideosPage.vue'
import ViewPage from '@/components/View/ViewPage.vue'
import { createRoute } from './createRoute'
import { tagsRoute } from './tagsRoute'
import { linksRoute } from './linksRoute'
import { loginRoute } from './loginRoute'
import { registerRoute } from './registerRoute'
import { shareRoute } from './shareRoute'
import { configRoute } from './configRoute'
import { getServerInfo } from '@/api/serverInfo'
import { useSessionStore } from '@/store/sessionStore'

// ======================================
// Define Simple Static Routes
// ======================================

const simpleRoutes: RouteRecordRaw[] = [
  { path: '/', redirect: '/timeline' },
  tagsRoute,
  linksRoute,
  loginRoute,
  registerRoute,
  shareRoute,
  configRoute
]

// ======================================
// Create Routes Using the Helper Function
// ======================================

const timelinePageRoutes = createRoute('timeline', TimelineMain)

const trashedPageRoutes = createRoute('trashed', TrashedPage)

const albumsPageRoutes = createRoute('albums', AlbumsPage)

const videosPageRoutes = createRoute('videos', VideosPage)

// ======================================
// Combine All Routes
// ======================================

const albumContentRoute: RouteRecordRaw = {
  path: '/album/:albumId',
  component: AlbumContentsPage,
  name: 'album',
  meta: {
    level: 1,
    baseName: 'album',
    getParentPage: (route) => ({
      name: 'timeline',
      params: {},
      query: route.query
    }),
    getChildPage: (route, assetId) => ({
      name: 'albumViewPage',
      params: { albumId: route.params.albumId, assetId: assetId },
      query: route.query
    })
  },
  children: [
    {
      path: 'view/:assetId',
      component: ViewPage,
      name: 'albumViewPage',
      meta: {
        level: 2,
        baseName: 'album',
        getParentPage: (route) => ({
          name: 'album',
          params: { albumId: route.params.albumId },
          query: route.query
        }),
        // No child page below level 2 (level 3/4 were eliminated). Identity fallback
        // since RouteMeta.getChildPage is required by the type but has no real caller here.
        getChildPage: (route) => ({
          name: 'albumViewPage',
          params: { assetId: route.params.assetId },
          query: route.query
        })
      }
    }
  ]
}

const routes: RouteRecordRaw[] = [
  ...simpleRoutes,
  ...timelinePageRoutes,
  ...trashedPageRoutes,
  ...albumsPageRoutes,
  ...videosPageRoutes,
  albumContentRoute
]

// ======================================
// Create and Export the Router Instance
// ======================================

const router = createRouter({
  history: createWebHistory(),
  routes,
  scrollBehavior(_to, _from, savedPosition) {
    if (savedPosition) {
      return savedPosition
    }
    return { top: 0 }
  }
})

// Boot guard: on an empty store with enrollment open the backend reports
// `enrolled: false` + `enrollmentOpen: true`, and no album or config page
// may render before the admin registration completes. A present session
// profile allows immediately with zero fetches; otherwise one fresh
// anonymous `getServerInfo()` ping decides: needs-enrollment forces the
// register screen, anything else (enrolled store, closed install) falls
// through to the normal login. Nothing is cached — the ping runs only
// when sessionless, so there is no stale state to invalidate.
router.beforeEach(async (to) => {
  // Share links are capability URLs: they authenticate out-of-band and
  // must work without login — including on installs where the ping below
  // reports a closed install. Never redirect them.
  if (to.name === 'share' || to.name === 'shareViewPage') {
    return true
  }
  if (useSessionStore('mainId').profile !== null) {
    return true
  }
  const { enrolled, enrollmentOpen } = await getServerInfo()
  if (!enrolled && enrollmentOpen) {
    return to.name === 'register' ? true : { name: 'register' }
  }
  // Enrolled store or closed install: sessionless means login. This also
  // absorbs the deleted RegisterPage leave-redirect — a sessionless visit
  // to /register lands on /login instead of rendering a dead form.
  return to.name === 'login' ? true : { name: 'login' }
})

// Update the browser tab title based on the current route
router.afterEach((to) => {
  const baseName =
    typeof to.meta.baseName === 'string'
      ? to.meta.baseName
      : typeof to.name === 'string'
        ? to.name
        : undefined

  const baseTitleMap: Record<string, string> = {
    timeline: 'Timeline',
    trashed: 'Trash',
    albums: 'Albums',
    videos: 'Videos',
    album: 'Album',
    tags: 'Tags',
    links: 'Links',
    login: 'Login',
    register: 'Register',
    share: 'Share',
    config: 'Configuration'
  }

  let baseTitle: string
  if (baseName != null && baseName !== '') {
    baseTitle = baseTitleMap[baseName] ?? baseName
  } else {
    baseTitle = typeof to.name === 'string' ? to.name : 'Picasu'
  }
  const isView = typeof to.meta.isViewPage === 'boolean' ? to.meta.isViewPage : false

  // When on a View page, append the assetId to the title
  let suffix = ''
  if (isView) {
    const maybeAssetId = typeof to.params.assetId === 'string' ? to.params.assetId : undefined
    if (maybeAssetId != null && maybeAssetId !== '') {
      suffix = `View ${maybeAssetId}`
    } else {
      suffix = 'View'
    }
  }

  const pageTitle = suffix ? `${baseTitle} ${suffix}` : baseTitle

  document.title = `${pageTitle} - Picasu`
})

// On first app load, if user lands directly on a view page (level 2),
// synthesize the parent (level 1) entry in history so a simple router.back() returns to it.
void router.isReady().then(async () => {
  const to = router.currentRoute.value

  const meta = to.meta

  // Only act on initial load for the view page (level 2 is the only nested level now)
  const isNested = meta.level > 1
  if (isNested) {
    const routeName = typeof to.name === 'string' ? to.name : undefined
    const baseName = typeof meta.baseName === 'string' ? meta.baseName : undefined
    if (routeName === undefined || baseName === undefined) return

    const q = to.query

    // Level-1 routes that accept path params need them included in the ancestor entry.
    const level1Params: Record<string, string | undefined> = {}
    const albumId = to.params.albumId
    if (typeof albumId === 'string') {
      level1Params.albumId = albumId
    }

    if (routeName === `${baseName}ViewPage`) {
      const target: RouteLocationRaw = { path: to.fullPath }
      try {
        await router.replace({ name: baseName, params: level1Params, query: q })
        await router.push(target)
      } catch {
        // No-op on navigation aborts
      }
    }
  }
})

export default router
