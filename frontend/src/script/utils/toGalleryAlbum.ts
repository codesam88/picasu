import { AlbumInfo, GalleryAlbum } from '@type/types'

/**
 * Reconstructs a GalleryAlbum-shaped object (the full UnifiedData album type)
 * from the lighter AlbumInfo store entry. Used wherever a component needs an
 * album's data outside a grid context (e.g. GalleryTemp, which needs a full
 * album object for the album currently being browsed).
 *
 * Fields with no AlbumInfo equivalent (startTime/endTime/lastModifiedTime/
 * cover/tags/itemCount/itemSize/description/rating) are not derivable here
 * and default to empty/zero — callers needing those should read from
 * dataStore instead.
 */
export function toGalleryAlbum(info: AlbumInfo): GalleryAlbum {
  return {
    type: 'album',
    id: info.albumId,
    title: info.albumName,
    startTime: null,
    endTime: null,
    lastModifiedTime: 0,
    cover: null,
    thumbhash: null,
    tags: [],
    itemCount: 0,
    itemSize: 0,
    pending: false,
    description: null,
    isFavorite: false,
    isArchived: false,
    isTrashed: false,
    rating: null,
    updateAt: 0,
    shareList: Object.fromEntries(info.shareList)
  }
}
