---
status: done
type: feature
priority: medium
area: frontend
---

## Notes

Minimal in-app "Report issue..." dialog posting directly from the browser to
Web3Forms. No backend change, no attachments (Web3Forms free tier), no captcha,
no per-install boards, no AI triage — all deferred.

Owner decisions (settled): key via `VITE_WEB3FORMS_KEY` env with the project
access key as built-in default; diagnostics set approved (app version, route,
user agent, timestamp, viewport, recent error messages); 20s re-submit
throttle; launcher is the logged-in user dropdown (GalleryBar `v-menu`) after
Settings, titled "Report issue...".

## Implementation steps

1. Diagnostics + version source (`script/utils/appInfo.ts` + test).
2. Client error ring-buffer (`script/utils/errorLog.ts` + test, listeners in
   `main.ts` only).
3. Web3Forms transport (`api/reportIssue.ts` + test): dedicated `fetch`
   (never global axios), env key with prefilled default, 20s throttle.
4. Modal store flag + `App.vue` mount + GalleryBar launcher item
   (`data-testid="user-menu-report-issue"`).
5. `ReportIssueModal.vue` form + component test (validation, diagnostics
   toggle + payload preview, honeypot, anti-double-submit, toasts).
6. Docs paragraph + final gate (`frontend-check`, `frontend-test`, `format`).
