import { z } from 'zod'

const VarName = z.string().regex(/^\$[a-zA-Z_][a-zA-Z0-9_]*$/)

export const GivenDirAlbum = z
  .object({
    dir_album: z.string(),
    id_as: VarName.optional()
  })
  .strict()

export const GivenPhoto = z
  .object({
    photo: z.string(),
    id_as: VarName.optional(),
    format: z.enum(['jpeg', 'png']).optional(),
    width: z.number().int().positive().optional(),
    height: z.number().int().positive().optional(),
    tags: z.array(z.string()).optional(),
    exif_date: z.string().optional()
  })
  .strict()

export const GivenEmpty = z
  .object({
    empty: z.literal(true)
  })
  .strict()

export const GivenRemove = z
  .object({
    remove: z.string()
  })
  .strict()

export const GivenMove = z
  .object({
    move: z.string(),
    to: z.string()
  })
  .strict()

export const GivenIndex = z
  .object({
    index: z.string().min(1)
  })
  .strict()

/// Declared backend startup options. Keys mirror the real Picasu option
/// names (`AppConfig` field names in `backend/src/model/config.rs`
/// `config_tier_table`): `bootstrap_admin` = `PICASU_BOOTSTRAP_ADMIN`,
/// `login_ip_base_ban_secs` = `PICASU_LOGIN_IP_BASE_BAN_SECS`. Absent
/// `server:` keeps the harness defaults (bootstrap open, weak password
/// hashing).
export const ServerDeclaration = z
  .object({
    bootstrap_admin: z.boolean().optional(),
    login_ip_base_ban_secs: z.number().int().nonnegative().optional()
  })
  .strict()

export const GivenConfig = z
  .object({
    config: z
      .object({
        read_only_mode: z.boolean().optional(),
        auth_key: z.string().optional(),
        fs_notify_watcher: z.boolean().optional(),
        allow_registration: z.boolean().optional()
      })
      .strict()
  })
  .strict()

/// Mint a single-use invite token via the admin API. `username` binds the
/// invite to that user id (403 on mismatch at redeem); omit it for an open
/// invite that registers any id. `expires_in_hours` defaults to 168 (the
/// conventional 7-day lifetime; `0` never expires). The plaintext token is
/// exposed as `${id_as}` for link interpolation. The minting `user:` given
/// must come first: invites exist only on a non-empty store.
export const GivenInvite = z
  .object({
    invite: z.object({
      username: z.string().min(1).optional(),
      expires_in_hours: z.number().int().optional()
    }),
    id_as: VarName.optional()
  })
  .strict()

export const GivenUser = z
  .object({
    user: z.object({
      user_id: z.string().min(1),
      password: z.string().min(1),
      admin: z.boolean().optional()
    })
  })
  .strict()

export const GivenPhotoRaw = z
  .object({
    photo_raw: z.string().min(1),
    format: z.enum(['jpeg', 'png']).optional(),
    width: z.number().int().positive().optional(),
    height: z.number().int().positive().optional()
  })
  .strict()

export const GivenSourceFile = z
  .object({
    source_file: z.string().min(1),
    /// Id of a pinned fixture in `utils/snapfab/capabilities.json`. The
    /// binary-safe alternative to `format` for the formats snapfab cannot
    /// encode (TIFF, WebP, MP4, MOV); `source_file` stays the uploaded name.
    fixture: z.string().min(1).optional(),
    format: z.enum(['jpeg', 'png']).optional(),
    width: z.number().int().positive().optional(),
    height: z.number().int().positive().optional(),
    id_as: VarName.optional()
  })
  .strict()
  .refine((value) => !(value.fixture && value.format), {
    message:
      'source_file takes either `fixture` (pinned manifest bytes) or `format` (snapfab-generated), not both'
  })

/// A user provisioned offline via `picasu-cmd` before backend launch
/// (phase A). Unlike `user:` (online bootstrap, first admin only), this can
/// seed any number of users with any role — the only way to create the
/// second (non-admin) user. Requires `server:` (the server must be down
/// while the CLI holds the store).
export const GivenCliUser = z
  .object({
    cli_user: z.object({
      user_id: z.string().min(1),
      password: z.string().min(1),
      admin: z.boolean().optional()
    })
  })
  .strict()

export const GivenItem = z.union([
  GivenDirAlbum,
  GivenPhoto,
  GivenPhotoRaw,
  GivenSourceFile,
  GivenEmpty,
  GivenRemove,
  GivenConfig,
  GivenUser,
  GivenInvite,
  GivenCliUser,
  GivenMove,
  GivenIndex
])

