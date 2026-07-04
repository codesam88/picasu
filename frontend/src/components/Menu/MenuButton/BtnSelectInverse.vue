<template>
  <v-tooltip location="top" text="Invert">
    <template #activator="{ props: btnProps }">
      <v-btn v-bind="btnProps" icon="mdi-checkbox-intermediate-variant" @click="selectInverse" />
    </template>
  </v-tooltip>
</template>

<script lang="ts" setup>
import { useCollectionStore } from '@/store/collectionStore'
import { usePrefetchStore } from '@/store/prefetchStore'
import { IsolationId } from '@type/types'
const props = defineProps<{
  isolationId: IsolationId
}>()

const collectionStore = useCollectionStore(props.isolationId)
const prefetchStore = usePrefetchStore(props.isolationId)

const selectInverse = () => {
  for (let i = 0; i < prefetchStore.dataLength; i++) {
    if (collectionStore.editModeCollection.has(i)) {
      collectionStore.editModeCollection.delete(i)
    } else {
      collectionStore.editModeCollection.add(i)
    }
  }
}
</script>
