import { readAndCompressImage } from '@misskey-dev/browser-image-resizer'
import { bindActionDispatch, createHandler } from 'typesafe-agent-events'
import { fromImgWorker, toImgWorker } from '@/worker/workerApi'
import {
  ProcessAbortPayload,
  ProcessImagePayload,
  ProcessSmallImagePayload
} from '@/worker/workerApi'
import axios from 'axios'
import { getSrc } from '@utils/getter'
import { setupWorkerAxiosInterceptor } from './workerAxiosInterceptor'
import {
  getOrEncodeThumbnail,
  LruCache,
  RESIZED_BLOB_CACHE_LIMIT,
  SOURCE_BLOB_CACHE_LIMIT,
  thumbnailCacheKey
} from './imgCache'

const postToMainImg = bindActionDispatch(fromImgWorker, self.postMessage.bind(self))
const controllerMap = new Map<number, AbortController>()
const workerAxios = axios.create()

// No retries here by design: a failed fetch (expired token, 401, network
// error) goes straight to the catch block, and recovery is owned by the
// token/prefetch layer (silent renewal, fresh snapshot) — retrying the same
// stale request would only repeat the failure.

setupWorkerAxiosInterceptor(workerAxios, { notify: postToMainImg.notification }, 'image')

// ================== Blob Cache ==================

const IMG_CACHE_NAME = 'img-blob-cache-v1'

/**
 * In-memory cache of fetched image Blobs, keyed by assetId (LRU-bounded).
 *
 * A thumbnail-size or window resize re-queues visible images under a new
 * output key. Without this cache every re-queue would repeat the full network
 * fetch; with it the worker skips straight to the encode step below. The cache
 * lives for the lifetime of the worker (terminated on component unmount via
 * workerStore.terminateWorker()).
 */
const blobCache = new LruCache<string, Blob>(SOURCE_BLOB_CACHE_LIMIT)

/**
 * In-memory cache of already re-encoded thumbnails, keyed by
 * (assetId, width, height, dpr, albumMode). Survives gallery refreshes and
 * resizes, so re-queued tiles skip decode + resize + JPEG encode entirely.
 */
const resizedBlobCache = new LruCache<string, Blob>(RESIZED_BLOB_CACHE_LIMIT)

/**
 * Grid thumbnails are re-encoded at 0.8 rather than the library default (0.5)
 * or the previous maximum (1). At tile size 0.8 and 1.0 are not visibly
 * distinguishable, while quality 1 forces the least-quantised JPEG tables and
 * a larger blob for no visible gain. 0.8 stays above the library default,
 * which is tuned for much smaller previews.
 */
const THUMBNAIL_QUALITY = 0.8

function imgCacheKey(hash: string): string {
  return `https://img-blob-cache.internal/${hash}`
}

async function getFromDiskCache(hash: string): Promise<Blob | undefined> {
  if (typeof caches === 'undefined') return undefined
  try {
    const cache = await caches.open(IMG_CACHE_NAME)
    const response = await cache.match(imgCacheKey(hash))
    if (response) {
      const blob = await response.blob()
      if (blob.size > 0) return blob
      await cache.delete(imgCacheKey(hash))
    }
  } catch {
    // Cache API unavailable
  }
  return undefined
}

async function putToDiskCache(hash: string, blob: Blob): Promise<void> {
  if (typeof caches === 'undefined') return
  try {
    const cache = await caches.open(IMG_CACHE_NAME)
    await cache.put(imgCacheKey(hash), new Response(blob))
  } catch {
    // Non-critical
  }
}

async function purgeFromCaches(assetId: string): Promise<void> {
  blobCache.delete(assetId)
  resizedBlobCache.deleteWhere((key) => key.startsWith(`${assetId}|`))
  if (typeof caches === 'undefined') return
  try {
    const cache = await caches.open(IMG_CACHE_NAME)
    await cache.delete(imgCacheKey(assetId))
  } catch {
    // Non-critical
  }
}

// ================== Handlers ==================

