import axios, { AxiosError, InternalAxiosRequestConfig } from 'axios'
import { AUTHENTICATE_URL, retryAfterSecs, throttleMessage } from '@/script/utils/retryAfter'
import { errorDisplay } from '@/script/utils/errorDisplay'
import { useSessionStore } from '@/store/sessionStore'
import { useShareStore } from '@/store/shareStore'
import { useModalStore } from '@/store/modalStore'
import { useMessageStore } from '@/store/messageStore'
import { useRedirectionStore } from '@/store/redirectionStore'

const HEADERS = {
  ALBUM_ID: 'x-album-id',
  SHARE_ID: 'x-share-id',
  SHARE_PASSWORD: 'x-share-password'
}

// --- Helpers ---

/**
 * Aggregates stores to avoid repetitive useStore calls.
 * Note: Must be called inside the interceptor to ensure Pinia is active.
 */
const getStores = () => ({
  shareStore: useShareStore('mainId'),
  modalStore: useModalStore('mainId'),
  messageStore: useMessageStore('mainId'),
  redirectionStore: useRedirectionStore('mainId')
})

// --- Error Handlers ---

/**
 * Handles errors specific to the Public Share / Album context.
 */
function handleShareError(error: AxiosError, stores: ReturnType<typeof getStores>) {
  const { shareStore, modalStore, messageStore } = stores
  const status = error.response?.status

  // 1. Handle 401: Password Required / Stale Request
  if (status === 401) {
    // eslint-disable-next-line @typescript-eslint/no-unsafe-assignment, @typescript-eslint/no-unnecessary-condition
    const sentPassword = error.config?.headers?.[HEADERS.SHARE_PASSWORD]
    const currentPassword = shareStore.password

    // Check for "Zombie" requests (stale password sent vs current store state)
    /* eslint-disable @typescript-eslint/no-unnecessary-condition */
    if (
      currentPassword !== null &&
      currentPassword !== undefined &&
      sentPassword !== currentPassword
    ) {
      /* eslint-enable @typescript-eslint/no-unnecessary-condition */
      return // Ignore stale request
    }

    if (!modalStore.showShareLoginModal) {
      shareStore.isLinkExpired = false
      modalStore.showShareLoginModal = true
    }
    return
  }

  // 2. Handle 403: Link Expired / Access Denied
  if (status === 403) {
    const displayMsg = errorDisplay(error)
    messageStore.error(
      displayMsg !== 'Unknown error occurred'
        ? displayMsg
        : 'Share link has expired or access is denied.'
    )

    if (!modalStore.showShareLoginModal) {
      shareStore.isLinkExpired = true
      modalStore.showShareLoginModal = true
    }
  }
}

/**
 * Handles standard application errors (Auth, Permissions, Generic).
 */
async function handleGeneralError(error: AxiosError, stores: ReturnType<typeof getStores>) {
  const { messageStore, redirectionStore } = stores
  const status = error.response?.status

  switch (status) {
    case 401:
      // Tier taxonomy: the backend means 401 to be "the presented cookie
      // was rejected" — a session-tier failure. The only other source is
      // the failed login itself (POST /post/authenticate, a wired 401),
      // which the login page owns its display for; it is caller-owned.
      if (error.config?.url === AUTHENTICATE_URL) break
      // The session is gone server-side: drop the persisted display
      // profile too, so a stale admin badge can never outlive the
      // session that backed it. No snackbar here: the login page
      // always shows the innocuous "Please log in to continue" banner
      // on arrival. Share pages never reach here — their 401s are
      // share-password flow, not sessions. Repeat 401s cannot
      // double-navigate: redirectionToLogin is a no-op once the route
      // is login.
      useSessionStore('mainId').clearProfile()
      await redirectionStore.redirectionToLogin()
      break

    case 403: {
      const displayMsg = errorDisplay(error)
      messageStore.error(displayMsg !== 'Unknown error occurred' ? displayMsg : 'Access denied.')
      break
    }

    case 405:
      messageStore.error('Read only mode is on.')
      break

    case 429: {
      // A 429 is a temporary limiter, not a dead session: the profile is
      // kept and no redirect happens. LoginPage owns the authenticate
      // message (inline countdown banner), so no toast there; any other
      // route gets one toast carrying the retry hint.
      if (error.config?.url === AUTHENTICATE_URL) break
      const secs = retryAfterSecs(error)
      messageStore.error(secs !== null ? throttleMessage(secs) : errorDisplay(error))
      break
    }

    default: {
      // Generic error handler (400, 404, 500, etc.)
      const displayMsg = errorDisplay(error)
      messageStore.error(displayMsg)
      break
    }
  }
}

/**
 * Main Error Interceptor Logic
 */
async function handleAxiosResponseError(error: AxiosError) {
  if (!error.response) return Promise.reject(error)

  const stores = getStores()
  /* eslint-disable @typescript-eslint/no-unnecessary-condition */
  const isSharePage =
    stores.shareStore.albumId !== null &&
    stores.shareStore.albumId !== undefined &&
    stores.shareStore.shareId !== null &&
    stores.shareStore.shareId !== undefined
  /* eslint-enable @typescript-eslint/no-unnecessary-condition */

  if (isSharePage) {
    handleShareError(error, stores)
  } else {
    await handleGeneralError(error, stores)
  }

  return Promise.reject(error)
}

// --- Interceptor Setup ---

export function setupMainAxiosInterceptor() {
  // Request Interceptor
  axios.interceptors.request.use((config: InternalAxiosRequestConfig) => {
    const shareStore = useShareStore('mainId')

    // Only attach share headers if both IDs exist
    if (typeof shareStore.albumId === 'string' && typeof shareStore.shareId === 'string') {
      config.headers.set(HEADERS.ALBUM_ID, shareStore.albumId)
      config.headers.set(HEADERS.SHARE_ID, shareStore.shareId)

      /* eslint-disable @typescript-eslint/no-unnecessary-condition */
      if (
        shareStore.password !== null &&
        shareStore.password !== undefined &&
        shareStore.password !== ''
      ) {
        /* eslint-enable @typescript-eslint/no-unnecessary-condition */
        config.headers.set(HEADERS.SHARE_PASSWORD, shareStore.password)
      }
    }

    return config
  })

  // Response Interceptor
  axios.interceptors.response.use((response) => response, handleAxiosResponseError)
}
