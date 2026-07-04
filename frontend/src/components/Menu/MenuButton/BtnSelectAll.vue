<template>
  <v-tooltip location="top" text="All">
    <template #activator="{ props: btnProps }">
      <v-btn v-bind="btnProps" icon="mdi-checkbox-intermediate" @click="selectAll" />
    </template>
  </v-tooltip>
</template>

<script lang="ts" setup>
import { IsolationId } from '@type/types'
import { useCollectionStore } from '@/store/collectionStore'
import { usePrefetchStore } from '@/store/prefetchStore'

const props = defineProps<{
  isolationId: IsolationId
}>()

const collectionStore = useCollectionStore(props.isolationId)
const prefetchStore = usePrefetchStore(props.isolationId)

const selectAll = () => {
  for (let i = 0; i < prefetchStore.dataLength; i++) {
    collectionStore.editModeCollection.add(i)
  }
}
</script>
