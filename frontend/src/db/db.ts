const DB_NAME = 'assetToken'
const DB_VERSION = 2
const ASSET_STORE_NAME = 'assetToken'
const SHARE_STORE_NAME = 'shareInfo'

// Export constants for Service Worker
export { DB_NAME, DB_VERSION, SHARE_STORE_NAME }

function openAssetDB(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    const request = indexedDB.open(DB_NAME, DB_VERSION)

    request.onupgradeneeded = (event) => {
      const db = (event.target as IDBOpenDBRequest).result
      if (!db.objectStoreNames.contains(ASSET_STORE_NAME)) {
        db.createObjectStore(ASSET_STORE_NAME)
      }
      if (!db.objectStoreNames.contains(SHARE_STORE_NAME)) {
        db.createObjectStore(SHARE_STORE_NAME)
      }
    }

    request.onsuccess = (event) => {
      resolve((event.target as IDBOpenDBRequest).result)
    }

    request.onerror = (event) => {
      const error = (event.target as IDBOpenDBRequest).error
      console.error(
        `Database error: ${error instanceof DOMException ? error.message : String(error)}`
      )
      resolve(null)
    }
  })
}

export async function storeAssetToken(assetId: string, token: string): Promise<void> {
  const db = await openAssetDB()
  if (!db) {
    console.error('Failed to open database for storing asset token')
    return
  }

  return new Promise<void>((resolve) => {
    const transaction = db.transaction(ASSET_STORE_NAME, 'readwrite')
    const store = transaction.objectStore(ASSET_STORE_NAME)
    const request = store.put(token, assetId)

    request.onsuccess = () => {
      resolve()
    }

    request.onerror = () => {
      console.error('Error storing asset token')
      resolve()
    }
  })
}

export async function getAssetToken(assetId: string): Promise<string | null> {
  const db = await openAssetDB()
  if (!db) {
    console.error('Failed to open database for retrieving asset token')
    return null
  }

  return new Promise<string | null>((resolve) => {
    const transaction = db.transaction(ASSET_STORE_NAME, 'readonly')
    const store = transaction.objectStore(ASSET_STORE_NAME)
    const request = store.get(assetId)

    request.onsuccess = () => {
      const rawResult: unknown = request.result
      if (typeof rawResult === 'string') {
        resolve(rawResult)
      } else {
        resolve(null)
      }
    }

    request.onerror = () => {
      console.error('Error retrieving asset token')
      resolve(null)
    }
  })
}

export async function deleteAssetToken(assetId: string): Promise<void> {
  const db = await openAssetDB()
  if (!db) {
    console.error('Failed to open database for deleting asset token')
    return
  }

  return new Promise<void>((resolve) => {
    const transaction = db.transaction(ASSET_STORE_NAME, 'readwrite')
    const store = transaction.objectStore(ASSET_STORE_NAME)
    const request = store.delete(assetId)

    request.onsuccess = () => {
      resolve()
    }

    request.onerror = () => {
      console.error('Error deleting asset token')
      resolve()
    }
  })
}

// Share info storage for Service Worker
export interface ShareInfo {
  albumId: string | null
  shareId: string | null
  password: string | null
}

// Use composite key: albumId_shareId to support multiple shares
function getShareKey(albumId: string, shareId: string): string {
  return `${albumId}_${shareId}`
}

export async function storeShareInfo(info: ShareInfo): Promise<void> {
  // eslint-disable-next-line @typescript-eslint/strict-boolean-expressions
  if (!info.albumId || !info.shareId) {
    console.error('Cannot store share info without albumId and shareId')
    return
  }

  const db = await openAssetDB()
  if (!db) {
    console.error('Failed to open database for storing share info')
    return
  }

  const key = getShareKey(info.albumId, info.shareId)

  return new Promise<void>((resolve) => {
    const transaction = db.transaction(SHARE_STORE_NAME, 'readwrite')
    const store = transaction.objectStore(SHARE_STORE_NAME)
    const request = store.put(info, key)

    request.onsuccess = () => {
      resolve()
    }

    request.onerror = () => {
      console.error('Error storing share info')
      resolve()
    }
  })
}

export async function getShareInfo(albumId: string, shareId: string): Promise<ShareInfo | null> {
  const db = await openAssetDB()
  if (!db) {
    console.error('Failed to open database for retrieving share info')
    return null
  }

  const key = getShareKey(albumId, shareId)

  return new Promise<ShareInfo | null>((resolve) => {
    const transaction = db.transaction(SHARE_STORE_NAME, 'readonly')
    const store = transaction.objectStore(SHARE_STORE_NAME)
    const request = store.get(key)

    request.onsuccess = () => {
      const result = request.result as ShareInfo | undefined
      resolve(result ?? null)
    }

    request.onerror = () => {
      console.error('Error retrieving share info')
      resolve(null)
    }
  })
}

export async function clearShareInfo(albumId: string, shareId: string): Promise<void> {
  const db = await openAssetDB()
  if (!db) {
    console.error('Failed to open database for clearing share info')
    return
  }

  const key = getShareKey(albumId, shareId)

  return new Promise<void>((resolve) => {
    const transaction = db.transaction(SHARE_STORE_NAME, 'readwrite')
    const store = transaction.objectStore(SHARE_STORE_NAME)
    const request = store.delete(key)

    request.onsuccess = () => {
      resolve()
    }

    request.onerror = () => {
      console.error('Error clearing share info')
      resolve()
    }
  })
}
