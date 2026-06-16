# Configuration Guide

Urocissa uses `config.json` for application configuration. This file is located in the `gallery-backend` directory.

If `config.json` does not exist, it will be automatically created with default values when the application starts for the first time.

## Configuration Structure

The configuration is divided into `public` and `private` sections.

```json
{
  "public": {
    "address": "0.0.0.0",
    "port": 5673,
    "limits": {
      "file": "10GiB",
      "json": "10MiB",
      "data-form": "10GiB"
    },
    "imagePath": null,
    "uploadFolder": "uploads",
    "readOnlyMode": false,
    "disableImg": false
  },
  "private": {
    "password": null,
    "authKey": null,
    "discordHookUrl": null
  }
}
```

## Public Settings

These settings control the server's public-facing behavior.

| Setting            | Type           | Default      | Description                                                                                                                                                          |
| ------------------ | -------------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `address`          | string         | `"0.0.0.0"` | The IP address the server binds to. `"0.0.0.0"` means it listens on all available network interfaces.                                                               |
| `port`             | number         | `5673`       | The port number the server listens on.                                                                                                                               |
| `limits.file`      | string         | `"10GiB"`    | Maximum size for a single file upload. Accepts human-readable sizes: `"1GiB"`, `"512MiB"`, etc.                                                                     |
| `limits.json`      | string         | `"10MiB"`    | Maximum size for JSON request bodies.                                                                                                                                |
| `limits.data-form` | string         | `"10GiB"`    | Maximum size for multipart form submissions (used for photo/video import).                                                                                           |
| `imagePath`        | string \| null | `null`       | Single root directory to watch for new or changed media files. Example: `"/mnt/photos"`. If relative, resolved against `UROCISSA_IMAGE_HOME`. Aggregate multiple physical libraries under this one root at the filesystem level (bind mounts/symlinks) rather than configuring several paths — the OS already handles that. |
| `uploadFolder`     | string         | `"uploads"`  | Subfolder name (relative to `imagePath`) that uploads with no target album land in. Becomes its own top-level album automatically (album = directory). Uploads *with* a target album write directly into that album's real directory instead. |
| `readOnlyMode`     | boolean        | `false`      | If `true`, the gallery runs in read-only mode — uploads, edits, and deletions are disabled.                                                                          |
| `disableImg`       | boolean        | `false`      | If `true`, disables image processing in the frontend. **Intended for debugging only; do not use in production.**                                                     |

**Backfilling pre-existing files:** the filesystem watcher only reacts to
*future* create/modify events — it does not scan files already sitting
under `imagePath` when the app starts (e.g. a volume populated before first
run). After setting `imagePath`, use **Settings → One-Time Import → Scan
Image Path** to index what's already there; unlike importing an arbitrary
external folder, this always targets the configured `imagePath` itself, so
album hierarchy is built from the directory structure correctly.

## Private Settings

These settings handle sensitive security and authentication data.

| Setting          | Type           | Default | Description                                                                                                                                                                                                                                                                                                                                     |
| ---------------- | -------------- | ------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `password`       | string \| null | `null`  | The password required to log in to the web interface. If `null`, no password is set and you will need to configure one.                                                                                                                                                                                                                         |
| `authKey`        | string \| null | `null`  | The secret key used for signing authentication tokens (JWT). <br> - If `null`, a random key is generated on every startup, which invalidates existing login sessions upon restart.<br> - Set this to a random string to persist sessions across server restarts.<br> **If you are unsure what this does, keeping it as `null` is recommended.** |
| `discordHookUrl` | string \| null | `null`  | Optional Discord Webhook URL for receiving error notifications.                                                                                                                                                                                                                                                                                 |

## Storage locations

Urocissa resolves three independent root directories, each with the same
precedence: **environment variable > legacy single-folder layout > OS-standard
directory > working directory (last resort)**.

| Root | Env var | Holds | Default when unset |
|---|---|---|---|
| Config | `UROCISSA_CONFIG_HOME` | `config.json` | platform config dir (e.g. `~/.config/urocissa` on Linux) |
| Data | `UROCISSA_DATA_HOME` | `db/`, `object/`, `upload/` | platform data dir (e.g. `~/.local/share/urocissa` on Linux) |
| Image | `UROCISSA_IMAGE_HOME` | base for resolving a relative `imagePath` | `<data dir>/images` |

