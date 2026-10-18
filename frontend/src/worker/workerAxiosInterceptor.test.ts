import axios, { AxiosError, type AxiosResponse, type InternalAxiosRequestConfig } from 'axios'
import { beforeEach, describe, expect, test, vi } from 'vitest'
import { setupWorkerAxiosInterceptor } from './workerAxiosInterceptor'

const rejectWithStatus = (instance: ReturnType<typeof axios.create>, status: number) => {
  instance.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
    const response: AxiosResponse = {
      status,
      statusText: String(status),
      data: { message: `boom ${status}` },
      headers: {},
      config
    }
    throw new AxiosError(
      `Request failed with status code ${status}`,
      'ERR_BAD_REQUEST',
      config,
      null,
      response
    )
  }
}

const rejectWithoutResponse = (instance: ReturnType<typeof axios.create>) => {
  instance.defaults.adapter = async (config: InternalAxiosRequestConfig) => {
    throw new AxiosError('Network Error', 'ERR_NETWORK', config, null, undefined)
  }
}

beforeEach(() => {
  vi.restoreAllMocks()
})

describe('setupWorkerAxiosInterceptor', () => {
  test('401 calls onUnauthorized once, never notify, rejection propagates', async () => {
    const instance = axios.create()
    const notify = vi.fn()
    const onUnauthorized = vi.fn()
    setupWorkerAxiosInterceptor(instance, { notify, onUnauthorized }, 'data')
    rejectWithStatus(instance, 401)

    await expect(instance.get('/get/get-data')).rejects.toBeTruthy()

    expect(onUnauthorized).toHaveBeenCalledTimes(1)
    expect(notify).not.toHaveBeenCalled()
  })

  test('500 calls notify once with error text and color error, never onUnauthorized', async () => {
    const instance = axios.create()
    const notify = vi.fn()
    const onUnauthorized = vi.fn()
    setupWorkerAxiosInterceptor(instance, { notify, onUnauthorized }, 'data')
    rejectWithStatus(instance, 500)

    await expect(instance.get('/get/get-data')).rejects.toBeTruthy()

    expect(notify).toHaveBeenCalledTimes(1)
    expect(notify).toHaveBeenCalledWith(
      expect.objectContaining({ color: 'error', text: expect.stringContaining('boom 500') })
    )
    expect(onUnauthorized).not.toHaveBeenCalled()
  })

  test.each([403, 429])(
    'status %i logs info with worker name and code, calls neither callback',
    async (status) => {
      const instance = axios.create()
      const notify = vi.fn()
      const onUnauthorized = vi.fn()
      const infoSpy = vi.spyOn(console, 'info').mockImplementation(() => {})
      setupWorkerAxiosInterceptor(instance, { notify, onUnauthorized }, 'data')
      rejectWithStatus(instance, status)

      await expect(instance.get('/get/get-data')).rejects.toBeTruthy()

      expect(notify).not.toHaveBeenCalled()
      expect(onUnauthorized).not.toHaveBeenCalled()
      expect(infoSpy).toHaveBeenCalledOnce()
      expect(infoSpy.mock.calls[0]?.[0]).toContain('worker data got unexpected return code')
      expect(infoSpy.mock.calls[0]?.[0]).toContain(String(status))
    }
  )

  test('401 without onUnauthorized logs like an unexpected code (image-worker shape)', async () => {
    const instance = axios.create()
    const notify = vi.fn()
    const infoSpy = vi.spyOn(console, 'info').mockImplementation(() => {})
    setupWorkerAxiosInterceptor(instance, { notify }, 'image')
    rejectWithStatus(instance, 401)

    await expect(instance.get('/get/asset/1.jpg')).rejects.toBeTruthy()

    expect(notify).not.toHaveBeenCalled()
    expect(infoSpy).toHaveBeenCalledOnce()
    expect(infoSpy.mock.calls[0]?.[0]).toContain('worker image got unexpected return code 401')
  })

  test('responseless failure logs no-response variant, calls neither callback', async () => {
    const instance = axios.create()
    const notify = vi.fn()
    const onUnauthorized = vi.fn()
    const infoSpy = vi.spyOn(console, 'info').mockImplementation(() => {})
    setupWorkerAxiosInterceptor(instance, { notify, onUnauthorized }, 'data')
    rejectWithoutResponse(instance)

    await expect(instance.get('/get/get-data')).rejects.toBeTruthy()

    expect(notify).not.toHaveBeenCalled()
    expect(onUnauthorized).not.toHaveBeenCalled()
    expect(infoSpy).toHaveBeenCalledOnce()
    expect(infoSpy.mock.calls[0]?.[0]).toContain(
      'worker data got an unexpected error without a response'
    )
  })
})
