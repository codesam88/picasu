---
status: open
type: feature
priority: high
area: frontend
---

## Notes

Cross-cutting error handling and user-facing activity logging overhaul.

### Problem statement

Errors are currently invisible to the user in many code paths. Workers swallow errors via `console.error`, interceptors only handle specific status codes, and transient notifications (snackbars) disappear after 2.5s with no way to review them. There is no backend error logging for admin diagnostics.

### Scope

#### 1. Worker error surfacing (frontend)

- `workerAxiosInterceptor.ts` currently only notifies on 500. Extend to cover 401, 403, 404, and any non-retryable error.
- `toImgWorker.ts` catch blocks silently `console.error`. Forward failures to the main thread via `postToMainImg.notification(...)`, distinguishing transient (network timeout) from actionable (401 session expired, 403 access denied).
- `toDataWorker.ts` — same treatment for data fetch failures.
- `SmallImageContainer.ts` `checkAndFetch` catch — surface repeated failures (e.g. after N retries) rather than silently dropping.

#### 2. Real error pages (frontend)

- Dedicated error route/page for unhandled navigation errors, API 500s on page load, and component error boundaries.
- `router.onError` and Vue `onErrorCaptured` should redirect or render the error page with status code, request path, and retry action.
- Differentiate between recoverable (retry/back) and fatal (login required, server down).

#### 3. Activity log (frontend)

Replace ephemeral snackbar-only notifications with a persistent user-centric activity log:

- **Dropdown widget** (header bar): shows last N (e.g. 10) activity entries (success, error, info). Each entry has icon, message, timestamp. Clicking expands or navigates to full log.
- **Full activity log page** (settings/profile area): filterable list of all activity entries with severity, timestamp, source action, and detail.
- **Storage**: Pinia store backed by `localStorage` (or IndexedDB for large logs). Configurable retention (e.g. 500 entries max, 30-day TTL).
- **Migration path**: Existing `messageStore.push()` calls remain as-is initially; the activity log subscribes to the same event stream and persists entries. Gradually migrate callers to an `activityLog` API that handles both toast + persistence.
- **User control**: toggle in settings to enable/disable toast popups independently of log persistence.

#### 4. Backend error logging

- Structured JSON error logging for all API error responses (status >= 400).
- Log request method, path, status, response time, and client IP (sanitized).
- Admin-facing endpoint or CLI command to tail/search recent error logs.
- Consider `tracing` crate with `tracing-subscriber` for structured logs if not already in use.

### Current state (discovered during album thumbnail fix)

| Location                       | Issue                                                                 |
| ------------------------------ | --------------------------------------------------------------------- |
| `workerAxiosInterceptor.ts:12` | Only handles 500; 401/403/404 silently rejected                       |
| `toImgWorker.ts:156`           | Catch block: `console.error` only, no user notification               |
| `toDataWorker.ts:27`           | Interceptor only notifies on 500                                      |
| `SmallImageContainer.ts:40`    | `checkAndFetch` catch: silent `console.error`                         |
| `axiosInterceptor.ts`          | Main-thread interceptor is well-structured; workers should mirror it  |
| Backend                        | No structured error logging; errors go to stderr via `rocket` default |

### Suggested implementation order

1. Fix worker interceptors + catch blocks (quick win, immediate value)
2. Add activity log Pinia store + localStorage persistence
3. Migrate `messageStore` callers to also write to activity log
4. Build dropdown widget in header
5. Build full activity log page in settings
6. Add error pages (route + error boundary)
7. Backend structured error logging
