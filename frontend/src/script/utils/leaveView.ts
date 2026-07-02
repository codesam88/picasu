import type { Router, RouteLocationNormalizedLoaded } from 'vue-router'
import { useModalStore } from '@/store/modalStore'

// Closes an open dialog first; only when nothing is open does it act like a
// back button, returning to the page this view was entered from (rather
// than router.back(), which can land outside the app's own history or on a
// stale grid state depending on how this page was reached).
//
// Uses router.replace (not push): leaving the view undoes the "enter this
// view" navigation rather than taking a new forward step. push would leave
// the view page as a live history entry ahead of the grid, so the
// browser's own Back button would immediately re-enter the view just left.
export function leaveView(route: RouteLocationNormalizedLoaded, router: Router): void {
  const modalStore = useModalStore('mainId')
  if (modalStore.hasOpenDialog) {
    modalStore.closeOpenDialog()
    return
  }

  const albumId = typeof route.params.albumId === 'string' ? route.params.albumId : undefined
  const shareId = typeof route.params.shareId === 'string' ? route.params.shareId : undefined
  const parentPage = route.meta.getParentPage(route, albumId, shareId)
  router.replace(parentPage).catch(() => {
    // No-op on navigation aborts (e.g. rapid double activation).
  })
}
