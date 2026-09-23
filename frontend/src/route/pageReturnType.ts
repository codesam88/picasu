import { LocationQuery, RouteLocationNormalizedLoaded } from 'vue-router'

export interface PageReturnType {
  name: string
  params: {
    assetId?: string | string[] | undefined
    albumId?: string | string[]
    shareId?: string | string[]
  }

  query: LocationQuery
}

declare module 'vue-router' {
  interface RouteMeta {
    level: number
    baseName: BaseName
    getParentPage: (
      router: RouteLocationNormalizedLoaded,
      albumId?: string,
      shareId?: string
    ) => PageReturnType
    getChildPage: (
      router: RouteLocationNormalizedLoaded,
      assetId: string | undefined
    ) => PageReturnType
  }
}

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
  | 'config'
