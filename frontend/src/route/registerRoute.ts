// src/route/registerRoute.ts

import { RouteRecordRaw } from 'vue-router'
import 'vue-router'

import RegisterPage from '@/components/Login/RegisterPage.vue'

export const registerRoute: RouteRecordRaw = {
  path: '/register',
  component: RegisterPage,
  name: 'register',

  meta: {
    basicString: null,
    level: 1,
    baseName: 'register',
    getParentPage: (route) => {
      return {
        name: 'timeline',
        params: {},
        query: route.query
      }
    },
    getChildPage: (route) => {
      return {
        name: 'register',
        params: {},
        query: route.query
      }
    }
  }
}
