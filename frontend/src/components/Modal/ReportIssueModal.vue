<template>
  <BaseModal
    id="report-issue-modal"
    v-model="show"
    title="Report issue..."
    :width="560"
    :loading="submitting"
  >
    <p class="text-caption mb-3">
      Reports are sent to the project's Web3Forms inbox (a third-party service).
    </p>

    <v-textarea
      v-model="message"
      label="Message"
      required
      maxlength="2000"
      counter
      autofocus
      :error-messages="messageError"
      class="mb-2"
    ></v-textarea>

    <v-checkbox
      v-model="includeDiagnostics"
      label="Include diagnostics"
      hide-details
      class="mb-2"
    ></v-checkbox>

    <details class="mb-3">
      <summary>What will be sent</summary>
      <pre data-testid="report-issue-preview" class="text-body-2 pa-2">{{ previewText }}</pre>
    </details>

    <input
      id="botcheck"
      v-model="botcheck"
      type="text"
      name="botcheck"
      tabindex="-1"
      autocomplete="off"
      aria-hidden="true"
      style="position: absolute; left: -9999px; opacity: 0; pointer-events: none"
    />

    <template #actions>
      <v-spacer />
      <v-btn data-testid="report-issue-cancel" variant="text" :disabled="submitting" @click="close">
        Cancel
      </v-btn>
      <v-btn
        data-testid="report-issue-send"
        color="primary"
        variant="elevated"
        :disabled="sendDisabled"
        :loading="submitting"
        @click="send"
      >
        Send
      </v-btn>
    </template>
  </BaseModal>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'
import { useRoute } from 'vue-router'
import { useModalStore } from '@/store/modalStore'
import { useMessageStore } from '@/store/messageStore'
import BaseModal from '@/components/Modal/BaseModal.vue'
import { submitReportIssue, canSubmitReport, buildReportMessage } from '@/api/reportIssue'

const modalStore = useModalStore('mainId')
const messageStore = useMessageStore('mainId')
const route = useRoute()

const show = computed({
  get: () => modalStore.showReportIssueModal,
  set: (val: boolean) => {
    modalStore.showReportIssueModal = val
  }
})

const message = ref('')
const includeDiagnostics = ref(true)
const botcheck = ref('')
const submitting = ref(false)

const messageError = computed(() => {
  const trimmed = message.value.trim()
  if (trimmed === '') {
    return 'Message is required'
  }
  if (trimmed.length < 10) {
    return 'Message must be at least 10 characters'
  }
  return ''
})

const isValid = computed(() => messageError.value === '')

const sendDisabled = computed(() => submitting.value || !isValid.value)

const previewText = computed(() =>
  buildReportMessage(message.value, route.fullPath, includeDiagnostics.value)
)

const close = (): void => {
  if (submitting.value) {
    return
  }
  modalStore.showReportIssueModal = false
}

const send = async (): Promise<void> => {
  if (submitting.value) {
    return
  }
  if (!isValid.value) {
    return
  }

  if (!canSubmitReport()) {
    messageStore.error('please wait before sending another report')
    return
  }

  submitting.value = true
  try {
    await submitReportIssue(message.value, {
      includeDiagnostics: includeDiagnostics.value,
      route: route.fullPath
    })
    messageStore.success('Thanks — your report was sent.')
    message.value = ''
    includeDiagnostics.value = true
    modalStore.showReportIssueModal = false
  } catch {
    messageStore.error('Could not send report — please try again.')
  } finally {
    submitting.value = false
  }
}
</script>