const RoleLabel = z.string()

export const UiWhenNavigate = z
  .object({
    navigate: z.string()
  })
  .strict()

export const UiWhenClick = z
  .object({
    click: RoleLabel
  })
  .strict()

export const UiWhenFill = z
  .object({
    fill: RoleLabel,
    value: z.string()
  })
  .strict()

export const UiWhenSelect = z
  .object({
    select: RoleLabel,
    option: z.string()
  })
  .strict()

export const UiWhenSubmit = z
  .object({
    submit: z.literal(true)
  })
  .strict()

export const UiWhenWait = z
  .object({
    'wait.ms': z.number().int().positive()
  })
  .strict()

export const UiWhenClickText = z
  .object({
    'click.text': z.string()
  })
  .strict()

export const UiWhenClickIcon = z
  .object({
    'click.icon': z.string()
  })
  .strict()

export const UiWhenClickFirst = z
  .object({
    'click.first': z.literal(true)
  })
  .strict()

export const UiWhenClickSelectFirst = z
  .object({
    'click.select_first': z.literal(true)
  })
  .strict()

export const UiWhenClickSelectText = z
  .object({
    'click.select_text': z.string()
  })
  .strict()

export const UiWhenClickTestId = z
  .object({
    'click.testid': z.string()
  })
  .strict()

export const UiWhenKeyboard = z
  .object({
    keyboard: z.string()
  })
  .strict()

export const UiWhenBrowserBack = z
  .object({
    'browser.back': z.literal(true)
  })
  .strict()

export const UiWhenUploadFiles = z
  .object({
    'upload.files': z.object({
      trigger: z.string(),
      files: z.array(z.string()).min(1)
    })
  })
  .strict()

export const UiWhenSetAutoRename = z
  .object({
    'set.auto_rename': z.boolean()
  })
  .strict()

/// Explicit UI login step. `password` is inline: the step fills the login
/// form and requires landing on `/timeline`. Keeps credentials visible in
/// the scenario instead of a harness password lookup.
export const UiWhenLogin = z
  .object({
    login: z.object({
      user: z.string().min(1),
      password: z.string().min(1)
    })
  })
  .strict()

/// Explicit index step. Runs `POST /post/index/album` for the given album
/// (default `/`) against the scenario backend and polls
/// `GET /get/index/status` to idle/completed.
export const UiWhenIndex = z
  .object({
    index: z.string().min(1)
  })
  .strict()

/// Explicit page reload. The hand-written register specs prove the session
/// survives reload (no bounce to `/login`); the DSL needs the same primitive.
export const UiWhenReload = z
  .object({
    reload: z.literal(true)
  })
  .strict()

export const UiWhenCapture = z
  .object({
    capture: z.object({
      param: z.string().min(1),
      id_as: VarName
    })
  })
  .strict()

/// Click a text item inside an overlay menu (`v-menu` content teleports to
/// `body`, where list-item roles are unreliable — the hand-written specs use
/// `getByText` for the same reason). Exact text match within `.v-overlay`.
export const UiWhenClickMenu = z
  .object({
    'click.menu': z.string().min(1)
  })
  .strict()

export const UiWhenItem = z.union([
  UiWhenNavigate,
  UiWhenClick,
  UiWhenFill,
  UiWhenSelect,
  UiWhenSubmit,
  UiWhenWait,
  UiWhenClickText,
  UiWhenClickIcon,
  UiWhenClickFirst,
  UiWhenClickSelectFirst,
  UiWhenClickSelectText,
  UiWhenClickTestId,
  UiWhenKeyboard,
  UiWhenBrowserBack,
  UiWhenUploadFiles,
  UiWhenSetAutoRename,
  UiWhenLogin,
  UiWhenIndex,
  UiWhenReload,
  UiWhenCapture,
  UiWhenClickMenu
])

export const UiAssertVisible = z
  .object({
    'ui.visible': RoleLabel
  })
  .strict()

export const UiAssertHidden = z
  .object({
    'ui.hidden': RoleLabel
  })
  .strict()

export const UiAssertText = z
  .object({
    'ui.text': RoleLabel,
    contains: z.string()
  })
  .strict()

export const UiAssertToast = z
  .object({
    'ui.toast': z.object({
      type: z.enum(['error', 'success', 'warning']),
      contains: z.string()
    })
  })
  .strict()

export const UiAssertModal = z
  .object({
    'ui.modal': z.enum(['open', 'closed'])
  })
  .strict()

