import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'
import { playwright } from '@vitest/browser-playwright'
import { resolve } from 'path'

const rootDir = import.meta.dirname

export default defineConfig({
  plugins: [vue()],
  test: {
    projects: [
      {
        extends: true,
        test: {
          name: 'unit',
          environment: 'node',
          exclude: ['node_modules/**', 'tests/playwright/**', 'src/**/*.browser.test.{ts,tsx}']
        }
      },
      {
        extends: true,
        test: {
          name: 'component',
          include: ['src/components/**/*.browser.test.{ts,tsx}'],
          setupFiles: ['./src/components/testing/browserSetup.ts'],
          browser: {
            enabled: true,
            headless: true,
            provider: playwright(),
            instances: [{ browser: 'chromium' }]
          }
        }
      }
    ]
  },
  resolve: {
    alias: {
      '@': resolve(rootDir, 'src'),
      '@Menu': resolve(rootDir, 'src/components/Menu'),
      '@worker': resolve(rootDir, 'src/worker'),
      '@utils': resolve(rootDir, 'src/script/utils'),
      '@type': resolve(rootDir, 'src/type'),
      path: 'path-browserify'
    }
  }
})
