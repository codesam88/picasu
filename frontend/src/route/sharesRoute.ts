// src/router.ts

import { RouteRecordRaw } from 'vue-router'
import 'vue-router'

import LinksPage from '@/components/Page/LinksPage.vue'

export const sharesRoute: RouteRecordRaw = {
  path: '/shares',
  component: LinksPage,
  name: 'shares',

  meta: {
    basicString: null,
    level: 1,
    baseName: 'shares',
    getParentPage: (route) => {
      return {
        name: 'timeline',
        params: {},
        query: route.query
      }
    },
    getChildPage: (route) => {
      return {
        name: 'shares',
        params: {},
        query: route.query
      }
    }
  }
}
