<template>
  <div id="further-metadata" v-if="entries.length > 0">
    <v-list-subheader>Further metadata</v-list-subheader>
    <v-list
      class="pa-0"
      lines="two"
      bg-color="transparent"
      :density="compact ? 'compact' : 'default'"
    >
      <!--
        Read-only by contract. The bucket is what `ExifTool` reported and the
        app does not model, keyed `Group:Tag`; the app does not know what any of
        these fields mean well enough to write one back, so this section renders
        text and nothing that could edit it. That is the whole difference from
        the rating, the tags and the description above it.
      -->
      <v-list-item v-for="entry in entries" :key="entry.key" class="further-metadata-pair">
        <v-list-item-title class="text-wrap further-metadata-key">{{
          entry.key
        }}</v-list-item-title>
        <v-list-item-subtitle class="text-wrap further-metadata-value">
          {{ entry.value }}
        </v-list-item-subtitle>
      </v-list-item>
    </v-list>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue'
import { GalleryImage, GalleryVideo } from '@type/types'

const props = defineProps<{
  database: GalleryImage | GalleryVideo
  compact?: boolean
}>()

/**
 * One row per `Group:Tag` key, in the order the API sent them. The backend
 * stores the bucket in a `BTreeMap`, so the order is already the grouped key
 * order; sorting again would only hide a change in what the API returns.
 */
const entries = computed(() =>
  Object.entries(props.database.furtherMetadata).map(([key, value]) => ({ key, value }))
)
</script>
