<template>
  <v-tooltip location="top" text="Options">
    <template #activator="{ props: tooltipProps }">
      <v-menu location="start">
        <template #activator="{ props: menuProps }">
          <v-btn
            v-bind="mergeProps(tooltipProps, menuProps)"
            icon="mdi-dots-vertical"
            v-testid="'share-menu'"
          ></v-btn>
        </template>
        <v-list>
          <ItemViewOriginalFile
            :src="getSrcOriginal(database.id, true, database.ext, database.updateAt)"
            :hash="database.id"
            :isolation-id="props.isolationId"
          />
          <ItemDownload :index-list="[props.index]" />
        </v-list>
      </v-menu>
    </template>
  </v-tooltip>
</template>
<script setup lang="ts">
import { mergeProps } from 'vue'
import { GalleryImage, GalleryVideo, IsolationId } from '@type/types'
import { getSrcOriginal } from '@utils/getter'
import ItemViewOriginalFile from '@Menu/MenuItem/ItemViewOriginalFile.vue'
import ItemDownload from '@Menu/MenuItem/ItemDownload.vue'
const props = defineProps<{
  isolationId: IsolationId
  hash: string
  index: number
  database: GalleryImage | GalleryVideo
}>()
</script>
