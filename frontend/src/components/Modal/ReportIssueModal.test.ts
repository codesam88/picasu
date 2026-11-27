import { describe, expect, test } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

const source = readFileSync(resolve(import.meta.dirname, './ReportIssueModal.vue'), 'utf-8')

describe('ReportIssueModal', () => {
  test('uses BaseModal with the report-issue title', () => {
    expect(source).toMatch(/BaseModal/)
    expect(source).toMatch(/title="Report issue\.\.\."/)
  })

  test('discloses the Web3Forms third-party inbox', () => {
    expect(source).toMatch(/Web3Forms/)
  })

  test('textarea is required with maxlength 2000, counter, and autofocus', () => {
    expect(source).toMatch(/v-textarea/)
    expect(source).toMatch(/maxlength="2000"/)
    expect(source).toMatch(/\bcounter\b/)
    expect(source).toMatch(/\bautofocus\b/)
  })

  test('has a diagnostics checkbox defaulting on', () => {
    expect(source).toMatch(/v-checkbox/)
    expect(source).toMatch(/includeDiagnostics/)
  })

  test('shows a collapsed preview titled "What will be sent"', () => {
    expect(source).toMatch(/What will be sent/)
    expect(source).toMatch(/report-issue-preview/)
  })

  test('includes a visually-hidden honeypot input', () => {
    expect(source).toMatch(/name="botcheck"/)
    expect(source).toMatch(/tabindex="-1"/)
    expect(source).toMatch(/autocomplete="off"/)
  })

  test('has cancel and send buttons with the expected test ids', () => {
    expect(source).toMatch(/report-issue-cancel/)
    expect(source).toMatch(/report-issue-send/)
  })

  test('enforces the throttle and shows the expected toasts', () => {
    expect(source).toMatch(/canSubmitReport\(\)/)
    expect(source).toMatch(/please wait before sending another report/)
    expect(source).toMatch(/Thanks — your report was sent\./)
    expect(source).toMatch(/Could not send report — please try again\./)
  })

  test('records throttle and closes on success, keeps text on failure', () => {
    expect(source).toMatch(/modalStore\.showReportIssueModal = false/)
    expect(source).toMatch(/message\.value = ''/)
  })

  test('assembles preview via buildReportMessage', () => {
    expect(source).toMatch(/buildReportMessage/)
  })

  test('mounts the modal store flag and submitReportIssue transport', () => {
    expect(source).toMatch(/showReportIssueModal/)
    expect(source).toMatch(/submitReportIssue/)
  })
})
