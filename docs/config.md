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
image_home = "/home/user/.local/share/picasu/images"
upload_folder = "uploads"
read_only_mode = false
disable_img = false

[secrets]
# password, auth_key only present if configured
```

### `[server]` section

Server-level settings including web server and upload limits.

| Key                             | Type    | Default   | Description                                                                                                                                                                                                                                                                                                                                                                      |
| ------------------------------- | ------- | --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `address`                       | string  | `0.0.0.0` | IP address the server binds to.                                                                                                                                                                                                                                                                                                                                                  |
| `port`                          | number  | `5673`    | Port the server listens on.                                                                                                                                                                                                                                                                                                                                                      |
| `max_upload_size`               | string  | `100MiB`  | Maximum size for a single file upload. Accepts `1GiB`, `500MiB`, etc. Sets both Rocket `file` and `data-form` limits.                                                                                                                                                                                                                                                            |
| `trust_proxy_forwarded_for`     | boolean | `false`   | Trust the leftmost `X-Forwarded-For` entry as the client IP (for login throttling + failure logs). Leave off on direct LAN deploys — the header is spoofable. Turn on behind a TLS-terminating reverse proxy that sets it. When off/unparseable, the socket address is used (`unknown` when absent). IPv6 addresses key by /64 prefix (privacy extensions rotate interface IDs). |
| `login_ip_window_secs`          | number  | `600`     | Sliding window (seconds, 10 min) for the IP-short burst bucket.                                                                                                                                                                                                                                                                                                                  |
| `login_ip_max_failures`         | number  | `3`       | Failed logins per `login_ip_window_secs` from one IP before the IP-short ban starts (i.e. the 4th failure bans).                                                                                                                                                                                                                                                                 |
| `login_ip_base_ban_secs`        | number  | `10`      | First IP-short ban (seconds); doubles per two further failures up to `login_ip_max_ban_secs`.                                                                                                                                                                                                                                                                                    |
| `login_ip_max_ban_secs`         | number  | `28800`   | IP-short ban cap (seconds, 8h).                                                                                                                                                                                                                                                                                                                                                  |
| `login_ip_long_window_secs`     | number  | `86400`   | Sliding window (seconds, 24h) for the IP-long daily bucket.                                                                                                                                                                                                                                                                                                                      |
| `login_ip_long_max_failures`    | number  | `16`      | Failed logins per `login_ip_long_window_secs` from one IP before the IP-long ban starts (i.e. the 17th failure bans).                                                                                                                                                                                                                                                            |
| `login_ip_long_base_ban_secs`   | number  | `1800`    | First IP-long ban (seconds, 30 min); doubles per two further failures up to `login_ip_long_max_ban_secs`.                                                                                                                                                                                                                                                                        |
| `login_ip_long_max_ban_secs`    | number  | `28800`   | IP-long ban cap (seconds, 8h).                                                                                                                                                                                                                                                                                                                                                   |
| `login_user_long_window_secs`   | number  | `86400`   | Sliding window (seconds, 24h) for the user-long daily bucket.                                                                                                                                                                                                                                                                                                                    |
| `login_user_long_max_failures`  | number  | `18`      | Failed logins per `login_user_long_window_secs` for one claimed username before the user-long ban starts (i.e. the 19th failure bans), whether or not the account exists.                                                                                                                                                                                                        |
| `login_user_long_base_ban_secs` | number  | `1800`    | First user-long ban (seconds, 30 min); doubles per two further failures up to `login_user_long_max_ban_secs`.                                                                                                                                                                                                                                                                    |
| `login_user_long_max_ban_secs`  | number  | `7200`    | User-long ban cap (seconds, 2h).                                                                                                                                                                                                                                                                                                                                                 |

#### Login throttling

`POST /post/users/authenticate` checks three buckets before verifying the
password (throttled requests skip the KDF and answer `429` with a
`Retry-After` header: the max remaining ban across all three). Two
evaluated over-threshold failures share each ban generation (both ban),
with `ban(n) = min(base · 2^n, cap)`; a successful login drops the
username entry and clears the IP burst bucket, while the IP-long history
is kept so one login cannot wipe a shared address's daily budget. The per-user throttling is
implemented on claimed usernames, regardless whether or not the account
exists. This prevents information leakage and further helps to slow down
blind/spraying attacks.

Default throttling configuration:

- IP-short: 3 attempts in 10min, iterative 2^n * 10s ban, capped at 8h
- IP-long: 16 attempts in 24h, iterative 2^n * 30min ban, capped at 8h
- User: 18 attempts in 24h, iterative 2^n * 30min ban, capped at 2h

Effect and purpose:

- IP-short causes initial small delays: seconds-long bans that graduate
  before the 30-minute hammer, softening the blow for legitimate owners
  (a 4th mistype costs 10s, not 30min)
- IP-long puts a strong throttle on longer-term campaigns based out of few IP
  addresses. 8h maximum ban time, maximum 16 attempts/day for paced attack
- User-long is last resort against longer-term botnet attacks. A distributed
  attack using many IPs will bypass the IP thresholds and run into this limit
  for individual probed accounts. The policy has slightly higher initial trigger
  so that any IP-based filter will trigger first. Additionally, the maximum ban
  time has a lower cap so that legitimate users will be able to login again
  soon after a sustained attack (2h max ban time).
- Entries are evicted once past both the ban snapshot and last-seen +
  longest window (24h); reaped opportunistically during login (at most
  once per 60s), with per-map caps as backstop.
- Behind a reverse proxy, enable `trust_proxy_forwarded_for`: otherwise
  every client shares the proxy's socket address in a single IP bucket,
  and a few failures can throttle all users at once.

### `[gallery]` section

Gallery application settings.

| Key              | Type           | Default                    | Description                                                                                                                                                                                                                     |
| ---------------- | -------------- | -------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `data_home`      | string \| null | _resolved at first launch_ | Absolute path holding `db/`, `object/`, etc. Set automatically from `PICASU_DATA_HOME` or platform default. Stored so the runtime path is consistent regardless of env var changes.                                             |
| `image_home`     | string \| null | _resolved at first launch_ | Single root directory the backend watches for new or changed media files. Set from `PICASU_IMAGE_HOME` or defaults to `<data_home>/images`. Read-only from the frontend — configure via env var or edit `config.toml` directly. |
| `upload_folder`  | string         | `uploads`                  | Subfolder name (relative to `image_home`) that uploads with no target album land in.                                                                                                                                            |
| `read_only_mode` | boolean        | `false`                    | If `true`, the gallery runs in read-only mode — uploads, edits, and deletions are disabled.                                                                                                                                     |
| `disable_img`    | boolean        | `false`                    | Disables image processing in the frontend. Intended for debugging only.                                                                                                                                                         |

### `[secrets]` section

Holds no live keys: the signing key moved to the owner-only secrets file
below. A legacy `auth_key` left in an old file is ignored, never read back;
a legacy `password` is ignored (the boot path warns).

> `[secrets] password` was removed: there is no password field anymore,
> nothing reads it, and it is never written back. Old files containing it
> still parse — the key is ignored (the boot path warns). Passwords now live
> per-user in the password store; use `PUT /put/users/password`
> (`{userId, oldPassword, newPassword}` for a self-change, `{userId,
newPassword}` for an admin reset of another user).

> **`data_home`** and **`image_home`** are resolved on first launch from
> environment variables or defaults and written to `config.toml`. On
> subsequent launches, `PICASU_DATA_HOME` and `PICASU_IMAGE_HOME` env
> vars still override the stored value (they are checked every startup).

### Backfilling pre-existing files

The filesystem watcher only reacts to _future_ create/modify events — it
does not scan files already sitting under `image_home` when the app starts.
After setting `image_home`, use **Scan Now** in the web UI to index what's
already there. Scan Now re-walks the tree and indexes every media file it
finds — identity is by path, and a content hash is computed per file only
for duplicate grouping. Safe to re-run routinely.

## Storage locations

Picasu resolves three independent root directories, each with the same
precedence: **environment variable > legacy single-folder layout > OS-standard
directory > working directory (last resort)**.

| Root   | Env var              | Holds                             | Default when unset                                        |
| ------ | -------------------- | --------------------------------- | --------------------------------------------------------- |
| Config | `PICASU_CONFIG_HOME` | `config.toml`                     | platform config dir (e.g. `~/.config/picasu` on Linux)    |
| Data   | `PICASU_DATA_HOME`   | `db/`, `object/`, `index_v5.redb` | platform data dir (e.g. `~/.local/share/picasu` on Linux) |
| Image  | `PICASU_IMAGE_HOME`  | base for `image_home` if unset    | `<data dir>/images`                                       |

The platform config/data dirs come from the `directories` crate, which
already honors `$XDG_CONFIG_HOME`/`$XDG_DATA_HOME` on Linux and the platform
equivalents on Windows/macOS.

```sh
PICASU_CONFIG_HOME=/etc/picasu PICASU_DATA_HOME=/var/lib/picasu PICASU_IMAGE_HOME=/mnt/photos ./picasu
```

**Legacy single-folder layout:** if `./config.toml`
already exists in the working directory, both the config and data roots fall
back to the working directory, so existing installs keep working unchanged.

**The offline admin CLI** resolves the user store the same way, with one
addition: `--userdb <PATH>` names the store file directly. It has no
fallback — with neither `--userdb` nor `PICASU_DATA_HOME` set and no
`config.toml` to read, it refuses to run instead of guessing a location.
See [Admin CLI](admin-cli.md).

## Environment variable overrides

The following env vars override the config file on every launch:

| Variable                               | Overrides                                                             |
| -------------------------------------- | --------------------------------------------------------------------- |
| `PICASU_ADDRESS`                       | `server.address`                                                      |
| `PICASU_ALLOW_REGISTRATION`            | `gallery.allow_registration`                                          |
| `PICASU_MAX_UPLOAD_SIZE`               | `server.max_upload_size`                                              |
| `PICASU_PORT`                          | `server.port`                                                         |
| `PICASU_BOOTSTRAP_ADMIN`               | `gallery.bootstrap_admin`                                             |
| `PICASU_DATA_HOME`                     | `gallery.data_home`                                                   |
| `PICASU_DISABLE_IMG`                   | `gallery.disable_img`                                                 |
| `PICASU_FS_NOTIFY_WATCHER`             | `gallery.fs_notify_watcher`                                           |
| `PICASU_IMAGE_HOME`                    | `gallery.image_home`                                                  |
| `PICASU_READ_ONLY_MODE`                | `gallery.read_only_mode`                                              |
| `PICASU_TRIVIAL_PASSWORDS`             | `gallery.trivial_passwords`                                           |
| `PICASU_UPLOAD_FOLDER`                 | `gallery.upload_folder`                                               |
| `PICASU_AUTH_KEY`                      | signing key (in-memory override of the secrets file, never persisted) |
| `PICASU_TRUST_PROXY_FORWARDED_FOR`     | `server.trust_proxy_forwarded_for`                                    |
| `PICASU_LOGIN_IP_WINDOW_SECS`          | `server.login_ip_window_secs`                                         |
| `PICASU_LOGIN_IP_MAX_FAILURES`         | `server.login_ip_max_failures`                                        |
| `PICASU_LOGIN_IP_BASE_BAN_SECS`        | `server.login_ip_base_ban_secs`                                       |
| `PICASU_LOGIN_IP_MAX_BAN_SECS`         | `server.login_ip_max_ban_secs`                                        |
| `PICASU_LOGIN_IP_LONG_WINDOW_SECS`     | `server.login_ip_long_window_secs`                                    |
| `PICASU_LOGIN_IP_LONG_MAX_FAILURES`    | `server.login_ip_long_max_failures`                                   |
| `PICASU_LOGIN_IP_LONG_BASE_BAN_SECS`   | `server.login_ip_long_base_ban_secs`                                  |
| `PICASU_LOGIN_IP_LONG_MAX_BAN_SECS`    | `server.login_ip_long_max_ban_secs`                                   |
| `PICASU_LOGIN_USER_LONG_WINDOW_SECS`   | `server.login_user_long_window_secs`                                  |
| `PICASU_LOGIN_USER_LONG_MAX_FAILURES`  | `server.login_user_long_max_failures`                                 |
| `PICASU_LOGIN_USER_LONG_BASE_BAN_SECS` | `server.login_user_long_base_ban_secs`                                |
| `PICASU_LOGIN_USER_LONG_MAX_BAN_SECS`  | `server.login_user_long_max_ban_secs`                                 |
| `PICASU_RATE_LIMITS`                   | env-only kill switch (exact `false` disables all login throttling)    |

## Runtime changes via the API

`PUT /put/config` accepts only runtime-safe knobs. Host-only knobs are
rejected with 400 when present in the body — set them via the flag, env
var, or `config.toml` key listed, then restart:

| Knob                         | Set via                                                                           |
| ---------------------------- | --------------------------------------------------------------------------------- |
| `server.address`             | `PICASU_ADDRESS` or `server.address` in `config.toml`                             |
| `server.port`                | `PICASU_PORT` or `server.port` in `config.toml`                                   |
| `gallery.allow_registration` | `PICASU_ALLOW_REGISTRATION` or `gallery.allow_registration` in `config.toml`      |
| `gallery.trivial_passwords`  | `--trivial-passwords`, `PICASU_TRIVIAL_PASSWORDS`, or `gallery.trivial_passwords` |
| `gallery.bootstrap_admin`    | `--bootstrap-admin`, `PICASU_BOOTSTRAP_ADMIN`, or `gallery.bootstrap_admin`       |
| `gallery.data_home`          | `PICASU_DATA_HOME` (not accepted by the API at all)                               |
| `gallery.image_home`         | `PICASU_IMAGE_HOME` (not accepted by the API at all)                              |

The JWT signing key is managed, never set: it lives in
`<DATA_HOME>/auth/secrets.json` (owner-only file permissions), `PICASU_AUTH_KEY`
overrides it in memory without persisting, and the API offers only
`POST /post/config/auth-key/reset`, which generates a new strong random key
(invalidating every outstanding token). `GET /get/config/export` redacts it.

API-writable knobs: `upload_folder`, `max_upload_size`, `read_only_mode`,
`disable_img`, `fs_notify_watcher`, `normalize_upload_filenames`,
`validate_upload_content`, and `use_client_timestamp_info`.

Three settings are environment-only by construction: `PICASU_CONFIG_HOME`
(selects where `config.toml` itself lives, so it cannot come from the
file or the API), `PICASU_PBKDF_ITERATIONS` (test-only password-hashing
effort — never set in production, accepted from neither the file nor the
API), and `PICASU_RATE_LIMITS` (exact `false` bypasses all login
throttling for schemathesis fuzzing; failure logging stays on).

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
>
> Deploy HTTPS for anything beyond a trusted LAN: the backend itself
> serves plain HTTP and the login stores the 14-day session JWT in a
> cookie whose `Secure` flag engages only when the proxy forwards
> `X-Forwarded-Proto: https` — over plain HTTP, every bearer of the
> token can replay the session until it expires. A TLS-terminating proxy
> must forward that header (and set `X-Forwarded-For`, with
> `trust_proxy_forwarded_for` enabled, for login throttling to key by
> client IP).