The platform config/data dirs come from the `directories` crate, which
already honors `$XDG_CONFIG_HOME`/`$XDG_DATA_HOME` on Linux and the platform
equivalents on Windows/macOS — no separate XDG handling needed.

`UROCISSA_IMAGE_HOME`'s default is deliberately *not* the working
directory: the cwd is arbitrary depending on how the binary was launched
(systemd unit, Docker `WORKDIR`, a desktop shortcut), so it isn't a reliable
place to expect media to already exist. Defaulting to a subdirectory of the
already-resolved, stable data dir means a fresh install has a discoverable,
predictable place to drop files into with zero configuration.

```sh
UROCISSA_CONFIG_HOME=/etc/urocissa UROCISSA_DATA_HOME=/var/lib/urocissa UROCISSA_IMAGE_HOME=/mnt/photos ./urocissa
```

**Legacy single-folder layout:** if `./config.json` already exists in the
working directory (true for any install that pre-dates the config/data
split), both the config and data roots fall back to the working directory,
so existing installs keep working unchanged without needing to set
anything.

**Single root only:** `imagePath` and `UROCISSA_IMAGE_HOME` each take exactly
one directory. Aggregate multiple physical photo/video libraries under that
one root at the filesystem level (bind mounts or symlinks) rather than
configuring a list — the OS already handles that well.

`just run` uses `UROCISSA_CONFIG_HOME`/`UROCISSA_DATA_HOME` to point a
manually-launched dev instance at a throwaway sandbox directory
(`sandbox/data`) instead of a real install's data, and
`UROCISSA_IMAGE_HOME=sandbox/images` as a place to drop test galleries.

The Docker image (`Dockerfile` at repo root) sets these to fixed in-image
paths (`/config`, `/data`, `/images`); `compose.yaml` bind-mounts host
directories onto them. See the README's Docker quick-setup section.

> **Note:** the data directory holds real, back-up-worthy data, not disposable
> cache — `db/index_v5.redb` is the only store of record for tags/album
> assignments/flags. `IMAGE_HOME` holds the originals themselves (never
> duplicated into `DATA_HOME`); `object/compressed/` here is only a
> regenerable thumbnail/preview cache. A handful of files (`cache_db.redb`,
> `temp_db.redb`, `expire_db.redb`) *are* safely disposable; splitting those
> into a dedicated state directory is a possible future change, not yet
> done (see `TODO.md`).
>
> If you're upgrading from before this was fixed, any pre-existing
> `object/imported/` directory under your old data path is now unused —
> safe to delete manually, nothing reads or writes it anymore.

## Advanced: Rocket Web Server Configuration

`config.json` covers the settings most users need to change. Internally, these are merged into the configuration of [Rocket](https://rocket.rs), the web framework Urocissa uses.

Rocket has additional options that are **not** exposed in `config.json` — such as TLS, worker thread count, connection keep-alive, and reverse-proxy header trust. You can set these without modifying the application using either of the two standard Rocket mechanisms, both of which work alongside `config.json`.

### Option 1: `Rocket.toml`

Place a `Rocket.toml` file in the `gallery-backend` directory. Settings here are merged with (and can override) the values from `config.json`.

```toml
[default]
workers = 8          # async worker threads (default: logical CPUs × 2)
keep_alive = 30      # connection keep-alive in seconds
```

> **HTTPS:** The standard build does not include Rocket's built-in TLS. The recommended approach for most deployments is to terminate TLS at a reverse proxy (nginx, Caddy) in front of Urocissa. Caddy handles certificate provisioning and renewal automatically.

### Option 2: Environment variables

Any Rocket option can also be set via a `ROCKET_` prefixed environment variable, which is convenient for containers and systemd units:

```sh
ROCKET_WORKERS=8
ROCKET_KEEP_ALIVE=30
ROCKET_ADDRESS=127.0.0.1  # overrides config.json
ROCKET_PORT=5673           # overrides config.json
```

> **Note:** `ROCKET_ADDRESS` and `ROCKET_PORT` will override the values in `config.json`. Set them in one place only to avoid confusion.

For the full list of available Rocket options see the [Rocket configuration guide](https://rocket.rs/guide/configuration/).
