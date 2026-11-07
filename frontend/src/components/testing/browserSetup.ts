import { createVuetify } from 'vuetify'
import * as components from 'vuetify/components'
import * as directives from 'vuetify/directives'
import 'vuetify/styles'

/// Fresh Vuetify instance for a browser test. A shared instance leaks
/// theme/display state between tests; `defaultTheme` is explicit because the
/// Vuetify 4 default is `system`, which makes rendered theme state depend on
/// the host OS preference.
export function createTestVuetify() {
  return createVuetify({
    components,
    directives,
    theme: { defaultTheme: 'light' }
  })
}
