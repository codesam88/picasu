// src/router.ts

import { RouteRecordRaw } from 'vue-router'
import 'vue-router'

import ViewPage from '@/components/View/ViewPage.vue'

import GalleryShare from '@/components/Gallery/GalleryShare.vue'
import { PageReturnType } from './pageReturnType'

export const shareRoute: RouteRecordRaw = {
  path: '/share/:albumId-:shareId',
  component: GalleryShare,
  name: 'share',
  meta: {
    basicString: null,
    baseName: 'share',
    level: 1,
    getParentPage: (route) => {
      return {
        name: 'share',
        params: { hash: undefined, subhash: undefined },
        query: route.query
      }
    },
    getChildPage: (route, hash) => {
      return {
        name: `shareViewPage`,
        params: { hash: hash, subhash: undefined },
        query: route.query
      }
    }
  },
  children: [
    {
      path: 'view/:hash',
      component: ViewPage,
      name: `shareViewPage`,
      meta: {
        level: 2,
        baseName: 'share',
        getParentPage: (route, albumId, shareId) => {
          console.log('123')
          return {
            name: 'share',
            params: { albumId: albumId, shareId: shareId },
            query: route.query
          }
        },
        getChildPage: function (): PageReturnType {
          throw new Error('Function not implemented.')
        }
      }
    }
  ]
}
