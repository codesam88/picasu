# User Menu (Settings + Logout)

## Summary

Add a User button with a dropdown menu in the top-right toolbar, giving access
to a Settings modal and Logout.

## Motivation

The app has no user-facing controls — no way to change preferences or end a
session. Settings are buried in the /config page and there is no logout
mechanism at all. This feature puts common user actions one click away.

## Layout

A single icon button with `mdi-account-circle` is added as the rightmost element
in `GalleryBar.vue`'s toolbar, after the Upload button. It is gated on
`route.meta.level === 1` (same visibility as Theme and Upload).

Clicking it opens a `v-menu` containing two items in a `v-list`:

| Item     | Icon         | Action                                 |
| -------- | ------------ | -------------------------------------- |
| Settings | `mdi-cog`    | Opens `UserSettingsModal`              |
| Log out  | `mdi-logout` | Clears JWT cookie, redirects to /login |

## Settings modal

A new `UserSettingsModal.vue` component wraps the two existing config sections
in a `BaseModal`:

```vue
<BaseModal v-model="show" title="Settings" width="600">
  <FrontendConfig />
  <ChangePassword v-model:has-password="hasPassword" />
</BaseModal>
```

- **FrontendConfig** — Thumbnail size slider + Show Filename Chip toggle.
  Reused as-is; no changes needed.
- **ChangePassword** — Enable/disable password protection + password fields.
  Reused as-is; `hasPassword` is fetched from `configStore.fetchConfig()` on
  modal open.
- Modal is opened by setting `modalStore.showUserSettingsModal = true`.
- Rendered conditionally in `App.vue` (same pattern as all other modals).
- On open, the modal fetches config from `configStore.fetchConfig()` and passes
  `hasPassword` to ChangePassword (`v-model:has-password`).
- Each section saves independently (FrontendConfig saves on change;
  ChangePassword has a Save button) — no umbrella save needed.

## Logout behavior

JWT is stored in a cookie named `jwt` (set via `js-cookie` on login). No
backend logout endpoint exists — JWTs are stateless, so client-side cookie
removal is sufficient.

```ts
import Cookies from "js-cookie";
import { useRouter } from "vue-router";

const router = useRouter();
Cookies.remove("jwt");
router.push({ name: "login" });
```

Consideration: the login page already has a redirect-back-to-previous-page
mechanism (`redirectionStore`), so after logout the user sees a clean login
page without any redirect loop.

## Files changed

| File                                                        | Change                                                   |
| ----------------------------------------------------------- | -------------------------------------------------------- |
| `frontend/src/components/NavBar/GalleryBars/GalleryBar.vue` | Add User button + `v-menu` at right end                  |
| `frontend/src/components/Modal/UserSettingsModal.vue`       | **New** — modal wrapping FrontendConfig + ChangePassword |
| `frontend/src/store/modalStore.ts`                          | Add `showUserSettingsModal` to state + `dialogKeys`      |
| `frontend/src/components/App.vue`                           | Render `UserSettingsModal` conditionally                 |
