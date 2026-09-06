<template>
  <v-tooltip location="top" text="Options">
    <template #activator="{ props: tooltipProps }">
      <v-menu location="start">
        <template #activator="{ props: menuProps }">
          <v-btn
            v-bind="mergeProps(tooltipProps, menuProps)"
            icon="mdi-dots-vertical"
            variant="text"
            size="small"
            class="control-btn"
            v-testid="'photo-menu'"
          ></v-btn>
        </template>
        <v-list role="menu">
          <template v-if="database.isTrashed">
            <ItemRestore :index-list="[props.index]" />
            <ItemEditAlbums label="Restore to Album..." />
            <ItemPermanentlyDelete :index-list="[props.index]" />
          </template>
          <template v-else>
            <ItemViewOriginalFile
              :src="getSrc(database.id, true, database.ext, database.updateAt)"
              :isolation-id="props.isolationId"
              :hash="database.id"
            />
            <ItemDownload :index-list="[props.index]" />
            <ItemFindInTimeline :hash="props.hash" />
            <v-divider></v-divider>
            <ItemEditTags />
            <ItemEditAlbums />
            <ItemDelete :index-list="[props.index]" />
            <v-divider></v-divider>
            <ItemScanAlbum />
            <ItemRegenerateThumbnailByFrame v-if="currentFrameStore.video !== null" />
            <ItemRotateImage v-if="database.type === 'image'" />
          </template>
        </v-list>
      </v-menu>
    </template>
  </v-tooltip>
</template>
<script setup lang="ts">
import { mergeProps } from 'vue'
import { GalleryImage, GalleryVideo, IsolationId } from '@type/types'
import { getSrc } from '@utils/getter'
import ItemViewOriginalFile from '@Menu/MenuItem/ItemViewOriginalFile.vue'
import ItemDownload from '@Menu/MenuItem/ItemDownload.vue'
import ItemFindInTimeline from '@Menu/MenuItem/ItemFindInTimeline.vue'
import ItemEditTags from '@Menu/MenuItem/ItemEditTags.vue'
import ItemEditAlbums from '@Menu/MenuItem/ItemEditAlbums.vue'
import ItemDelete from '@Menu/MenuItem/ItemDelete.vue'
import ItemScanAlbum from '@Menu/MenuItem/ItemScanAlbum.vue'
import ItemRestore from '@Menu/MenuItem/ItemRestore.vue'
import ItemPermanentlyDelete from '@Menu/MenuItem/ItemPermanentlyDelete.vue'
import ItemRegenerateThumbnailByFrame from '@Menu/MenuItem/ItemRegenerateThumbnailByFrame.vue'
import ItemRotateImage from '@Menu/MenuItem/ItemRotateImage.vue'
import { useCurrentFrameStore } from '@/store/currentFrameStore'
const props = defineProps<{
  isolationId: IsolationId
  hash: string
  index: number
  database: GalleryImage | GalleryVideo
}>()
const currentFrameStore = useCurrentFrameStore(props.isolationId)
</script>
