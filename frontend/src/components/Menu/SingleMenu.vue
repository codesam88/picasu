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
          <ItemViewOriginalFile
            :src="getSrc(database.id, true, database.ext, database.updateAt)"
            :isolation-id="props.isolationId"
            :hash="database.id"
          />
          <ItemDownload :index-list="[props.index]" />
          <ItemFindInTimeline :hash="props.hash" />
          <v-divider></v-divider>
          <!-- Trashed items get no tag/album edits here: restore is the recovery
               action and moves the item out of .trash/ into an album via the
               assign-album dialog. -->
          <ItemEditTags v-if="!isTrashedContext" />
          <ItemEditAlbums v-if="!isTrashedContext" />
          <ItemDelete v-if="!isTrashedContext" :index-list="[props.index]" />
          <ItemRestore v-if="isTrashedContext" :index-list="[props.index]" />
          <ItemPermanentlyDelete v-if="isTrashedContext" :index-list="[props.index]" />
          <v-divider></v-divider>
          <ItemScanAlbum v-if="!isTrashedContext" />
          <ItemRegenerateThumbnailByFrame v-if="currentFrameStore.video !== null" />
          <ItemRotateImage v-if="!isTrashedContext && database.type === 'image'" />
        </v-list>
      </v-menu>
    </template>
  </v-tooltip>
</template>
<script setup lang="ts">
import { computed, mergeProps } from 'vue'
import { GalleryImage, GalleryVideo, IsolationId } from '@type/types'
import { getSrc } from '@utils/getter'
import ItemViewOriginalFile from '@Menu/MenuItem/ItemViewOriginalFile.vue'
import ItemDownload from '@Menu/MenuItem/ItemDownload.vue'
import ItemFindInTimeline from '@Menu/MenuItem/ItemFindInTimeline.vue'
import ItemEditTags from '@Menu/MenuItem/ItemEditTags.vue'
import ItemEditAlbums from '@Menu/MenuItem/ItemEditAlbums.vue'
import ItemDelete from '@Menu/MenuItem/ItemDelete.vue'
import ItemPermanentlyDelete from '@Menu/MenuItem/ItemPermanentlyDelete.vue'
import ItemScanAlbum from '@Menu/MenuItem/ItemScanAlbum.vue'
import ItemRestore from '@Menu/MenuItem/ItemRestore.vue'
import ItemRegenerateThumbnailByFrame from '@Menu/MenuItem/ItemRegenerateThumbnailByFrame.vue'
import ItemRotateImage from '@Menu/MenuItem/ItemRotateImage.vue'
import { useCurrentFrameStore } from '@/store/currentFrameStore'
import { useRoute } from 'vue-router'
const route = useRoute()
const isTrashedContext = computed(() => route.meta.baseName === 'trashed')
const props = defineProps<{
  isolationId: IsolationId
  hash: string
  index: number
  database: GalleryImage | GalleryVideo
}>()
const currentFrameStore = useCurrentFrameStore(props.isolationId)
</script>
