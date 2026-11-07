import eslint from '@eslint/js'
import tseslint from 'typescript-eslint'
import vueParser from 'vue-eslint-parser'
import prettierConfig from 'eslint-config-prettier'
import pluginVue from 'eslint-plugin-vue'
import globals from 'globals'

export default tseslint.config(
  {
    ignores: [
      '**/node_modules/**/*',
      '**/dist/**/*',
      '**/dev-dist/**/*',
      'src/script/lexer/MyParserCst.d.ts',
      '**/*.mjs',
      'src/type/MyParserCst.d.ts'
    ]
  },
  eslint.configs.recommended,
  ...tseslint.configs.strictTypeChecked.map((config) => ({
    ...config,
    files: ['src/**/*']
  })),
  ...tseslint.configs.stylisticTypeChecked.map((config) => ({
    ...config,
    files: ['src/**/*']
  })),
  ...pluginVue.configs['flat/strongly-recommended'],
  {
    files: ['src/**/*'],
    languageOptions: {
      parser: vueParser,
      parserOptions: {
        parser: tseslint.parser,
        sourceType: 'module',
        extraFileExtensions: ['.vue'],
        projectService: true,
        tsconfigRootDir: import.meta.dirname
      },
      globals: {
        ...globals.browser
      }
    },
    rules: {
      '@typescript-eslint/strict-boolean-expressions': 'error',
      '@typescript-eslint/restrict-template-expressions': ['error', { allowNumber: true }],
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }],
      'vue/multi-word-component-names': 'off',
      '@typescript-eslint/no-unnecessary-type-parameters': 'off',
      // Structural bans: dialogs render through BaseModal/ConfirmDialog,
      // Config sections through SectionCard (scoped below).
      'vue/no-restricted-html-elements': [
        'error',
        {
          element: 'v-dialog',
          message: 'Use BaseModal (or ConfirmDialog for confirmations) instead of a raw <v-dialog>.'
        },
        {
          element: 'VDialog',
          message: 'Use BaseModal (or ConfirmDialog for confirmations) instead of a raw <VDialog>.'
        }
      ],
      'vue/attributes-order': 'error',
      'vue/require-explicit-emits': 'error',
      'vue/require-explicit-slots': 'error'
      // Deliberately not enabled: vue/no-bare-strings-in-template (would flag
      // the whole existing copy surface; needs an i18n/copy strategy first) and
      // vue/no-restricted-block (no concrete block target identified yet).
    }
  },
  {
    // Config sections must use SectionCard. v-card stays a legitimate
    // primitive elsewhere (Gallery/View), so this ban is scoped here only.
    files: ['src/components/Page/Config/**/*.vue'],
    rules: {
      'vue/no-restricted-html-elements': [
        'error',
        {
          element: 'v-dialog',
          message: 'Use BaseModal (or ConfirmDialog for confirmations) instead of a raw <v-dialog>.'
        },
        {
          element: 'VDialog',
          message: 'Use BaseModal (or ConfirmDialog for confirmations) instead of a raw <VDialog>.'
        },
        {
          element: 'v-card',
          message: 'Use SectionCard instead of a raw <v-card> in Config sections.'
        },
        {
          element: 'VCard',
          message: 'Use SectionCard instead of a raw <VCard> in Config sections.'
        }
      ]
    }
  },
  {
    // The single legitimate direct owner of the raw dialog.
    files: ['src/components/Modal/BaseModal.vue'],
    rules: {
      'vue/no-restricted-html-elements': 'off'
    }
  },
  {
    // SectionCard owns its raw card; AdvancedConfig keeps its raw card until
    // a title slot can satisfy the Danger Zone source pin. The dialog ban
    // still applies to both.
    files: [
      'src/components/Page/Config/SectionCard.vue',
      'src/components/Page/Config/AdvancedConfig.vue'
    ],
    rules: {
      'vue/no-restricted-html-elements': [
        'error',
        {
          element: 'v-dialog',
          message: 'Use BaseModal (or ConfirmDialog for confirmations) instead of a raw <v-dialog>.'
        },
        {
          element: 'VDialog',
          message: 'Use BaseModal (or ConfirmDialog for confirmations) instead of a raw <VDialog>.'
        }
      ]
    }
  },
  {
    files: ['playwright.config.ts', 'tests/**/*.ts'],
    plugins: {
      '@typescript-eslint': tseslint.plugin
    },
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: {
        sourceType: 'module'
      },
      globals: {
        ...globals.node,
        ...globals.browser,
        NodeJS: 'readonly'
      }
    },
    rules: {
      '@typescript-eslint/no-explicit-any': 'off',
      '@typescript-eslint/no-non-null-assertion': 'off',
      'no-redeclare': 'off',
      // The base rule reads TypeScript type positions as real bindings, so a
      // named parameter in a function *type* (`draw: (attempt: number) => …`)
      // is reported as an unused variable. Swap in the TypeScript-aware rule,
      // which understands them, as the `src/**` block above already does.
      'no-unused-vars': 'off',
      '@typescript-eslint/no-unused-vars': ['error', { argsIgnorePattern: '^_' }]
    }
  },
  prettierConfig
)
