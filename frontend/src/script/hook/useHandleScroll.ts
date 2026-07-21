import { Ref } from 'vue'
import { usePrefetchStore } from '@/store/prefetchStore'
import { useLocationStore } from '@/store/locationStore'
import { useScrollTopStore } from '@/store/scrollTopStore'
import { IsolationId } from '@type/types'

export function handleScroll(
  imageContainerRef: Ref<HTMLElement | null>,
  _lastScrollTop: Ref<number>,
  _stopScroll: Ref<boolean>,
  _windowHeight: Ref<number>,
  isolationId: IsolationId
) {
  const prefetchStore = usePrefetchStore(isolationId)
  const locationStore = useLocationStore(isolationId)
  const scrollTopStore = useScrollTopStore(isolationId)

  const throttledHandleScroll = () => {
    const el = imageContainerRef.value
    if (!el) return

    const scrollTop = el.scrollTop
    const scrollHeight = el.scrollHeight
    const clientHeight = el.clientHeight

    scrollTopStore.scrollTop = scrollTop

    const percentage = scrollTop / (scrollHeight - clientHeight)
    const imageIndex = Math.floor(percentage * prefetchStore.dataLength)
    locationStore.locationIndex = Math.min(imageIndex, prefetchStore.dataLength - 1)
  }

  return { throttledHandleScroll }
}
