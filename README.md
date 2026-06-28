# Picasu - a filesystem-first photo gallery

_**Disclaimer** Picasu is fully vibe-coded work in progress. Not ready. Use at your own risk._

Picasu is a self-hosted photo gallery based on
[urocissa](https://github.com/tedsamhain/urocissa). In contrast to most
galleries (and certainly most galleries written in a modern, memory-safe
language), Picasu treats your filesystem as the source of truth - additional
database content is purely used to accelerate lookups and will be re-generated
based on filesystem updates. This results in a few desirable features:

- Launch Picasu on an existing image repo to generate an online view with paths mapped to Albums
- Share an image repository over the network to edit or re-arrange with
  external tools[^1], Picasu will detect and adopt the changes on the fly
- Stay in control over your files, to know what you have backed up or to move on

[^1]: Metadata is (will be) kept in XMP sidecars, which is supported by many professional photo editing tools.

## Quick Start

1. Build from Source (Linux)

See [docs/linux.md](docs/linux.md).

2. Docker

```bash
git clone https://github.com/tedsamhain/picasu.git
cd picasu
docker compose up -d
```

## Configuration

`config.toml` is auto-created on first launch. Key settings:

| Env var                 | Overrides        |
| ----------------------- | ---------------- |
| `PICASU_IMAGE_HOME`     | Image root       |
| `PICASU_CONFIG_HOME`    | Config directory |
| `PICASU_DATA_HOME`      | Data directory   |
| `PICASU_PORT`           | Server port      |
| `PICASU_READ_ONLY_MODE` | Read-only mode   |

See [docs/config.md](docs/config.md) for the full reference.

## Development & Contributing

First-time setup on a fresh clone:

```bash
just setup-dev    # install tooling + enable pre-commit hook
```

`setup-dev` runs `install-dev` (cargo tools + npm deps) and enables the
`.githooks/pre-commit` hook via `git config core.hooksPath .githooks`.
The hook runs format + lint + typecheck on staged changes before every commit.
On `main` it also runs the full test suite.

- `just check` — lint + format check (backend, frontend, docs)
- `just test` — run all tests (backend nextest + frontend vitest)
- `just run` — build and launch against `sandbox/`
- `just plan` — view the project board (see `.plan/tasks/`)

Picasu aims to maximally leverage best practice development and validation
infrastructure. Since everything is vibe-coded nowadays, the strategy is to
cover all major features with end-to-end tests, as well as self-tests and
negative tests for the test-infrastructure itself. For more details check
[docs/design.md](docs/design.md) and
[docs/test-strategy.md](docs/test-strategy.md).

## License

MIT — see [LICENSE](LICENSE). Picasu is based on urocissa by [hsa00000](https://github.com/hsa00000), also MIT-licensed.
