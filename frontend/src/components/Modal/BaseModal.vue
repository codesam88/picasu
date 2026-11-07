<template>
  <v-dialog
    :id="id"
    v-model="internalValue"
    :max-width="width"
    variant="flat"
    persistent
    theme="dark"
    scrollable
    :fullscreen="fullscreen"
    :transition="transition"
  >
    <v-card rounded="lg" class="d-flex flex-column" elevation="12">
      <slot name="header">
        <v-toolbar color="transparent" density="compact">
          <v-toolbar-title>
            {{ title }}
          </v-toolbar-title>

          <template #append>
            <v-tooltip v-if="!hideClose" location="top" text="Close">
              <template #activator="{ props: btnProps }">
                <v-btn
                  v-bind="btnProps"
                  data-testid="modal-close"
                  icon="mdi-close"
                  variant="text"
                  density="comfortable"
                  :disabled="loading"
                  @click="internalValue = false"
                />
              </template>
            </v-tooltip>
          </template>
        </v-toolbar>
      </slot>

      <v-progress-linear
        v-if="loading"
        indeterminate
        color="primary"
        height="2"
      ></v-progress-linear>
      <v-divider v-else class="border-opacity-25"></v-divider>

      <v-card-text :class="['custom-scrollbar', contentClass]">
        <slot></slot>
      </v-card-text>

      <template v-if="$slots.actions">
        <v-card-actions>
          <slot name="actions"></slot>
        </v-card-actions>
      </template>
    </v-card>
  </v-dialog>
</template>

<script setup lang="ts">
import { computed, onMounted, onUnmounted } from 'vue'

const props = defineProps({
  modelValue: { type: Boolean, required: true },
  title: { type: String, default: '' },
  width: { type: [String, Number], default: 450 },
  /** External control of inner padding. Default: 'pa-4' (Standard) */
  contentClass: { type: String, default: 'pa-4' },
  loading: { type: Boolean, default: false },
  hideClose: { type: Boolean, default: false },
  fullscreen: { type: Boolean, default: false },
  transition: { type: String, default: 'dialog-transition' },
  id: { type: String, default: undefined }
})

const emit = defineEmits(['update:modelValue'])

defineSlots<{
  header?: () => unknown
  default?: () => unknown
  actions?: () => unknown
}>()

const internalValue = computed({
  get: () => props.modelValue,
  set: (val) => {
    emit('update:modelValue', val)
  }
})

// An open dialog owns Escape: close it unless a close is disallowed, and stop
// the event from reaching the page-level handlers in PageTemplate/ViewPage so
// they do not also exit edit mode or navigate back. Listening in the capture
// phase is what makes stopPropagation effective against those window-level
// bubble listeners. While loading or with `hideClose`, the dialog stays open
// but still swallows Escape, otherwise those same page handlers would close
// the store-tracked dialog behind the guard.
const handleKeydown = (event: KeyboardEvent) => {
  if (event.key !== 'Escape' || !props.modelValue) return

  event.stopPropagation()

  if (props.loading || props.hideClose) return

  internalValue.value = false
}

onMounted(() => {
  window.addEventListener('keydown', handleKeydown, { capture: true })
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeydown, { capture: true })
})
</script>

<style scoped>
.custom-scrollbar::-webkit-scrollbar {
  width: 4px;
}
.custom-scrollbar::-webkit-scrollbar-track {
  background: transparent;
}
.custom-scrollbar::-webkit-scrollbar-thumb {
  background-color: rgba(255, 255, 255, 0.2);
  border-radius: 4px;
}
</style>
