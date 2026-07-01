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
          <v-textarea
            v-model="descriptionModel"
            label="Description"
            variant="outlined"
            density="comfortable"
            rows="3"
            auto-grow
          />
          <v-combobox
            v-model="tagsModel"
            chips
            multiple
            closable-chips
            label="Keywords"
            variant="outlined"
            density="comfortable"
            autocomplete="off"
            :items="tagStore.tags.map((t) => t.tag)"
          />
          <v-text-field
            v-model="customDateModel"
            label="Date"
            placeholder="e.g. 2024-06-01"
            variant="outlined"
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
 * Views and edits custom metadata (title, description, keywords, date) for a
 * single selected album-type grid item. Reachable from the batch context
 * menu's "Album Info" action (ItemAlbumInfo.vue), enabled only when exactly
 * one album is selected. All fields are optional and persisted to disk via
 * the album's .albuminfo.xmp sidecar (dir-albums) or DB only (manual
 * albums).
 */
import { ref, computed, watch } from 'vue'
import { useModalStore } from '@/store/modalStore'
import { useCollectionStore } from '@/store/collectionStore'
import { useDataStore } from '@/store/dataStore'
import { useTagStore } from '@/store/tagStore'
import { editTitle, editCustomDate } from '@utils/createAlbums'
import { editUserDefinedDescription } from '@utils/editDescription'
import { editTags } from '@/api/editTags'
import { EnrichedUnifiedData } from '@type/types'

type EnrichedAlbum = Extract<EnrichedUnifiedData, { type: 'album' }>

const modalStore = useModalStore('mainId')
const collectionStore = useCollectionStore('mainId')
const dataStore = useDataStore('mainId')
const tagStore = useTagStore('mainId')

const titleModel = ref('')
const descriptionModel = ref('')
const tagsModel = ref<string[]>([])
const customDateModel = ref('')
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
    descriptionModel.value = a.description ?? ''
    tagsModel.value = [...a.tags]
    customDateModel.value = a.customDate ?? ''
  },
  { immediate: true }
)

const save = async () => {
  const a = album.value
  const index = selectedIndex.value
  if (a === undefined || index === undefined) return

  saving.value = true
  try {
    const addTags = tagsModel.value.filter((t) => !a.tags.includes(t))
    const removeTags = a.tags.filter((t) => !tagsModel.value.includes(t))

    // Each of these reads-modifies-writes the same album record (and its
    // .albuminfo.xmp sidecar) independently on the backend, so they must run
    // sequentially — firing them concurrently races and can drop edits.
    await editTitle(a, titleModel.value)
    await editUserDefinedDescription(a, descriptionModel.value, index, 'mainId')
    await editCustomDate(a, customDateModel.value)
    if (addTags.length > 0 || removeTags.length > 0) {
      await editTags([index], addTags, removeTags, 'mainId')
    }
  } finally {
    saving.value = false
    modalStore.showAlbumInfoModal = false
  }
}
</script>
