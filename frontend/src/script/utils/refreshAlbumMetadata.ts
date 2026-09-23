import { fetchDataInWorker } from '@/api/fetchData'
import { useDataStore } from '@/store/dataStore'
import { useMessageStore } from '@/store/messageStore'
import { useWorkerStore } from '@/store/workerStore'
import { bindActionDispatch } from 'typesafe-agent-events'
import { toImgWorker } from '@/worker/workerApi'
import { watch } from 'vue'
import { useShareStore } from '@/store/shareStore'
import { useTokenStore } from '@/store/tokenStore'
export async function refreshAlbumMetadata(albumId: string) {
  const dataStore = useDataStore('mainId')
  const workerStore = useWorkerStore('mainId')
  const messageStore = useMessageStore('mainId')
  const shareStore = useShareStore('mainId')
  const tokenStore = useTokenStore('mainId')

  const albumIndex = dataStore.assetIdMapData.get(albumId)
  if (albumIndex === undefined) {
    console.error(`cannot find albumIndex with albumId = ${albumId}`)
    return
  }

  // perform after fetchDataInWorker
  const stopWatch = watch(
    () => dataStore.data.get(albumIndex),
    async () => {
      const postToWorker = bindActionDispatch(toImgWorker, (action) => {
        const worker = workerStore.imgWorker[0]
        if (worker) {
          worker.postMessage(action)
        } else {
          throw new Error(`Worker not found for index: 0`)
        }
      })

      const data = dataStore.data.get(albumIndex)
      if (data?.type !== 'album') {
        console.error(`cannot find album with albumIndex = ${albumIndex}`)
        return
      }

      const coverAssetId = data.cover
      if (coverAssetId === null) return

      await tokenStore.refreshTimestampTokenIfExpired()
      await tokenStore.refreshAssetTokenIfExpired(coverAssetId)

      const timestampToken = tokenStore.timestampToken
      const assetToken = tokenStore.assetTokenMap.get(coverAssetId)

      if (timestampToken === null) {
        console.error('timestampToken is null after refresh')
        return
      }

      if (assetToken === undefined) {
        console.error('assetToken is undefined after refresh')
        return
      }

      postToWorker.processImage({
        index: albumIndex,
        hash: coverAssetId,
        assetId: coverAssetId,
        devicePixelRatio: window.devicePixelRatio,
        albumId: shareStore.albumId,
        shareId: shareStore.shareId,
        password: shareStore.password,
        timestampToken,
        hashToken: assetToken,
        updatedAt: data.updateAt
      })

      postToWorker.processSmallImage({
        index: albumIndex,
        hash: coverAssetId,
        assetId: coverAssetId,
        width: 300,
        height: 300,
        devicePixelRatio: window.devicePixelRatio,
        albumMode: true,
        albumId: shareStore.albumId,
        shareId: shareStore.shareId,
        password: shareStore.password,
        timestampToken,
        hashToken: assetToken,
        updatedAt: data.updateAt
      })

      messageStore.success(`Album cover updated successfully`)
      stopWatch()
    }
  )

  await fetchDataInWorker('single', albumIndex, 'mainId')
}
