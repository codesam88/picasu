# Contributing to Picasu

Contributions are welcome — bug reports, documentation, tests, fixes and
features alike. Picasu is pre-1.0 and has a single maintainer, so changes that
are small, focused and easy to review are the most likely to land.

How to set up a checkout, build, check and test the project is already covered
elsewhere, so it is not repeated here: see
[Development & Contributing](README.md#development--contributing) in the README
and the [documentation](docs/index.md).

## Small changes vs. larger changes

- **Small changes** — a bug fix, a typo or docs correction, a missing test, a
  narrow improvement. No prior discussion needed: follow the existing
  documentation and the conventions already in the codebase, make sure
  `just check` and `just test` pass, and open a pull request.
- **Larger changes** — new features, changes to observable behavior or
  architecture, broad refactors, new dependencies. Open a GitHub issue first
  and describe the problem or gap you perceive along with your proposed
  solution. Agreeing on the approach and the scope before implementation
  avoids duplicated or discarded work.

When in doubt, open the issue — it is cheap, and the discussion is useful even
if the change ends up taking a different shape.

## Reporting bugs and requesting features

Use the repository's issue templates. For a bug, the steps to reproduce and the
expected versus actual behavior are what make a report actionable.

## Using AI

Using AI is encouraged — have your agent review code, write tests, and propose
and question solutions. Contributions produced with AI are held to the same
standard as any other: responsibility stays with you, the contributor.

Contributions or issues that read as AI slop (not understandable, off scope, or
below the expectations outlined in this document) may be closed without further
discussion.

## Opening a pull request

Every pull request description should contain:

- **Motivation** — the problem or gap the change addresses, and why this
  approach solves it.
- **Summary** — what the change does, written so a reviewer can follow it
  without reading the whole diff.

Further expectations:

- One concern per pull request; keep refactors separate from behavior changes.
- `just check` and `just test` must pass — CI runs the same commands.
- Fixes come with a test that fails without them, and new behavior is covered
  by tests. See [docs/test-strategy.md](docs/test-strategy.md) for available
  infrastructure.
- Link the related issue, if there is one.

## Review

Review is done by the maintainer in their available time, so turnaround varies.
Expect questions and requested changes, particularly on changes touching
authentication, filesystem boundaries or the metadata pipeline.

## Security

Do not open a public issue for a security vulnerability — follow
[SECURITY.md](SECURITY.md).

## License

MIT — contributions are accepted under the project's license
([LICENSE](LICENSE)).
