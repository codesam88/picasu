<template>
  <BaseModal
    :id="id"
    v-model="modelValue"
    :title="title"
    :width="width"
    :loading="loading"
    :hide-close="hideClose"
  >
    <slot>
      <p v-if="message !== ''" class="mb-0">{{ message }}</p>
    </slot>

    <template #actions>
      <v-btn variant="text" data-testid="confirm-cancel" @click="handleCancel">
        {{ cancelLabel }}
      </v-btn>
      <v-btn
        :color="destructive ? 'error' : 'primary'"
        variant="flat"
        data-testid="confirm-ok"
        :loading="loading"
        @click="handleConfirm"
      >
        {{ confirmLabel }}
      </v-btn>
    </template>
  </BaseModal>
</template>

<script setup lang="ts">
import BaseModal from '@/components/Modal/BaseModal.vue'

withDefaults(
  defineProps<{
    id: string
    title?: string
    message?: string
    width?: string | number
    confirmLabel?: string
    cancelLabel?: string
    loading?: boolean
    destructive?: boolean
    hideClose?: boolean
  }>(),
  {
    title: '',
    message: '',
    width: 480,
    confirmLabel: 'Confirm',
    cancelLabel: 'Cancel',
    loading: false,
    destructive: false,
    hideClose: false
  }
)

const emit = defineEmits<{
  confirm: []
  cancel: []
}>()

defineSlots<{
  default?: () => unknown
}>()

const modelValue = defineModel<boolean>({ required: true })

const handleConfirm = () => {
  emit('confirm')
}

const handleCancel = () => {
  emit('cancel')
}
</script>
