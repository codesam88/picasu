<template>
  <v-dialog
    v-if="album !== undefined"
    v-model="modalStore.showAlbumInfoModal"
    persistent
    id="album-info-modal"
    max-width="480"
  >
    <v-card variant="elevated">
      <template #title>Album Info</template>
      <template #text>
        <v-form @submit.prevent="save">
          <v-text-field
            v-model="titleModel"
            label="Title"
            variant="outlined"
            :placeholder="'Untitled'"
            density="comfortable"
          />
        </v-form>
      </template>
      <v-divider />
      <template #actions>
        <v-spacer />
        <v-btn @click="modalStore.showAlbumInfoModal = false">Cancel</v-btn>
        <v-btn color="primary" :loading="saving" @click="save">Save</v-btn>
      </template>
    </v-card>
  </v-dialog>
</template>

<script setup lang="ts">
/**
 * Views and edits the display title of a single selected album-type grid
 * item. Reachable from the batch context menu's "Album Info" action
 * (ItemAlbumInfo.vue), enabled only when exactly one album is selected.
 * Description/tags for albums are edited via the existing per-item metadata
 * panel and EditTagsModal, not duplicated here.
 */
import { ref, computed, watch } from 'vue'
import { useModalStore } from '@/store/modalStore'
import { useCollectionStore } from '@/store/collectionStore'
import { useDataStore } from '@/store/dataStore'
import { editTitle } from '@utils/createAlbums'
import { EnrichedUnifiedData } from '@type/types'

type EnrichedAlbum = Extract<EnrichedUnifiedData, { type: 'album' }>

const modalStore = useModalStore('mainId')
const collectionStore = useCollectionStore('mainId')
const dataStore = useDataStore('mainId')

const titleModel = ref('')
const saving = ref(false)

const selectedIndex = computed<number | undefined>(() => {
  if (collectionStore.editModeCollection.size !== 1) return undefined
  return Array.from(collectionStore.editModeCollection)[0]
})

const album = computed<EnrichedAlbum | undefined>(() => {
  if (selectedIndex.value === undefined) return undefined
  const data = dataStore.data.get(selectedIndex.value)
  return data?.type === 'album' ? data : undefined
})

// Any change in grid selection force-closes the dialog — it can never carry
// stale open/visible state across from a previous album into a newly
// selected one, which previously let it "pop open" without an explicit
// click on "Album Info" after the selection changed.
watch(selectedIndex, () => {
  modalStore.showAlbumInfoModal = false
})

watch(
  album,
  (a) => {
    if (a === undefined) return
    titleModel.value = a.title ?? ''
  },
  { immediate: true }
)

const save = async () => {
  const a = album.value
  if (a === undefined) return

  saving.value = true
  try {
    await editTitle(a, titleModel.value)
    modalStore.showAlbumInfoModal = false
  } finally {
    saving.value = false
  }
}
</script>