export const UiAssertRoute = z
  .object({
    'ui.route': z.string()
  })
  .strict()

export const UiAssertAriaSnapshot = z
  .object({
    'ui.aria_snapshot': z.string()
  })
  .strict()

export const UiAssertApiResponse = z
  .object({
    'api.response': z
      .object({
        url: z.string(),
        status: z.union([z.number(), z.array(z.number())]),
        method: z.enum(['GET', 'POST', 'PUT', 'DELETE']).optional().default('GET'),
        headers: z.record(z.string(), z.string()).optional(),
        body: z.unknown().optional(),
        header: z
          .object({
            name: z.string().min(1),
            gt: z.number().optional(),
            equals: z.union([z.string(), z.number()]).optional()
          })
          .strict()
          .refine((value) => value.gt !== undefined || value.equals !== undefined, {
            message: 'header needs gt or equals'
          })
          .optional(),
        json: z
          .record(z.string(), z.union([z.string(), z.number(), z.boolean(), z.null()]))
          .optional(),
        json_absent: z.string().optional()
      })
      .strict()
  })
  .strict()

export const UiAssertTextVisible = z
  .object({
    'ui.text_visible': z.string()
  })
  .strict()

export const UiAssertCount = z
  .object({
    'ui.count': z.string(),
    equals: z.number().int().nonnegative()
  })
  .strict()

export const UiAssertSidebarVisible = z
  .object({
    'ui.sidebar_visible': z.string()
  })
  .strict()

export const UiAssertChipVisible = z
  .object({
    'ui.chip_visible': z.string()
  })
  .strict()

export const UiAssertInputValue = z
  .object({
    'ui.input_value': RoleLabel,
    contains: z.string()
  })
  .strict()

export const UiAssertFileContains = z
  .object({
    'file.contains': z.string(),
    text: z.string()
  })
  .strict()

export const UiAssertApiStatuses = z
  .object({
    'api.statuses': z.object({
      url: z.string(),
      equals: z.array(z.number().int())
    })
  })
  .strict()

export const UiAssertDisabled = z
  .object({
    'ui.disabled': RoleLabel
  })
  .strict()

export const UiAssertEnabled = z
  .object({
    'ui.enabled': RoleLabel
  })
  .strict()

export const UiAssertTextHidden = z
  .object({
    'ui.text_hidden': z.string()
  })
  .strict()

export const UiAssertItem = z.union([
  UiAssertVisible,
  UiAssertHidden,
  UiAssertText,
  UiAssertToast,
  UiAssertModal,
  UiAssertRoute,
  UiAssertAriaSnapshot,
  UiAssertApiResponse,
  UiAssertApiStatuses,
  UiAssertTextVisible,
  UiAssertTextHidden,
  UiAssertCount,
  UiAssertSidebarVisible,
  UiAssertChipVisible,
  UiAssertInputValue,
  UiAssertDisabled,
  UiAssertEnabled,
  UiAssertFileContains
])

export const UiStep = z
  .object({
    when: z.array(UiWhenItem).min(1),
    assert: z.array(UiAssertItem).min(1)
  })
  .strict()

export const Covers = z
  .object({
    api: z.array(z.string()).optional().default([]),
    ui: z.array(z.string()).optional().default([])
  })
  .strict()

export const UiScenario = z
  .object({
    name: z.string(),
    covers: Covers.optional().default({ api: [], ui: [] }),
    server: ServerDeclaration.optional(),
    given: z.array(GivenItem).optional().default([]),
    steps: z.array(UiStep).min(1).optional(),
    when: z.array(UiWhenItem).min(1).optional(),
    assert: z.array(UiAssertItem).min(1).optional()
  })
  .strict()
  .refine(
    (data) => {
      const hasSteps = data.steps !== undefined
      const hasFlat = data.when !== undefined && data.assert !== undefined
      return hasSteps !== hasFlat
    },
    { message: 'Specify either steps (multi-step) or when + assert (single-step), not both' }
  )

export type UiScenario = z.infer<typeof UiScenario>
export type UiWhenItem = z.infer<typeof UiWhenItem>
export type UiAssertItem = z.infer<typeof UiAssertItem>
export type UiAssertInputValue = z.infer<typeof UiAssertInputValue>
export type UiStep = z.infer<typeof UiStep>
export type GivenMove = z.infer<typeof GivenMove>
export type GivenCliUser = z.infer<typeof GivenCliUser>
export type ServerDeclaration = z.infer<typeof ServerDeclaration>
export type GivenItem = z.infer<typeof GivenItem>
export type Covers = z.infer<typeof Covers>
