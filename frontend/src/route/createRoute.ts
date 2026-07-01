// src/router.ts

import { Component } from 'vue'
import { RouteRecordRaw } from 'vue-router'
import 'vue-router'

import ViewPage from '@/components/View/ViewPage.vue'

type BaseName =
  | 'timeline'
  | 'favorite'
  | 'archived'
  | 'trashed'
  | 'albums'
  | 'videos'
  | 'album'
  | 'tags'
  | 'login'
  | 'share'
  | 'links'

// ======================================
// Define a Helper Function to Create Routes
// ======================================

/**
 * Creates a main route with an optional child route.
 *
 * @param path - The base path for the route.
 * @param component - The component to be rendered.
 * @param name - The unique name for the route.
 * @returns An array containing the RouteRecordRaw object.
 */
export function createRoute(baseName: BaseName, component: Component): RouteRecordRaw[] {
  const mainRoute: RouteRecordRaw = {
    path: `/${baseName}`,
    component: component,
    name: baseName,
    meta: {
      level: 1,
      baseName: baseName,
      getParentPage: (route) => {
        return {
          name: baseName,
          params: { hash: undefined, subhash: undefined },
          query: route.query
        }
      },
      getChildPage: (route, hash) => {
        return {
          name: `${baseName}ViewPage`,
          params: { hash: hash, subhash: undefined },
          query: route.query
        }
      }
    },
    children: [
      {
        path: 'view/:hash',
        component: ViewPage,
        name: `${baseName}ViewPage`,
        meta: {
          level: 2,
          baseName: baseName,
          getParentPage: (route) => {
            return {
              name: baseName,
              params: { hash: undefined, subhash: undefined },
              query: route.query
            }
          },
          // No child page below level 2 (level 3/4 were eliminated). Identity fallback
          // since RouteMeta.getChildPage is required by the type but has no real caller here.
          getChildPage: (route) => {
            return {
              name: `${baseName}ViewPage`,
              params: { hash: route.params.hash, subhash: undefined },
              query: route.query
            }
          }
        }
      }
    ]
  }
  return [mainRoute]
}
