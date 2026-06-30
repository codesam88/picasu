<template>
  <v-list-item>
    <template #prepend>
      <v-icon class="mr-2">mdi-star-outline</v-icon>
    </template>
    <v-rating
      v-model="ratingModel"
      :readonly="props.readonly"
      clearable
      density="compact"
      color="amber"
      active-color="amber"
      aria-label="Rating"
      @update:model-value="onRatingChange"
    />
  </v-list-item>
</template>

<script setup lang="ts">
import { ref, watch } from 'vue'
import { editRating } from '@/api/editRating'
import { IsolationId } from '@type/types'

const props = defineProps<{
  isolationId: IsolationId
  index: number
  rating: number | null
  readonly?: boolean
}>()

const ratingModel = ref<number>(props.rating ?? 0)

watch(
  () => props.rating,
  (val) => {
    ratingModel.value = val ?? 0
  }
)

async function onRatingChange(val: string | number) {
  if (props.readonly) return
  const num = typeof val === 'string' ? parseInt(val) : val
  const newRating = num === 0 || isNaN(num) ? null : num
  await editRating(newRating, props.index, props.isolationId)
}
</script>
