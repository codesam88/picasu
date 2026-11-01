import { computed, type ComputedRef } from 'vue'
import { useConfigStore } from '@/store/configStore'
import { useCurrentUser } from './currentUser'

export function usePersonalAlbumsEnabled(): ComputedRef<boolean> {
  const configStore = useConfigStore('mainId')
  const currentUser = useCurrentUser()
  return computed(() => {
    if (currentUser.value === null) return false
    return configStore.config?.personalAlbumsEnabled === true
  })
}
