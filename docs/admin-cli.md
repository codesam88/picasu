# Admin CLI (`picasu-cmd`)

`picasu-cmd` is the offline administration tool for a Picasu installation.
It edits the user store directly, without the server running: provisioning
the first administrator before the gallery has ever been started, recovering
access when every admin password is lost, or removing an account that must
not exist any more.

It ships next to the server binary (`cargo build --bin picasu-cmd`).

```
picasu-cmd [--userdb <PATH>] user list|add|mod|del|passwd [<USER>]
```

## The user store

The userdb of record is one file: `<data home>/auth/users.redb`. It holds
`username ⇒ {hash?, admin, home}` — one table, one redb write transaction
per change, so every mutation is atomic and serial. Sessions live
elsewhere (the rebuildable gallery index); `user del` purges them.

Hashes are never exported. `user list` prints whether a password is set, not
the password.

## Where the CLI looks for the store

The CLI resolves the store file in this order and stops at the first hit:

| #   | Source             | Store path                                                                  |
| --- | ------------------ | --------------------------------------------------------------------------- |
| 1   | `--userdb <PATH>`  | `<PATH>` — the file itself                                                  |
| 2   | `PICASU_DATA_HOME` | `<data home>/auth/users.redb`                                               |
| 3   | `config.toml`      | `<data home>/auth/users.redb`, where the data home is `[gallery] data_home` |

`config.toml` is the file the server itself reads
(`PICASU_CONFIG_HOME` → legacy `./config.toml` in the working directory →
platform config dir). The CLI reads it without modifying it.

There is **no fallback**: with none of the three sources set, the CLI does
not know where the store is and refuses to run. It never guesses a location
and never creates a store somewhere nobody chose.

Every command reports the file it resolved, on stderr:

```
Using users.redb at /var/lib/picasu/auth/users.redb
```

or, when the file is not there:

```
Could not find users.redb at /var/lib/picasu/auth/users.redb; set location via PICASU_DATA_HOME or --userdb
```

`user list`, `user mod`, `user del` and `user passwd` need an existing
store and fail when it is missing — an empty table would claim that no users
exist. `user add` creates it, which is what provisioning the first
administrator before the server's first launch means.

### Before the server's first launch

The CLI works without a `config.toml`; that is the point of provisioning
the first admin offline. Declare the data home and the CLI and the server
agree on the store:

```sh
export PICASU_DATA_HOME=/var/lib/picasu
picasu-cmd user add admin --admin      # creates /var/lib/picasu/auth/users.redb
picasu                               # first launch: writes config.toml with that data_home
```

`PICASU_DATA_HOME` overrides the stored `data_home` on every server launch,
so keep it set for the server as well — a server started without it uses
the platform data dir instead, and the administrator the CLI created is
invisible to it.

The store file can also be named directly, without touching the data home:

```sh
picasu-cmd --userdb /var/lib/picasu/auth/users.redb user list
```

`--userdb` points at the _file_. The data home stays unknown in that case,
which matters for `user del`: the sessions live in the index database next
to the data home, so without one the CLI deletes the account and reports
that the sessions were not purged. Those sessions cannot authenticate any
more (the passwd entry is gone), so the state is harmless — but a purging
delete is cleaner.

## Commands

| Command            | Effect                                                                  |
| ------------------ | ----------------------------------------------------------------------- |
| `user list [USER]` | Table of all users, or one entry. Hashes stay hidden.                   |
| `user add USER`    | Creates a user. `--admin`, `--home`, `--password`.                      |
| `user mod USER`    | Changes attributes: `--home`, `--admin`, `--no-admin`.                  |
| `user del USER`    | Deletes the user and purges its sessions. `--force` for the last admin. |
| `user passwd USER` | Re-keys the password, keeping `admin` and `home`.                       |

`add` and `passwd` prompt for the password twice with no echo when
`--password` is absent; `--password` exists for scripts and tests.

The last administrator is protected: `user mod --no-admin` and `user del`
refuse to leave the store without one. `--force` overrides the refusal for
`user del`.

## Environment variables

| Variable                   | Effect in the CLI                                                    |
| -------------------------- | -------------------------------------------------------------------- |
| `PICASU_DATA_HOME`         | Data home; the store is `<data home>/auth/users.redb`.               |
| `PICASU_CONFIG_HOME`       | Where `config.toml` is read from.                                    |
| `PICASU_TRIVIAL_PASSWORDS` | `true` accepts weak passwords, exactly as on the server.             |
| `PICASU_PBKDF_ITERATIONS`  | PBKDF2 effort for new hashes. Test-only; never set it in production. |

`PICASU_TRIVIAL_PASSWORDS` and the `trivial_passwords` key in
`config.toml` carry the same meaning as on the server: the environment
variable wins, an unparsable value keeps the configured one, and unset
means strict. The CLI warns when the strength checks are off.

## Running against a live server

The store is locked while the server holds it open. Writes then fail with

```
picasu-cmd: error: Conflict: user store is held by another process (the server may be running); stop the server before running this offline command: /var/lib/picasu/auth/users.redb
```

Stop the server before administering the store. The server opens the store
when it first serves an authentication, so a server that has never
authenticated anyone does not hold the lock yet — treat the store as
offline-only regardless.

## Related

- [Authentication](auth.md) — the store's consistency model and the online
  equivalents of these commands.
- [Configuration](config.md) — data home, config file, environment
  overrides.
