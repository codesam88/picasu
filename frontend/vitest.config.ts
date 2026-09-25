import { defineConfig } from 'vitest/config'
import { resolve } from 'path'

const rootDir = import.meta.dirname

export default defineConfig({
  test: {
    environment: 'node',
    exclude: ['node_modules/**', 'tests/playwright/**']
  },
  resolve: {
    alias: {
      '@': resolve(rootDir, 'src'),
      '@utils': resolve(rootDir, 'src/script/utils'),
      '@type': resolve(rootDir, 'src/type')
    }
  }
})
