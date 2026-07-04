<template>
  <v-tooltip location="top" text="Clear">
    <template #activator="{ props: btnProps }">
      <v-btn v-bind="btnProps" icon="mdi-checkbox-blank-outline" @click="selectClear" />
    </template>
  </v-tooltip>
</template>

<script lang="ts" setup>
import { useCollectionStore } from '@/store/collectionStore'
import { IsolationId } from '@type/types'

const props = defineProps<{
  isolationId: IsolationId
}>()

const collectionStore = useCollectionStore(props.isolationId)

const selectClear = () => {
  collectionStore.editModeCollection.clear()
}
</script>
