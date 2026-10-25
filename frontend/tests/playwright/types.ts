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

/// Declared backend startup options. Keys mirror the real Picasu option
/// names (`AppConfig` field names in `backend/src/model/config.rs`
/// `config_tier_table`): `bootstrap_admin` = `PICASU_BOOTSTRAP_ADMIN`,
/// `login_ip_base_ban_secs` = `PICASU_LOGIN_IP_BASE_BAN_SECS`, plus
/// env-only `pbkdf_iterations` = `PICASU_PBKDF_ITERATIONS`. Absent `server:`
/// keeps the harness default (bootstrap open, 1000 iterations).
export const ServerDeclaration = z
  .object({
    bootstrap_admin: z.boolean().optional(),
    pbkdf_iterations: z.number().int().positive().optional(),
    login_ip_base_ban_secs: z.number().int().nonnegative().optional()
  })
  .strict()

export const GivenConfig = z
  .object({
    config: z.object({
      read_only_mode: z.boolean().optional(),
      auth_key: z.string().optional(),
      fs_notify_watcher: z.boolean().optional()
    })
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

export const GivenItem = z.union([
  GivenDirAlbum,
  GivenPhoto,
  GivenPhotoRaw,
  GivenSourceFile,
  GivenEmpty,
  GivenRemove,
  GivenConfig,
  GivenUser,
  GivenMove
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
  UiWhenIndex
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
    'api.response': z.object({
      url: z.string(),
      status: z.union([z.number(), z.array(z.number())]),
      method: z.enum(['GET', 'POST', 'PUT', 'DELETE']).optional().default('GET'),
      body: z.unknown().optional()
    })
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

export const UiAssertItem = z.union([
  UiAssertVisible,
  UiAssertHidden,
  UiAssertText,
  UiAssertToast,
  UiAssertModal,
  UiAssertRoute,
  UiAssertAriaSnapshot,
  UiAssertApiResponse,
  UiAssertTextVisible,
  UiAssertCount,
  UiAssertSidebarVisible,
  UiAssertChipVisible,
  UiAssertInputValue,
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
    assert: z.array(UiAssertItem).min(1).optional(),
    /// Session policy. Absent = harness-managed: the harness enrolls the
    /// resolved account (a `user:` given when present, else `admin`) and then
    /// performs a UI login before the steps. `'none'` = the scenario drives
    /// enrollment/login itself; the harness does nothing.
    session: z.enum(['none']).optional()
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
export type ServerDeclaration = z.infer<typeof ServerDeclaration>
export type GivenItem = z.infer<typeof GivenItem>
export type Covers = z.infer<typeof Covers>
