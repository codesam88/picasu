# Configuration Guide

Urocissa uses `config.toml` for application configuration. The config file
location is determined at startup by the `UROCISSA_CONFIG_HOME` environment
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
data_home = "/home/user/.local/share/urocissa"
image_home = "/home/user/.local/share/urocissa/images"
upload_folder = "uploads"
read_only_mode = false
disable_img = false

[secrets]
# password, auth_key only present if configured
```

### `[server]` section

Server-level settings including web server and upload limits.

| Key               | Type   | Default   | Description                                                                                                        |
| ----------------- | ------ | --------- | ------------------------------------------------------------------------------------------------------------------ |
| `address`         | string | `0.0.0.0` | IP address the server binds to.                                                                                    |
| `port`            | number | `5673`    | Port the server listens on.                                                                                        |
| `max_upload_size` | string | `100MiB`  | Maximum size for a single file upload. Accepts `1GiB`, `500MiB`, etc. Sets both Rocket `file` and `data-form` limits. |

### `[gallery]` section

Gallery application settings.

| Key              | Type           | Default                    | Description                                                                                                                                                                                                                       |
| ---------------- | -------------- | -------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `data_home`      | string \| null | _resolved at first launch_ | Absolute path holding `db/`, `object/`, etc. Set automatically from `UROCISSA_DATA_HOME` or platform default. Stored so the runtime path is consistent regardless of env var changes.                                             |
| `image_home`     | string \| null | _resolved at first launch_ | Single root directory the backend watches for new or changed media files. Set from `UROCISSA_IMAGE_HOME` or defaults to `<data_home>/images`. Read-only from the frontend — configure via env var or edit `config.toml` directly. |
| `upload_folder`  | string         | `uploads`                  | Subfolder name (relative to `image_home`) that uploads with no target album land in.                                                                                                                                              |
| `read_only_mode` | boolean        | `false`                    | If `true`, the gallery runs in read-only mode — uploads, edits, and deletions are disabled.                                                                                                                                       |
| `disable_img`    | boolean        | `false`                    | Disables image processing in the frontend. Intended for debugging only.                                                                                                                                                           |

### `[secrets]` section

Sensitive authentication and notification credentials. Only present when configured.

| Key                | Type           | Default | Description                                                                                                                                                                                                                       |
| ------------------ | -------------- | ------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `password`         | string \| null | `null`  | Password required to log in to the web interface. Set via the web UI or the password endpoint.                                                                                                                                    |
| `auth_key`         | string \| null | `null`  | Secret key for signing JWT tokens. If `null`, a random key is generated on every startup (invalidates sessions on restart).                                                                                                       |

> **`data_home`** and **`image_home`** are resolved on first launch from
> environment variables or defaults and written to `config.toml`. On
> subsequent launches, `UROCISSA_DATA_HOME` and `UROCISSA_IMAGE_HOME` env
> vars still override the stored value (they are checked every startup).

### Backfilling pre-existing files

The filesystem watcher only reacts to _future_ create/modify events — it
does not scan files already sitting under `image_home` when the app starts.
After setting `image_home`, use **Scan Now** in the web UI to index what's
already there. Scan Now processes files whose content hash isn't indexed
yet — fast, safe to re-run routinely.

## Storage locations

Urocissa resolves three independent root directories, each with the same
precedence: **environment variable > legacy single-folder layout > OS-standard
directory > working directory (last resort)**.

| Root   | Env var                | Holds                             | Default when unset                                          |
| ------ | ---------------------- | --------------------------------- | ----------------------------------------------------------- |
| Config | `UROCISSA_CONFIG_HOME` | `config.toml`                     | platform config dir (e.g. `~/.config/urocissa` on Linux)    |
| Data   | `UROCISSA_DATA_HOME`   | `db/`, `object/`, `index_v5.redb` | platform data dir (e.g. `~/.local/share/urocissa` on Linux) |
| Image  | `UROCISSA_IMAGE_HOME`  | base for `image_home` if unset    | `<data dir>/images`                                         |

The platform config/data dirs come from the `directories` crate, which
already honors `$XDG_CONFIG_HOME`/`$XDG_DATA_HOME` on Linux and the platform
equivalents on Windows/macOS.

```sh
UROCISSA_CONFIG_HOME=/etc/urocissa UROCISSA_DATA_HOME=/var/lib/urocissa UROCISSA_IMAGE_HOME=/mnt/photos ./urocissa
```

**Legacy single-folder layout:** if `./config.toml`
already exists in the working directory, both the config and data roots fall
back to the working directory, so existing installs keep working unchanged.

## Environment variable overrides

The following env vars override the config file on every launch:

| Variable                     | Overrides                    |
| ---------------------------- | ---------------------------- |
| `UROCISSA_ADDRESS`           | `server.address`             |
| `UROCISSA_MAX_UPLOAD_SIZE`   | `server.max_upload_size`     |
| `UROCISSA_PORT`              | `server.port`                |
| `UROCISSA_DATA_HOME`         | `gallery.data_home`          |
| `UROCISSA_DISABLE_IMG`       | `gallery.disable_img`        |
| `UROCISSA_IMAGE_HOME`        | `gallery.image_home`         |
| `UROCISSA_READ_ONLY_MODE`    | `gallery.read_only_mode`     |
| `UROCISSA_UPLOAD_FOLDER`     | `gallery.upload_folder`      |
| `UROCISSA_AUTH_KEY`          | `secrets.auth_key`         |

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
