<template>
  <v-dialog
    v-if="submit !== undefined"
    v-model="modalStore.showBatchEditTagsModal"
    persistent
    id="batch-edit-tag-overlay"
    max-width="400"
  >
    <v-confirm-edit
      v-model="changedTags"
      :disabled="false"
      @save="submit"
      @cancel="modalStore.showBatchEditTagsModal = false"
    >
      <template #default="{ model: proxyModel, actions }">
        <v-card variant="elevated" retain-focus>
          <template #title>Edit&nbsp;Tags</template>

          <template #text>
            <v-form
              ref="formRef"
              v-model="formIsValid"
              @submit.prevent="submit"
              validate-on="input"
            >
              <v-container>
                <v-combobox
                  v-model="proxyModel.value.add"
                  chips
                  multiple
                  return-object
                  item-title="title"
                  item-value="value"
                  label="Add Tags"
                  :items="allItems"
                  :rules="[addTagsRule]"
                  closable-chips
                  :menu-props="{ maxWidth: 0 }"
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
              </v-container>

              <v-container>
                <v-combobox
                  v-model="proxyModel.value.remove"
                  chips
                  multiple
                  return-object
                  item-title="title"
                  item-value="value"
                  label="Remove Tags"
                  :items="allItems"
                  :rules="[removeTagsRule]"
                  closable-chips
                  :menu-props="{ maxWidth: 0 }"
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
              </v-container>
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
 * Batch edit modal for adding/removing tags across multiple selected items.
 *
 * Both comboboxes run in Vuetify's `return-object` mode, so their models are mixed
 * arrays of plain strings (user-typed free text) and ComboboxItem objects (tags picked
 * from the dropdown). `getTagString` normalizes both. Validation rules prevent the same
 * tag from appearing in Add and Remove.
 */
import { ref, computed, watch, onMounted } from 'vue'
import { useRoute } from 'vue-router'
import { useModalStore } from '@/store/modalStore'
import { useCollectionStore } from '@/store/collectionStore'
import { useTagStore } from '@/store/tagStore'
import { getIsolationIdByRoute } from '@utils/getter'
import { refreshGalleryAfterMutation } from '@/script/hook/usePrefetch'
import type { VForm } from 'vuetify/components'
import { editTags } from '@/api/editTags'

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

interface ChangedTags {
  add: ModelValue[]
  remove: ModelValue[]
}

const formRef = ref<VForm | null>(null)
const formIsValid = ref(false)
const changedTags = ref<ChangedTags>({ add: [], remove: [] })

const route = useRoute()
const isolationId = getIsolationIdByRoute(route)

const modalStore = useModalStore('mainId')
const collectionStore = useCollectionStore(isolationId)
const tagStore = useTagStore('mainId')

const allItems = computed<ComboboxItem[]>(() =>
  tagStore.tags.map((t) => ({ title: t.tag, value: t.tag }))
)

// Validation: prevent the same tag from appearing in both Add and Remove.
const addTagsRule = (arr: ModelValue[]) => {
  const removeKeys = new Set(changedTags.value.remove.map(getTagString))
  return (
    arr.every((t) => !removeKeys.has(getTagString(t))) ||
    'Some items are already selected in Remove Tags'
  )
}

const removeTagsRule = (arr: ModelValue[]) => {
  const addKeys = new Set(changedTags.value.add.map(getTagString))
  return (
    arr.every((t) => !addKeys.has(getTagString(t))) || 'Some items are already selected in Add Tags'
  )
}

const submit = ref<() => Promise<void> | undefined>()

onMounted(() => {
  submit.value = async () => {
    const hashes = Array.from(collectionStore.editModeCollection)
    const addTagsArray = changedTags.value.add.map(getTagString)
    const removeTagsArray = changedTags.value.remove.map(getTagString)

    modalStore.showBatchEditTagsModal = false

    if (addTagsArray.length > 0 || removeTagsArray.length > 0) {
      await editTags(hashes, addTagsArray, removeTagsArray, isolationId)
    }

    await refreshGalleryAfterMutation(isolationId, route)
  }
})

watch(
  () => [changedTags.value.add, changedTags.value.remove],
  async () => {
    await formRef.value?.validate()
  },
  { deep: true }
)
</script>
