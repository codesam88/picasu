/**
 * These settings are used for both global application configuration (AppConfig)
 * and local UI preferences (e.g., isMobile, showFilenameChip).
 * Refactored to avoid dangerous default values for AppConfig and use inline types.
 */

import { IsolationId } from '@type/types'
import { defineStore } from 'pinia'
import { AppConfig, getConfig } from '@/api/config'
import { updateAdminConfig } from '@/api/adminConfig'
import type { RuntimeConfigPatch } from '@/api/adminConfig'
import { tryWithMessageStore } from '@/script/utils/try_catch'

export const useConfigStore = (isolationId: IsolationId) =>
  defineStore('configStore' + isolationId, {
    state: (): {
      // AppConfig is nullable to indicate it hasn't been loaded yet
      config: AppConfig | undefined
      // UI State
      isMobile: boolean
      showFilenameChip: boolean
    } => ({
      config: undefined,
      isMobile: false,
      showFilenameChip: false
    }),

    actions: {
      async fetchConfig() {
        // Return if already loaded to avoid redundant calls
        if (this.config) return

        return await tryWithMessageStore(isolationId, async () => {
          const data = await getConfig()
          this.config = data
        })
      },
      async updateConfig(newConfig: Partial<AppConfig>) {
        return await tryWithMessageStore(isolationId, async () => {
          const patch: RuntimeConfigPatch = {}
          if (newConfig.uploadFolder !== undefined && newConfig.uploadFolder !== null) {
            patch.uploadFolder = newConfig.uploadFolder
          }
          if (newConfig.maxUploadSize !== undefined) patch.maxUploadSize = newConfig.maxUploadSize
          if (newConfig.readOnlyMode !== undefined) patch.readOnlyMode = newConfig.readOnlyMode
          if (newConfig.fsNotifyWatcher !== undefined)
            patch.fsNotifyWatcher = newConfig.fsNotifyWatcher
          if (newConfig.normalizeUploadFilenames !== undefined) {
            patch.normalizeUploadFilenames = newConfig.normalizeUploadFilenames
          }
          if (newConfig.validateUploadContent !== undefined) {
            patch.validateUploadContent = newConfig.validateUploadContent
          }
          if (newConfig.useClientTimestampInfo !== undefined) {
            patch.useClientTimestampInfo = newConfig.useClientTimestampInfo
          }
          if (newConfig.allowRegistration !== undefined) {
            patch.allowRegistration = newConfig.allowRegistration
          }
          const response = await updateAdminConfig(patch)

          // Merge updates into local state
          if (this.config) {
            Object.assign(this.config, newConfig)
            if (response?.config) Object.assign(this.config, response.config)
          } else {
            const data = await getConfig()
            this.config = data
          }
          return true
        })
      }
    }
  })()
