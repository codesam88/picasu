<template>
  <v-dialog
    v-if="submit !== undefined"
    v-model="modalStore.showEditTagsModal"
    persistent
    id="edit-tag-overlay"
    max-width="400"
  >
    <v-confirm-edit
      v-model="changedTagsArray"
      :disabled="false"
      @save="submit"
      @cancel="modalStore.showEditTagsModal = false"
    >
      <template #default="{ model: proxyModel, actions }">
        <v-card variant="elevated" retain-focus>
          <template #title> Edit Tags </template>
          <template #text>
            <v-form v-model="formIsValid" @submit.prevent validate-on="input">
              <v-combobox
                v-model="proxyModel.value"
                chips
                multiple
                return-object
                item-title="title"
                item-value="value"
                :items="allItems"
                label="Tags"
                closable-chips
                variant="outlined"
                autocomplete="off"
              >
                <template #chip="{ props: chipProps, internalItem }">
                  <v-chip v-bind="chipProps">{{ internalItem.title }}</v-chip>
                </template>
                <template #item="{ props: itemProps }">
                  <v-list-item v-bind="itemProps">
                    <template #prepend="{ isActive }">
                      <v-list-item-action>
                        <v-checkbox-btn :model-value="isActive" />
                      </v-list-item-action>
                    </template>
                  </v-list-item>
                </template>
              </v-combobox>
            </v-form>
          </template>
          <v-divider />
          <template #actions>
            <v-spacer />
            <component :is="actions" />
          </template>
        </v-card>
      </template>
    </v-confirm-edit>
  </v-dialog>
</template>

<script setup lang="ts">
/**
 * This modal is used for editing the tags of a single photo on the single photo view page.
 *
 * The combobox runs in Vuetify's `return-object` mode, so its model is a mixed array of
 * plain strings (user-typed free text) and ComboboxItem objects (tags picked from the
 * dropdown). `getTagString` normalizes both to the tag string before saving.
 */
import { ref, computed, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useModalStore } from '@/store/modalStore'
import { useTagStore } from '@/store/tagStore'
import { useDataStore } from '@/store/dataStore'
import { getAssetIndexDataFromRoute, getIsolationIdByRoute } from '@utils/getter'
import { editTags } from '@/api/editTags'
import { fetchAssetMetadata } from '@/api/fetchMetadata'

// Combobox item shape used for tags picked from the dropdown.
interface ComboboxItem {
  title: string
  value: string
}

// With `return-object`, the combobox model contains ComboboxItem objects for items
// selected from the dropdown, and plain strings for user-typed free-text tags.
type ModelValue = string | ComboboxItem

// Extract the plain tag string from a model value.
// For user-typed strings this is the string itself; for ComboboxItem objects it's `.value`.
function getTagString(v: ModelValue): string {
  return typeof v === 'string' ? v : v.value
}

const formIsValid = ref(false)
const changedTagsArray = ref<ModelValue[]>([])
const submit = ref<(() => Promise<void>) | undefined>(undefined)

const route = useRoute()
const modalStore = useModalStore('mainId')
const tagStore = useTagStore('mainId')

const allItems = computed<ComboboxItem[]>(() =>
  tagStore.tags.map((t) => ({ title: t.tag, value: t.tag }))
)

onMounted(async () => {
  // List rows are lean (Phase 14): fetch the detail record before seeding so
  // the combobox prefills the item's actual tags. The modal can be opened
  // without the info panel, so it triggers its own detail fetch.
  const routeInit = getAssetIndexDataFromRoute(route)
  if (routeInit !== undefined && routeInit.data.type !== 'album') {
    const isolationId = getIsolationIdByRoute(route)
    const detail = await fetchAssetMetadata(routeInit.assetId, isolationId)
    if (detail !== null) {
      useDataStore(isolationId).mergeMetadata(routeInit.index, detail)
    }
  }

  const useSubmit = (): undefined | (() => Promise<void>) => {
    const initializeResult = getAssetIndexDataFromRoute(route)
    if (initializeResult === undefined) {
      console.error(
        "useSubmit Error: Failed to initialize result. 'getAssetIndexDataFromRoute(route)' returned undefined."
      )
      return undefined
    }
    const { index, data } = initializeResult
    let defaultTags: string[]

    if (data.type === 'image' || data.type === 'video') {
      defaultTags = data.tags
      // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
    } else if (data.type === 'album') {
      defaultTags = data.tags
    } else {
      console.error("useSubmit Error: 'data' type is not recognized.")
      return undefined
    }

    // Seed the model with the item's current tags.
    changedTagsArray.value = [...defaultTags]

    const innerSubmit = async () => {
      const currentTags = changedTagsArray.value.map(getTagString)

      const hashArray: number[] = [index]
      const addTagsArray = currentTags.filter((tag) => !defaultTags.includes(tag))
      const removeTagsArray = defaultTags.filter((tag) => !currentTags.includes(tag))

      const isolationId = getIsolationIdByRoute(route)

      modalStore.showEditTagsModal = false

      if (addTagsArray.length > 0 || removeTagsArray.length > 0) {
        await editTags(hashArray, addTagsArray, removeTagsArray, isolationId)
      }
    }
    return innerSubmit
  }
  submit.value = useSubmit()
})
</script>
