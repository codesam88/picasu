<template>
  <v-col cols="12">
    <v-card border flat>
      <v-card-title class="font-weight-bold">Frontend</v-card-title>
      <v-divider thickness="4" variant="double"></v-divider>

      <v-list-item class="pt-4">
        <v-list-item-title class="mb-2">Thumbnail size</v-list-item-title>
        <v-slider
          show-ticks="always"
          v-model="subRowHeightScaleValue"
          :min="250"
          :max="450"
          :step="10"
          hide-details
          thumb-size="16"
          prepend-icon="mdi-minus"
          append-icon="mdi-plus"
          color="primary"
          @click:prepend="onSubRowHeightScaleUpdate(-10)"
          @click:append="onSubRowHeightScaleUpdate(10)"
        ></v-slider>
      </v-list-item>

      <v-divider></v-divider>

      <v-list-item
        title="Show Filename Chip"
        @click="onShowFilenameChipUpdate(!showFilenameChipValue)"
      >
        <template #append>
          <v-switch
            :model-value="showFilenameChipValue"
            @update:model-value="onShowFilenameChipUpdate"
            color="primary"
            inset
            hide-details
            @click.stop
          ></v-switch>
        </template>
      </v-list-item>
    </v-card>
  </v-col>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { useConstStore } from '@/store/constStore'

const constStore = useConstStore('mainId')

const subRowHeightScaleValue = computed<number>({
  get: () => constStore.subRowHeightScale,
  set: (newVal: number | null) => {
    const value = newVal ?? constStore.subRowHeightScale
    const clamped = Math.max(250, Math.min(450, value))
    constStore.updateSubRowHeightScale(clamped).catch((error: unknown) => {
      console.error('Failed to update subRowHeightScale (via setter):', error)
    })
  }
})

const showFilenameChipValue = computed<boolean>({
  get: () => constStore.showFilenameChip,
  set: (newVal: boolean | null) => {
    constStore.updateShowFilenameChip(newVal ?? true).catch((error: unknown) => {
      console.error('Failed to update showFilenameChip (via setter):', error)
    })
  }
})

const onSubRowHeightScaleUpdate = (newValue: number | null) => {
  const value = newValue ?? constStore.subRowHeightScale
  const clamped = Math.max(250, Math.min(450, value))
  constStore.updateSubRowHeightScale(clamped).catch((error: unknown) => {
    console.error('Failed to update subRowHeightScale:', error)
  })
}

const onShowFilenameChipUpdate = (newValue: boolean | null) => {
  constStore.updateShowFilenameChip(newValue ?? true).catch((error: unknown) => {
    console.error('Failed to update showFilenameChip:', error)
  })
}
</script>
