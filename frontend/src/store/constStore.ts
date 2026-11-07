import { IsolationId } from '@type/types'
import { defineStore } from 'pinia'
import {
  storeSubRowHeightScale,
  getSubRowHeightScale,
  storeShowInfo,
  getShowInfo,
  storeConcurrencyNumber,
  getConcurrencyNumber,
  storeLimitRation,
  getLimitRation,
  storeTheme,
  getTheme,
  storeShowFilenameChip,
  getShowFilenameChip,
  storeDisableImg,
  getDisableImg,
  type ThemeName
} from '@/db/settingsDb'
import { useSessionStore } from '@/store/sessionStore'

export type { ThemeName }

const DEFAULT_THEME: ThemeName = 'dark'

// Session identity for theme scoping: `undefined` (global key) when logged
// out. Read at action time, mirroring how albumStore reads session state.
function scopedThemeUserId(): string | undefined {
  return useSessionStore('mainId').profile?.userId
}

export const useConstStore = (isolationId: IsolationId) =>
  defineStore('constStore' + isolationId, {
    state: (): {
      subRowHeightScale: number
      showInfo: boolean
      concurrencyNumber: number
      limitRatio: boolean
      theme: ThemeName
      showFilenameChip: boolean
      disableImg: boolean
    } => ({
      subRowHeightScale: 250,
      showInfo: false,
      concurrencyNumber: Math.max(Math.floor(navigator.hardwareConcurrency / 2), 1),
      // default: false
      limitRatio: false,
      theme: DEFAULT_THEME,
      showFilenameChip: false,
      disableImg: false
    }),

    actions: {
      async toggleTheme(vuetifyTheme?: { global: { name: { value: string } } }): Promise<void> {
        const newTheme: ThemeName = this.theme === 'light' ? 'dark' : 'light'
        this.theme = newTheme
        await storeTheme(newTheme, scopedThemeUserId())

        // Update Vuetify theme if provided
        if (vuetifyTheme) {
          vuetifyTheme.global.name.value = newTheme
        }
      },

      async updateSubRowHeightScale(value: number): Promise<void> {
        this.subRowHeightScale = value
        await storeSubRowHeightScale(value)
      },

      async loadSubRowHeightScale(): Promise<void> {
        const storedValue = await getSubRowHeightScale()
        if (storedValue !== null) {
          this.subRowHeightScale = storedValue
        }
      },

      async updateShowInfo(value: boolean): Promise<void> {
        this.showInfo = value
        await storeShowInfo(value)
      },

      async loadShowInfo(): Promise<void> {
        const stored = await getShowInfo()
        if (stored !== null) {
          this.showInfo = stored
        }
      },

      async updateConcurrencyNumber(value: number): Promise<void> {
        // Ensure minimum concurrency of 1
        const v = Math.max(1, Math.floor(value))
        this.concurrencyNumber = v
        await storeConcurrencyNumber(v)
      },

      async loadConcurrencyNumber(): Promise<void> {
        const stored = await getConcurrencyNumber()
        if (typeof stored === 'number' && Number.isFinite(stored) && stored > 0) {
          this.concurrencyNumber = Math.floor(stored)
        }
      },
      async updateLimitRation(value: boolean): Promise<void> {
        this.limitRatio = value
        await storeLimitRation(this.limitRatio)
      },

      async loadLimitRation(): Promise<void> {
        const stored = await getLimitRation()
        if (typeof stored === 'boolean') {
          this.limitRatio = stored
        }
      },
      async updateTheme(value: ThemeName): Promise<void> {
        this.theme = value
        await storeTheme(value, scopedThemeUserId())
      },

      // Load the theme for `userId` (`null` = logged out → global key),
      // falling back to the app default when nothing is stored. Resolves the
      // store only — callers apply `this.theme` to Vuetify.
      async loadThemeFor(userId: string | null): Promise<void> {
        const stored = await getTheme(userId ?? undefined)
        this.theme = stored ?? DEFAULT_THEME
      },

      // Boot entry: load the theme for the persisted session identity, so a
      // reload while signed in keeps that user's theme.
      async loadTheme(): Promise<void> {
        await this.loadThemeFor(scopedThemeUserId() ?? null)
      },

      async updateShowFilenameChip(value: boolean): Promise<void> {
        this.showFilenameChip = value
        await storeShowFilenameChip(value)
      },

      async loadShowFilenameChip(): Promise<void> {
        const stored = await getShowFilenameChip()
        if (typeof stored === 'boolean') {
          this.showFilenameChip = stored
        }
      },

      async updateDisableImg(value: boolean): Promise<void> {
        this.disableImg = value
        await storeDisableImg(value)
      },

      async loadDisableImg(): Promise<void> {
        const stored = await getDisableImg()
        if (typeof stored === 'boolean') {
          this.disableImg = stored
        }
      }
    }
  })()