const handler = createHandler<typeof toImgWorker>({
  async processSmallImage(event: ProcessSmallImagePayload) {
    try {
      const controller = new AbortController()
      controllerMap.set(event.index, controller)

      const cacheKey = event.assetId

      // Layer 1: In-memory cache
      let blob = blobCache.get(cacheKey)

      // Layer 2: Cache API (disk-persistent)
      if (blob === undefined) {
        blob = await getFromDiskCache(cacheKey)
        if (blob !== undefined) {
          blobCache.set(cacheKey, blob)
        }
      }

      // Layer 3: Network fetch
      if (blob === undefined) {
        const headers: Record<string, string> = {}
        if (event.albumId !== null) headers['x-album-id'] = event.albumId
        if (event.shareId !== null) headers['x-share-id'] = event.shareId
        // eslint-disable-next-line @typescript-eslint/strict-boolean-expressions
        if (event.password) headers['x-share-password'] = event.password

        headers.Authorization = `Bearer ${event.hashToken}`

        const config = {
          signal: controller.signal,
          responseType: 'blob' as const,
          headers,
          timestampToken: event.timestampToken
        }

        const response = await workerAxios.get<Blob>(
          getSrc(event.hash, false, 'jpg', event.updatedAt),
          config
        )

        blob = response.data
        blobCache.set(cacheKey, blob)
        void putToDiskCache(cacheKey, blob)
      }

      controllerMap.delete(event.index)

      const albumMode = event.albumMode === true
      const resizedKey = thumbnailCacheKey({
        assetId: event.assetId,
        width: event.width,
        height: event.height,
        devicePixelRatio: event.devicePixelRatio,
        albumMode
      })
      const sourceBlob: Blob = blob

      const converted = await getOrEncodeThumbnail(resizedBlobCache, resizedKey, async () => {
        const img = await createImageBitmap(sourceBlob)
        try {
          return await readAndCompressImage(img, {
            argorithm: 'bilinear',
            quality: THUMBNAIL_QUALITY,
            maxWidth: albumMode
              ? img.width *
                Math.max(event.width / img.width, event.height / img.height) *
                event.devicePixelRatio
              : event.width * event.devicePixelRatio,
            maxHeight: albumMode
              ? img.height *
                Math.max(event.width / img.width, event.height / img.height) *
                event.devicePixelRatio
              : event.height * event.devicePixelRatio
          })
        } finally {
          img.close()
        }
      })

      const objectUrl = URL.createObjectURL(converted)
      postToMainImg.smallImageProcessed({
        assetId: event.assetId,
        cacheKey: resizedKey,
        albumMode,
        url: objectUrl
      })
    } catch (error) {
      if (axios.isCancel(error)) return
      if (axios.isAxiosError(error) && error.response?.status === 401) {
        postToMainImg.tokenExpired({ assetId: event.assetId })
      }
      // Purge potentially corrupt blob from caches so the next attempt re-fetches
      void purgeFromCaches(event.assetId)
      console.error(error)
    }
  },

  async processImage(event: ProcessImagePayload) {
    try {
      const cacheKey = event.assetId

      // Layer 1: In-memory cache
      let blob = blobCache.get(cacheKey)

      // Layer 2: Cache API (disk-persistent)
      if (blob === undefined) {
        blob = await getFromDiskCache(cacheKey)
        if (blob !== undefined) {
          blobCache.set(cacheKey, blob)
        }
      }

      // Layer 3: Network fetch
      if (blob === undefined) {
        const headers: Record<string, string> = {}
        if (event.albumId !== null) headers['x-album-id'] = event.albumId
        if (event.shareId !== null) headers['x-share-id'] = event.shareId
        // eslint-disable-next-line @typescript-eslint/strict-boolean-expressions
        if (event.password) headers['x-share-password'] = event.password
        headers.Authorization = `Bearer ${event.hashToken}`

        const config = {
          responseType: 'blob' as const,
          headers,
          timestampToken: event.timestampToken
        }

        const response = await workerAxios.get<Blob>(
          getSrc(event.hash, false, 'jpg', event.updatedAt),
          config
        )

        blob = response.data
        blobCache.set(cacheKey, blob)
        void putToDiskCache(cacheKey, blob)
      }

      const img = await createImageBitmap(blob)

      const offscreenCanvas = new OffscreenCanvas(img.width, img.height)
      const context = offscreenCanvas.getContext('2d')
      context?.drawImage(img, 0, 0)

      const orientedImgBlob = await offscreenCanvas.convertToBlob()
      const objectUrl = URL.createObjectURL(orientedImgBlob)

      postToMainImg.imageProcessed({ index: event.index, url: objectUrl })
    } catch (error) {
      if (axios.isAxiosError(error) && error.response?.status === 401) {
        postToMainImg.tokenExpired({ assetId: event.assetId })
      }
      // Purge potentially corrupt blob from caches so the next attempt re-fetches
      void purgeFromCaches(event.assetId)
      console.error(error)
    }
  },

  processAbort(event: ProcessAbortPayload) {
    const controller = controllerMap.get(event.index)
    if (controller !== undefined) {
      controller.abort()
      controllerMap.delete(event.index)
    }
  }
})

self.addEventListener('message', (e) => {
  handler(e.data as ReturnType<(typeof toImgWorker)[keyof typeof toImgWorker]>)
})
