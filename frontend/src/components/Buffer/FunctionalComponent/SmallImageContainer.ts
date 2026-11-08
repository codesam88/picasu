import { Fragment, FunctionalComponent, h, PropType } from 'vue'
import DesktopSmallImage from './DesktopSmallImage'
import MobileSmallImage from './MobileSmallImage'
import { EnrichedUnifiedData, DisplayElement, IsolationId } from '@type/types'
import { useImgStore } from '@/store/imgStore'
import { useQueueStore } from '@/store/queueStore'
import { useWorkerStore } from '@/store/workerStore'
import { coverServingIds, getArrayValue } from '@utils/getter'
import { useShareStore } from '@/store/shareStore'
import { useTokenStore } from '@/store/tokenStore'
import { useConstStore } from '@/store/constStore'
import { thumbnailCacheKey } from '@/worker/imgCache'

interface ServingIds {
  hash: string
  assetId: string
  albumMode: boolean
}

/**
 * Resolves the asset a tile stands for. Album tiles serve their cover's
 * content; every other type is its own asset.
 */
function servingIds(abstractData: EnrichedUnifiedData): ServingIds | null {
  if (abstractData.type === 'album') {
    const serving = coverServingIds(abstractData.cover, abstractData.coverHash)
    if (serving === null) return null
    return { ...serving, albumMode: true }
  }
  return { hash: abstractData.id, assetId: abstractData.assetId, albumMode: false }
}

interface SmallImageContainerProps {
  abstractData: EnrichedUnifiedData
  index: number
  displayElement: DisplayElement
  isolationId: IsolationId
  mobile: boolean
  onPointerdown: (event: PointerEvent) => void
  onPointerup: (event: PointerEvent) => void
  onPointerleave: () => void
  onClick: (event: MouseEvent) => void
}

const SmallImageContainer: FunctionalComponent<SmallImageContainerProps> = (props) => {
  const imgStore = useImgStore(props.isolationId)
  const queueStore = useQueueStore(props.isolationId)

  const serving = servingIds(props.abstractData)
  if (serving === null) return null

  const cacheKey = thumbnailCacheKey({
    assetId: serving.assetId,
    width: props.displayElement.displayWidth,
    height: props.displayElement.displayHeight,
    devicePixelRatio: window.devicePixelRatio,
    albumMode: serving.albumMode
  })
  // Reuse the URL only when it was encoded for the same asset and output box;
  // a settings/resize change makes the key miss and re-queues the tile.
  const src = imgStore.getImgUrl(serving.assetId, serving.albumMode, cacheKey)

  if (src === undefined) {
    if (!queueStore.img.has(props.index)) {
      queueStore.img.add(props.index)
      checkAndFetch(
        props.abstractData,
        props.index,
        serving,
        props.displayElement.displayWidth,
        props.displayElement.displayHeight,
        props.isolationId
      ).catch((err: unknown) => {
        console.error('checkAndFetch failed:', err)
      })
    }
    return null
  }

  const chips = []

  const hasBorder = props.abstractData.type === 'album'

  if (props.mobile) {
    chips.push(
      h(MobileSmallImage, {
        hasBorder: hasBorder,
        src: src,
        onPointerdown: props.onPointerdown,
        onPointerup: props.onPointerup,
        onPointerleave: props.onPointerleave
      })
    )
  } else {
    chips.push(
      h(DesktopSmallImage, {
        hasBorder: hasBorder,
        src: src,
        onClick: props.onClick
      })
    )
  }

  return h(Fragment, null, chips)
}

SmallImageContainer.props = {
  abstractData: {
    type: Object as PropType<EnrichedUnifiedData>,
    required: true
  },
  displayElement: {
    type: Object as PropType<DisplayElement>,
    required: true
  },
  isolationId: {
    type: String as PropType<IsolationId>,
    required: true
  },
  index: {
    type: Number,
    required: true
  },
  mobile: {
    type: Boolean,
    required: true
  },
  onPointerdown: {
    type: Function as PropType<(event: PointerEvent) => void>,
    required: true
  },
  onPointerup: {
    type: Function as PropType<(event: PointerEvent) => void>,
    required: true
  },
  onPointerleave: {
    type: Function as PropType<() => void>,
    required: true
  },
  onClick: {
    type: Function as PropType<(event: MouseEvent) => void>,
    required: true
  }
}
async function checkAndFetch(
  abstractData: EnrichedUnifiedData,
  index: number,
  serving: ServingIds,
  displayWidth: number,
  displayHeight: number,
  isolationId: IsolationId
) {
  const workerStore = useWorkerStore(isolationId)
  const tokenStore = useTokenStore(isolationId)
  const shareStore = useShareStore('mainId')
  const constStore = useConstStore('mainId')
  const workerIndex = index % constStore.concurrencyNumber

  await tokenStore.refreshTimestampTokenIfExpired()
  const timestampToken = tokenStore.timestampToken
  if (timestampToken === null) {
    throw new Error('timestampToken is null after refresh')
  }

  if (workerStore.postToImgWorkerList === undefined) {
    console.error('workerStore.postToImgWorkerList is undefined')
    return
  }

  await tokenStore.refreshAssetTokenIfExpired(serving.assetId)

  const assetToken = tokenStore.assetTokenMap.get(serving.assetId)
  if (assetToken === undefined) {
    throw new Error(`assetToken is undefined after refresh for assetId: ${serving.assetId}`)
  }

  getArrayValue(workerStore.postToImgWorkerList, workerIndex).processSmallImage({
    index,
    hash: serving.hash,
    assetId: serving.assetId,
    width: displayWidth,
    height: displayHeight,
    devicePixelRatio: window.devicePixelRatio,
    albumMode: serving.albumMode,
    albumId: shareStore.albumId,
    shareId: shareStore.shareId,
    password: shareStore.password,
    timestampToken,
    hashToken: assetToken,
    updatedAt: abstractData.updateAt
  })
}

export default SmallImageContainer
