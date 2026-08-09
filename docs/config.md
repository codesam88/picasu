# Configuration Guide

Picasu uses `config.toml` for application configuration. The config file
location is determined at startup by the `PICASU_CONFIG_HOME` environment
variable or the platform default (see [Storage locations](#storage-locations)).

If `config.toml` does not exist, it is automatically created with defaults
when the application starts for the first time.

## Configuration File

```toml
[server]
address = "0.0.0.0"
port = 5673
max_upload_size = "100MiB"

[gallery]
data_home = "/home/user/.local/share/picasu"
upload_folder = "uploads"
read_only_mode = false
disable_img = false
trash_enabled = true

[[gallery.namespaces]]
name = "shared"
path = "/home/user/.local/share/picasu/images"

[[gallery.namespaces]]
name = "trash"
path = "/home/user/.local/share/picasu/trash"

[secrets]
# password, auth_key only present if configured
```

### `[server]` section

Server-level settings including web server and upload limits.

| Key               | Type   | Default   | Description                                                                                                           |
| ----------------- | ------ | --------- | --------------------------------------------------------------------------------------------------------------------- |
| `address`         | string | `0.0.0.0` | IP address the server binds to.                                                                                       |
| `port`            | number | `5673`    | Port the server listens on.                                                                                           |
| `max_upload_size` | string | `100MiB`  | Maximum size for a single file upload. Accepts `1GiB`, `500MiB`, etc. Sets both Rocket `file` and `data-form` limits. |

### `[gallery]` section

Gallery application settings.

| Key                          | Type           | Default                    | Description                                                                                                                                     |
| ---------------------------- | -------------- | -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| `data_home`                  | string \| null | _resolved at first launch_ | Absolute path holding `db/`, `object/`, etc. Set automatically from `PICASU_DATA_HOME` or platform default.                                     |
| `upload_folder`              | string         | `uploads`                  | Subfolder name (relative to namespace roots) that uploads with no target album land in.                                                         |
| `read_only_mode`             | boolean        | `false`                    | If `true`, the gallery runs in read-only mode — uploads, edits, and deletions are disabled.                                                     |
| `disable_img`                | boolean        | `false`                    | Disables image processing in the frontend. Intended for debugging only.                                                                         |
| `trash_enabled`              | boolean        | `true`                     | When `true`, a namespace named `trash` must be configured. Deleted files are moved to the trash namespace instead of being permanently removed. |
| `fs_notify_watcher`          | boolean        | `true`                     | Enable the filesystem watcher for real-time indexing of changes.                                                                                |
| `normalize_upload_filenames` | boolean        | `true`                     | NFC-normalize uploaded filenames so macOS NFD names collapse onto their composed form.                                                          |
| `validate_upload_content`    | boolean        | `true`                     | Cross-check uploads against the declared `Content-Type` using magic-byte detection.                                                             |
| `use_client_timestamp_info`  | boolean        | `false`                    | Trust the `lastModified` the upload client sends with each file.                                                                                |

### `[[gallery.namespaces]]` entries

Each namespace defines an independent filesystem root for media storage.
At least one namespace must be configured. Names must be unique.

| Key    | Type   | Description                                                                                                      |
| ------ | ------ | ---------------------------------------------------------------------------------------------------------------- |
| `name` | string | Namespace identifier. `"shared"` is the default for uploads and the main library. `"trash"` holds deleted files. |
| `path` | string | Absolute path to the namespace root directory. Must be on the same filesystem as all other namespace roots.      |

**Validation rules:**

- At least one namespace is required.
- Namespace names must be unique.
- When `trash_enabled = true`, a namespace named `"trash"` must exist.
- All namespace roots must be on the same filesystem (for `fs::rename` to work across namespaces).

### `[secrets]` section

Sensitive authentication and notification credentials. Only present when configured.

| Key        | Type           | Default | Description                                                                                                                 |
| ---------- | -------------- | ------- | --------------------------------------------------------------------------------------------------------------------------- |
| `password` | string \| null | `null`  | Password required to log in to the web interface. Set via the web UI or the password endpoint.                              |
| `auth_key` | string \| null | `null`  | Secret key for signing JWT tokens. If `null`, a random key is generated on every startup (invalidates sessions on restart). |

> **`data_home`** is resolved on first launch from the `PICASU_DATA_HOME`
> environment variable or the platform default and written to `config.toml`.
> On subsequent launches, `PICASU_DATA_HOME` still overrides the stored value.

### Backfilling pre-existing files

The filesystem watcher only reacts to _future_ create/modify events — it
does not scan files already sitting under namespace roots when the app starts.
Use **Scan Now** in the web UI to index what's already there. Scan Now
processes files whose content hash isn't indexed yet — fast, safe to re-run
routinely.

## Storage locations

Picasu resolves two independent root directories, each with the same
precedence: **environment variable > legacy single-folder layout > OS-standard
directory > working directory (last resort)**.

| Root   | Env var              | Holds                             | Default when unset                                        |
| ------ | -------------------- | --------------------------------- | --------------------------------------------------------- |
| Config | `PICASU_CONFIG_HOME` | `config.toml`                     | platform config dir (e.g. `~/.config/picasu` on Linux)    |
| Data   | `PICASU_DATA_HOME`   | `db/`, `object/`, `index_v5.redb` | platform data dir (e.g. `~/.local/share/picasu` on Linux) |

Namespace roots (media storage) are configured explicitly via
`[[gallery.namespaces]]` entries in `config.toml`.

The platform config/data dirs come from the `directories` crate, which
already honors `$XDG_CONFIG_HOME`/`$XDG_DATA_HOME` on Linux and the platform
equivalents on Windows/macOS.

```sh
PICASU_CONFIG_HOME=/etc/picasu PICASU_DATA_HOME=/var/lib/picasu ./picasu
```

**Legacy single-folder layout:** if `./config.toml`
already exists in the working directory, both the config and data roots fall
back to the working directory, so existing installs keep working unchanged.

## Environment variable overrides

The following env vars override the config file on every launch:

| Variable                 | Overrides                |
| ------------------------ | ------------------------ |
| `PICASU_ADDRESS`         | `server.address`         |
| `PICASU_MAX_UPLOAD_SIZE` | `server.max_upload_size` |
| `PICASU_PORT`            | `server.port`            |
| `PICASU_DATA_HOME`       | `gallery.data_home`      |
| `PICASU_DISABLE_IMG`     | `gallery.disable_img`    |
| `PICASU_READ_ONLY_MODE`  | `gallery.read_only_mode` |
| `PICASU_UPLOAD_FOLDER`   | `gallery.upload_folder`  |
| `PICASU_AUTH_KEY`        | `secrets.auth_key`       |

## Advanced: Rocket configuration

Rocket has additional options not exposed in this config — TLS, worker count,
keep-alive, reverse-proxy headers. Set these via `Rocket.toml` or `ROCKET_*`
env vars. See the [Rocket configuration guide](https://rocket.rs/guide/configuration/).

```toml
# Rocket.toml (searched from cwd upward; or use ROCKET_CONFIG env var)
[default]
workers = 8
keep_alive = 30
```

> HTTPS is best handled at a reverse proxy (nginx, Caddy) rather than
> through Rocket's built-in TLS.
