<template>
  <Gallery
    class="w-100 h-100"
    isolation-id="mainId"
    :basic-string="props.basicString"
    :search-string="searchString"
    :include-personal="props.includePersonal"
  />
</template>

<script setup lang="ts">
import { onBeforeMount, ref } from 'vue'
import Gallery from './Gallery.vue'
import { LocationQueryValue, useRoute } from 'vue-router'

const route = useRoute()
const searchString = ref<LocationQueryValue | LocationQueryValue[] | undefined>(null)

const props = defineProps<{
  basicString: string | null
  includePersonal?: boolean
}>()

onBeforeMount(() => {
  searchString.value = route.query.search
})
</script>
