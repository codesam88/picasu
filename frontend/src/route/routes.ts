// src/router.ts

import { createRouter, createWebHistory, RouteRecordRaw } from 'vue-router'
import type { RouteLocationRaw } from 'vue-router'
import 'vue-router'

import TimelineMain from '@/components/Page/TimelinePage.vue'
import FavoritePage from '@/components/Page/FavoritePage.vue'
import ArchivedPage from '@/components/Page/ArchivedPage.vue'
import TrashedPage from '@/components/Page/TrashedPage.vue'
import AlbumsPage from '@/components/Page/AlbumsPage.vue'
import AlbumContentsPage from '@/components/Page/AlbumContentsPage.vue'
import VideosPage from '@/components/Page/VideosPage.vue'
import ViewPage from '@/components/View/ViewPage.vue'
import { createRoute } from './createRoute'
import { tagsRoute } from './tagsRoute'
import { linksRoute } from './linksRoute'
import { loginRoute } from './loginRoute'
import { shareRoute } from './shareRoute'
import { configRoute } from './configRoute'

// ======================================
// Define Simple Static Routes
// ======================================

const simpleRoutes: RouteRecordRaw[] = [
  { path: '/', redirect: '/timeline' },
  tagsRoute,
  linksRoute,
  loginRoute,
  shareRoute,
  configRoute
]

// ======================================
// Create Routes Using the Helper Function
// ======================================

const timelinePageRoutes = createRoute('timeline', TimelineMain)

const favoritePageRoutes = createRoute('favorite', FavoritePage)

const archivedPageRoutes = createRoute('archived', ArchivedPage)

const trashedPageRoutes = createRoute('trashed', TrashedPage)

const albumsPageRoutes = createRoute('albums', AlbumsPage)

const videosPageRoutes = createRoute('videos', VideosPage)

// ======================================
// Combine All Routes
// ======================================

const albumContentRoute: RouteRecordRaw = {
  path: '/album/:albumHash',
  component: AlbumContentsPage,
  name: 'album',
  meta: {
    level: 1,
    baseName: 'album',
    getParentPage: (route) => ({
      name: 'timeline',
      params: { hash: undefined, subhash: undefined },
      query: route.query
    }),
    getChildPage: (route, hash) => ({
      name: 'albumViewPage',
      params: { albumHash: route.params.albumHash, hash: hash, subhash: undefined },
      query: route.query
    })
  },
  children: [
    {
      path: 'view/:hash',
      component: ViewPage,
      name: 'albumViewPage',
      meta: {
        level: 2,
        baseName: 'album',
        getParentPage: (route) => ({
          name: 'album',
          params: { albumHash: route.params.albumHash, hash: undefined, subhash: undefined },
          query: route.query
        }),
        // No child page below level 2 (level 3/4 were eliminated). Identity fallback
        // since RouteMeta.getChildPage is required by the type but has no real caller here.
        getChildPage: (route) => ({
          name: 'albumViewPage',
          params: { hash: route.params.hash, subhash: undefined },
          query: route.query
        })
      }
    }
  ]
}

const routes: RouteRecordRaw[] = [
  ...simpleRoutes,
  ...timelinePageRoutes,
  ...favoritePageRoutes,
  ...archivedPageRoutes,
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
  routes
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
    favorite: 'Favorites',
    archived: 'Archived',
    trashed: 'Trash',
    albums: 'Albums',
    videos: 'Videos',
    album: 'Album',
    tags: 'Tags',
    links: 'Links',
    login: 'Login',
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

  // When on a View page, append the hash to the title
  let suffix = ''
  if (isView) {
    const maybeHash = typeof to.params.hash === 'string' ? to.params.hash : undefined
    if (maybeHash != null && maybeHash !== '') {
      suffix = `View ${maybeHash}`
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
    const albumHash = to.params.albumHash
    if (typeof albumHash === 'string') {
      level1Params.albumHash = albumHash
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
