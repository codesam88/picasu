import { test } from '@playwright/test'

test('dump homepage ARIA snapshot', async ({ page }) => {
  await page.goto('/home', { waitUntil: 'networkidle', timeout: 30000 })
  await page.waitForTimeout(2000)

  const roles = await page.evaluate(() => {
    const implicitRole = (el: Element): string | null => {
      const tag = el.tagName.toLowerCase()
      const a = el as HTMLAnchorElement
      if (tag === 'a' && a.href) return 'link'
      if (tag === 'button') return 'button'
      if (tag === 'nav') return 'navigation'
      if (tag === 'main') return 'main'
      if (tag === 'header') return 'banner'
      if (tag === 'footer') return 'contentinfo'
      if (tag === 'aside') return 'complementary'
      if (tag === 'form') return 'form'
      if (tag === 'ul' || tag === 'ol') return 'list'
      if (tag === 'li') return 'listitem'
      if (tag === 'input' && (el as HTMLInputElement).type === 'text') return 'textbox'
      if (tag === 'input' && (el as HTMLInputElement).type === 'search') return 'searchbox'
      if (tag === 'select') return 'combobox'
      if (tag === 'textarea') return 'textbox'
      if (tag === 'h1' || tag === 'h2' || tag === 'h3') return 'heading'
      if (tag === 'img') return 'img'
      if (tag === 'table') return 'table'
      if (tag === 'hr') return 'separator'
      return null
    }

    const all = document.querySelectorAll(
      'nav, main, header, footer, aside, form, [role], a[href], button, input, select, textarea, ul, ol, li, h1, h2, h3, h4, h5, h6, img, hr, i, span, div'
    )
    const results: any[] = []
    for (const el of all) {
      const explicitRole = el.getAttribute('role')
      const implicit = explicitRole ? null : implicitRole(el)
      const role = explicitRole || implicit
      if (!role) continue
      const label = el.getAttribute('aria-label') || ''
      const a = el as HTMLAnchorElement
      const url = a.href || ''
      const text = (el.textContent || '').trim().slice(0, 80)
      const el2 = el as HTMLElement
      const visible = el2.offsetParent !== null || el2.offsetWidth > 0 || el2.offsetHeight > 0
      if (!visible) continue

      results.push({
        role,
        label: label.slice(0, 40),
        text: text.slice(0, 40),
        url: url && url !== 'about:blank' ? url.replace(window.location.origin, '') : '',
        depth: (() => {
          let d = 0,
            p = el.parentElement
          while (p) {
            d++
            p = p.parentElement
          }
          return d
        })()
      })
    }
    return results
  })

  console.log('role             label/text                       depth  url')
  console.log('─'.repeat(70))
  for (const r of roles) {
    const role = r.role.padEnd(16).slice(0, 16)
    const label = `"${r.label || r.text}"`.padEnd(36).slice(0, 36)
    const depth = String(r.depth).padStart(3)
    console.log(`  ${role} ${label} ${depth}  ${r.url}`)
  }
})
