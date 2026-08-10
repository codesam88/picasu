<template>
  <v-tooltip location="top" text="Options">
    <template #activator="{ props: tooltipProps }">
      <v-menu>
        <template #activator="{ props: menuProps }">
          <v-btn
            v-bind="mergeProps(tooltipProps, menuProps)"
            icon="mdi-dots-vertical"
            v-testid="'album-menu'"
          ></v-btn>
        </template>
        <v-list>
          <template v-if="album.isTrashed">
            <Restore :index-list="[props.index]" />
            <EditAlbums />
          </template>
          <template v-else>
            <FindInTimeline :hash="props.hash" />
            <v-divider></v-divider>
            <EditTags />
            <Delete :index-list="[props.index]" />
          </template>
        </v-list>
      </v-menu>
    </template>
  </v-tooltip>
</template>

<script setup lang="ts">
import { mergeProps } from 'vue'
import { GalleryAlbum, IsolationId } from '@type/types'
import FindInTimeline from '@Menu/MenuItem/ItemFindInTimeline.vue'
import EditTags from '@Menu/MenuItem/ItemEditTags.vue'
import EditAlbums from '@Menu/MenuItem/ItemEditAlbums.vue'
import Delete from '@Menu/MenuItem/ItemDelete.vue'
import Restore from '@Menu/MenuItem/ItemRestore.vue'

const props = defineProps<{
  isolationId: IsolationId
  hash: string
  index: number
  album: GalleryAlbum
}>()
</script>
