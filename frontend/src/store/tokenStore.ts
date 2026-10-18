import { IsolationId, TokenResponse } from '@type/types'
import { jwtDecode } from 'jwt-decode'
import { defineStore } from 'pinia'
import axios from 'axios'
import { TokenResponseSchema } from '@/type/schemas'
import { storeAssetToken } from '@/db/db'

interface JwtPayload {
  timestamp: number
  exp?: number
  [key: string]: unknown
}

/**
 * Renew a serving token handed out with less than this much validity left.
 * Margin against the dispatch→server window; server TTL is 300 s.
 */
export const RENEW_BUFFER_SECS = 120

export const useTokenStore = (isolationId: IsolationId) =>
  defineStore('tokenStore' + isolationId, {
    state: (): {
      timestampToken: string | null
      /** One serving token (JWT with a content-hash claim) per asset_id. */
      assetTokenMap: Map<string, string>
      _renewingTimestamp: Promise<void> | null
    } => ({
      timestampToken: null,
      assetTokenMap: new Map<string, string>(),
      _renewingTimestamp: null
    }),

    actions: {
      _isExpired(exp?: number): boolean {
        return typeof exp === 'number' && exp < Math.floor(Date.now() / 1000)
      },

      _decodeToken(token: string): JwtPayload | null {
        try {
          return jwtDecode<JwtPayload>(token)
        } catch (err) {
          console.warn('Invalid JWT:', err)
          return null
        }
      },

      _isTokenExpired(token: string): boolean {
        const decoded = this._decodeToken(token)
        return decoded ? this._isExpired(decoded.exp) : true
      },

      _withinRenewBuffer(token: string): boolean {
        const decoded = this._decodeToken(token)
        if (decoded === null) return true
        if (typeof decoded.exp !== 'number') return false
        return decoded.exp - Math.floor(Date.now() / 1000) < RENEW_BUFFER_SECS
      },

      _getTimestampFromToken(): number | null {
        if (this.timestampToken == null) return null
        const decoded = this._decodeToken(this.timestampToken)
        return decoded?.timestamp ?? null
      },

      _getTimestampFromAssetToken(assetId: string): number | undefined {
        const token = this.assetTokenMap.get(assetId)
        if (token === undefined) return undefined
        const decoded = this._decodeToken(token)
        return decoded?.timestamp
      },

      async _updateTimestampToken(): Promise<void> {
        try {
          const response = await axios.post('/post/renew-timestamp-token', {
            token: this.timestampToken
          })
          const parsed: TokenResponse = TokenResponseSchema.parse(response.data)
          this.timestampToken = parsed.token
        } catch (err) {
          console.error('Failed to renew timestamp token:', err)
        }
      },

      async _updateAssetToken(expiredToken: string): Promise<string | null> {
        if (this.timestampToken == null) {
          console.error('Missing timestampToken for authorization')
          return null
        }

        try {
          const response = await axios.post(
            '/post/renew-hash-token',
            { expiredHashToken: expiredToken },
            { headers: { Authorization: `Bearer ${this.timestampToken}` } }
          )
          const parsed: TokenResponse = TokenResponseSchema.parse(response.data)
          return parsed.token
        } catch (err) {
          console.error('Failed to update asset token:', err)
          return null
        }
      },

      /**
       * Updates the timestamp token with a lock to prevent concurrent renewals.
       */
      async _refreshTimestampTokenWithLock(): Promise<void> {
        if (this._renewingTimestamp) {
          await this._renewingTimestamp
          return
        }

        this._renewingTimestamp = (async () => {
          await this._updateTimestampToken()
        })().finally(() => {
          this._renewingTimestamp = null
        })

        await this._renewingTimestamp
      },

      async _ensureAssetTokenFresh(assetId: string): Promise<string | null> {
        const currentToken = this.assetTokenMap.get(assetId)
        if (currentToken === undefined) {
          console.error(`No token found for assetId: ${assetId}`)
          return null
        }

        if (!this._withinRenewBuffer(currentToken)) {
          return currentToken
        }

        await this.refreshTimestampTokenIfExpired()

        const newToken = await this._updateAssetToken(currentToken)
        if (newToken !== null) {
          this.assetTokenMap.set(assetId, newToken)
        }
        return newToken
      },

      async refreshTimestampTokenIfExpired(): Promise<void> {
        if (this.timestampToken == null || !this._withinRenewBuffer(this.timestampToken)) return
        await this._refreshTimestampTokenWithLock()
      },

      /**
       * Server-authoritative renewal: renews regardless of the local clock.
       */
      async forceRenewTimestampToken(): Promise<void> {
        await this._refreshTimestampTokenWithLock()
      },

      /**
       * Server-authoritative renewal of one asset token: refreshes the
       * timestamp token first when it is within the renew buffer (the
       * server needs an unexpired bearer), then renews the asset token
       * regardless of the local clock.
       */
      async forceRenewAssetToken(assetId: string): Promise<string | null> {
        const currentToken = this.assetTokenMap.get(assetId)
        if (currentToken === undefined) {
          console.error(`No token found for assetId: ${assetId}`)
          return null
        }

        await this.refreshTimestampTokenIfExpired()

        const newToken = await this._updateAssetToken(currentToken)
        if (newToken !== null) {
          this.assetTokenMap.set(assetId, newToken)
        }
        return newToken
      },

      async refreshAssetTokenIfExpired(assetId: string): Promise<void> {
        await this._ensureAssetTokenFresh(assetId)
      },

      async tryRefreshAndStoreTokenToDb(assetId: string): Promise<void> {
        const freshToken = await this._ensureAssetTokenFresh(assetId)
        if (freshToken === null) return

        await storeAssetToken(assetId, freshToken)
      }
    }
  })()
