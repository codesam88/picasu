<template>
  <v-col cols="12">
    <SectionCard title="Frontend">
      <v-list-item class="pt-4">
        <v-list-item-title class="mb-2">Theme</v-list-item-title>
        <v-select
          v-model="themeValue"
          data-testid="frontend-theme-select"
          :items="themeItems"
          item-title="label"
          item-value="value"
          density="compact"
          hide-details
          color="primary"
        ></v-select>
      </v-list-item>

      <v-divider></v-divider>

      <v-list-item class="pt-4">
        <v-list-item-title class="mb-2">Thumbnail size</v-list-item-title>
        <v-slider
          v-model="subRowHeightScaleValue"
          :label="`${subRowHeightScaleValue} px`"
          show-ticks="always"
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
        data-testid="frontend-show-filename-chip"
        title="Show Filename Chip"
        @click="onShowFilenameChipUpdate(!showFilenameChipValue)"
      >
        <template #append>
          <v-switch
            data-testid="frontend-show-filename-chip-switch"
            :model-value="showFilenameChipValue"
            color="primary"
            inset
            hide-details
            @update:model-value="onShowFilenameChipUpdate"
            @click.stop
          ></v-switch>
        </template>
      </v-list-item>

      <v-list-item
        v-if="personalAlbumsEnabled"
        data-testid="frontend-include-personal"
        title="Include personal items in timeline"
        @click="onToggle(!includePersonal)"
      >
        <template #append>
          <v-switch
            data-testid="frontend-include-personal-switch"
            :model-value="includePersonal"
            color="primary"
            inset
            hide-details
            @update:model-value="onToggle"
            @click.stop
          ></v-switch>
        </template>
      </v-list-item>

      <v-list-item title="Disable images" @click="onDisableImgUpdate(!disableImgValue)">
        <template #append>
          <v-switch
            :model-value="disableImgValue"
            color="primary"
            inset
            hide-details
            @update:model-value="onDisableImgUpdate"
            @click.stop
          ></v-switch>
        </template>
      </v-list-item>
    </SectionCard>
  </v-col>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import SectionCard from '@/components/Page/Config/SectionCard.vue'
import { useRoute } from 'vue-router'
import { useConstStore, type ThemeName } from '@/store/constStore'
import { useConfigStore } from '@/store/configStore'
import { getIncludePersonal, setIncludePersonal } from '@/script/utils/personalTimelineToggle'
import { refreshGalleryAfterMutation } from '@/script/hook/usePrefetch'
import { useTheme } from 'vuetify'

const constStore = useConstStore('mainId')
const configStore = useConfigStore('mainId')
const route = useRoute()
const vuetifyTheme = useTheme()

const includePersonal = ref(getIncludePersonal())
const personalAlbumsEnabled = computed(() => configStore.config?.personalAlbumsEnabled === true)

onMounted(() => {
  if (configStore.config === undefined) {
    void configStore.fetchConfig()
  }
})

async function onToggle(value: boolean | string | null) {
  includePersonal.value = value === true || value === 'true'
  setIncludePersonal(includePersonal.value)
  await refreshGalleryAfterMutation('mainId', route)
}

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

const disableImgValue = computed<boolean>({
  get: () => constStore.disableImg,
  set: (newVal: boolean | null) => {
    constStore.updateDisableImg(newVal ?? false).catch((error: unknown) => {
      console.error('Failed to update disableImg (via setter):', error)
    })
  }
})

const onDisableImgUpdate = (newValue: boolean | null) => {
  constStore.updateDisableImg(newValue ?? false).catch((error: unknown) => {
    console.error('Failed to update disableImg:', error)
  })
}

// One entry per ThemeName (see its definition): a new theme adds an option
// here plus a Vuetify `themes` entry.
const themeItems: { label: string; value: ThemeName }[] = [
  { label: 'Dark', value: 'dark' },
  { label: 'Light', value: 'light' }
]

const themeValue = computed<ThemeName>({
  get: () => constStore.theme,
  set: (value: ThemeName) => {
    void applyThemeUpdate(value)
  }
})

async function applyThemeUpdate(value: ThemeName): Promise<void> {
  try {
    await constStore.updateTheme(value)
    vuetifyTheme.global.name.value = constStore.theme
  } catch (error: unknown) {
    console.error('Failed to update theme (via FrontendConfig):', error)
  }
}
</script>
