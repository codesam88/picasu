import { AxiosInstance, AxiosError } from 'axios'
import { errorDisplay } from '@/script/utils/errorDisplay'

/**
 * Worker-realm response-error contract. Workers cannot import stores, so
 * auth UX travels to the main thread over postMessage (never imported here:
 * this module stays worker-agnostic plain callbacks). The data worker
 * expects 401 (session/token expiry) and 500; the image worker expects 500
 * only — a 401 there is logged like any other unexpected code because
 * image-worker silence for session expiry is by design (the data worker's
 * 401 owns the redirect). Every other response-error status and every
 * responseless failure is logged at info level; the rejection always
 * propagates so callers keep owning their catch.
 */
export function setupWorkerAxiosInterceptor(
  axiosInstance: AxiosInstance,
  callbacks: {
    notify: (payload: { text: string; color: 'error' }) => void
    onUnauthorized?: () => void
  },
  workerName: string
) {
  axiosInstance.interceptors.response.use(
    (response) => response,
    (error: AxiosError) => {
      const status = error.response?.status
      if (status === 401) {
        if (callbacks.onUnauthorized !== undefined) {
          callbacks.onUnauthorized()
        } else {
          console.info(`worker ${workerName} got unexpected return code 401`)
        }
        return Promise.reject(error)
      }
      if (status === 500) {
        const errorMessage = errorDisplay(error)
        callbacks.notify({ text: errorMessage, color: 'error' })
        return Promise.reject(error)
      }
      if (status !== undefined) {
        console.info(`worker ${workerName} got unexpected return code ${status}`)
      } else {
        console.info(`worker ${workerName} got an unexpected error without a response`)
      }
      return Promise.reject(error)
    }
  )
}
