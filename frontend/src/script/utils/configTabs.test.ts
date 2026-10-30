/// Config tab rearrangement: Server / Users / Invites, Danger Zone in Server tab.
import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const configPage = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/ConfigPage.vue'),
  'utf-8'
)
const advanced = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/Config/AdvancedConfig.vue'),
  'utf-8'
)
const navPanel = readFileSync(
  resolve(import.meta.dirname, '../../components/Page/PageLayout/NavPanel.vue'),
  'utf-8'
)
const galleryBar = readFileSync(
  resolve(import.meta.dirname, '../../components/NavBar/GalleryBars/GalleryBar.vue'),
  'utf-8'
)
const routes = readFileSync(resolve(import.meta.dirname, '../../route/routes.ts'), 'utf-8')

describe('config tab rearrangement', () => {
  test('tabs are Server / Users / Invites with no Administration tab', () => {
    expect(configPage).toMatch(/value="server"/)
    expect(configPage).toMatch(/value="users"/)
    expect(configPage).toMatch(/>Users</)
    expect(configPage).toMatch(/value="invites"/)
    expect(configPage).toMatch(/>Invites</)
    expect(configPage).not.toMatch(/Administration/)
    expect(configPage).not.toMatch(/value="admin"/)
  })

  test('UserManagement renders in users tab, InviteManagement in invites tab', () => {
    expect(configPage).toMatch(/value="users"[\s\S]*?<UserManagement/)
    expect(configPage).toMatch(/value="invites"[\s\S]*?<InviteManagement/)
  })

  test('AdvancedConfig is titled Danger Zone with error styling', () => {
    expect(advanced).toMatch(/text-error">Danger Zone/)
    expect(advanced).not.toMatch(/Advanced Settings/)
  })

  test('Allow Registration lives in Danger Zone, Export Config is a separate Server card', () => {
    expect(advanced).toMatch(/Allow Registration/)
    expect(configPage).not.toMatch(/Allow Registration/)
    expect(configPage).toMatch(/Export Config/)
  })
})

describe('admin nav entry', () => {
  test('config entry is renamed to Admin and hidden from non-admins', () => {
    expect(navPanel).not.toMatch(/title="Config"/)
    // The gate lives on the Admin entry itself, not elsewhere in the panel.
    expect(navPanel).toMatch(/v-if="isAdmin"[\s\S]{0,200}title="Admin"/)
  })

  test('Trash is the last nav entry for everyone', () => {
    const adminIndex = navPanel.indexOf('title="Admin"')
    const trashIndex = navPanel.indexOf('to="/trashed"')
    expect(adminIndex).toBeGreaterThanOrEqual(0)
    expect(trashIndex).toBeGreaterThan(adminIndex)
    // No gate on the Trash row: it renders for admins and users alike.
    const trashLine = navPanel.split('\n').find((line) => line.includes('to="/trashed"')) ?? ''
    expect(trashLine).not.toMatch(/v-if/)
  })

  test('page and document titles call it Admin', () => {
    expect(galleryBar).toMatch(/config:\s*'Admin'/)
    expect(routes).toMatch(/config:\s*'Admin'/)
  })
})

describe('shares nav entry', () => {
  test('Shares page is in the main nav under Tags', () => {
    const tagsIndex = navPanel.indexOf('title="Tags"')
    const sharesIndex = navPanel.indexOf('title="Shares"')
    expect(tagsIndex).toBeGreaterThanOrEqual(0)
    expect(sharesIndex).toBeGreaterThan(tagsIndex)
    expect(navPanel).toMatch(/to="\/shares"/)
    expect(navPanel).not.toMatch(/title="Links"/)
  })

  test('page and document titles call it Shares', () => {
    expect(galleryBar).toMatch(/shares:\s*'Shares'/)
    expect(routes).toMatch(/shares:\s*'Shares'/)
  })

  test('route lives at /shares with no /links legacy', () => {
    const sharesRoute = readFileSync(
      resolve(import.meta.dirname, '../../route/sharesRoute.ts'),
      'utf-8'
    )
    expect(sharesRoute).toMatch(/path:\s*'\/shares'/)
    expect(sharesRoute).toMatch(/name:\s*'shares'/)
    expect(routes).not.toMatch(/\/links/)
  })
})

describe('shares tab', () => {
  const shareMgmt = readFileSync(
    resolve(import.meta.dirname, '../../components/Page/Config/ShareManagement.vue'),
    'utf-8'
  )

  test('Shares tab exists for admins', () => {
    expect(configPage).toMatch(/value="shares"/)
    expect(configPage).toMatch(/>Shares</)
  })

  test('ShareManagement renders in the shares window', () => {
    expect(configPage).toMatch(/value="shares"[\s\S]*?<ShareManagement/)
  })

  test('lists every share across albums from the store', () => {
    expect(shareMgmt).toMatch(/for \([^)]*of album\.shareList\)/)
  })

  test('exposes the available share operations', () => {
    expect(shareMgmt).toMatch(/EditShareModal/)
    expect(shareMgmt).toMatch(/ShareDeleteConfirmModal/)
    expect(shareMgmt).toMatch(/\/share\//)
    expect(shareMgmt).toMatch(/copyTextToClipboard/)
    expect(shareMgmt).toMatch(/v-if="showCopy"/)
    expect(shareMgmt).not.toMatch(/navigator\.clipboard/)
    expect(shareMgmt).not.toMatch(/execCommand/)
  })
})
