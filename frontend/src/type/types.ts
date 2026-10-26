// src/type/types.ts
import { z } from 'zod'
import {
  FileEntrySchema,
  tagInfoSchema,
  scrollbarDataSchema,
  displayElementSchema,
  rowSchema,
  rowWithOffsetSchema,
  prefetchSchema,
  SubRowSchema,
  albumInfoSchema,
  prefetchReturnSchema,
  ShareSchema,
  ShareInfoSchema,
  ResolvedShareSchema,
  TokenResponseSchema,
  BackendDataParser
} from '@type/schemas'

// Basic types
export type FileEntry = z.infer<typeof FileEntrySchema>
export type TagInfo = z.infer<typeof tagInfoSchema>
export type AlbumInfo = z.infer<typeof albumInfoSchema>
export type ScrollbarData = z.infer<typeof scrollbarDataSchema>
export type DisplayElement = z.infer<typeof displayElementSchema>
export type Row = z.infer<typeof rowSchema>
export type RowWithOffset = z.infer<typeof rowWithOffsetSchema>
export type Prefetch = z.infer<typeof prefetchSchema>
export type PrefetchReturn = z.infer<typeof prefetchReturnSchema>
export type SubRow = z.infer<typeof SubRowSchema>
export type Share = z.infer<typeof ShareSchema>
export type ShareInfo = z.infer<typeof ShareInfoSchema>
export type ResolvedShare = z.infer<typeof ResolvedShareSchema>
export type TokenResponse = z.infer<typeof TokenResponseSchema>

// Core unified types (transformed)
export type UnifiedData = z.infer<typeof BackendDataParser>
export type GalleryImage = Extract<UnifiedData, { type: 'image' }>
export type GalleryVideo = Extract<UnifiedData, { type: 'video' }>
export type GalleryAlbum = Extract<UnifiedData, { type: 'album' }>

export type EnrichedUnifiedData = UnifiedData & {
  thumbhashUrl: string | null
  timestamp: number
  assetId: string
}

// List view data structure
export interface SlicedData {
  index: number
  data: EnrichedUnifiedData
  hashToken: string
  assetId: string
}

export type Sorting = 'ascending' | 'descending' | 'random' | 'similar'
export type IsolationId = 'mainId' | 'subId' | 'tempId' | 'shareId'
export type FetchDataMethod = 'batch' | 'single'
export type MessageColor = 'error' | 'success' | 'info'

export interface Message {
  text: string
  color: MessageColor
}

export interface EditShareData {
  albumId: string
  share: ShareInfo
  displayName: string
}

export interface ShareFormData {
  description: string
  passwordRequired: boolean
  password: string
  /** A password exists server-side (edit mode). */
  passwordSet: boolean
  /** The password field was edited this session: blanking a previously
   * non-empty value means "remove", while an untouched blank means "keep". */
  passwordDirty: boolean
  expireEnabled: boolean
  expDuration: number | null
  showUpload: boolean
  showDownload: boolean
  showMetadata: boolean
}
