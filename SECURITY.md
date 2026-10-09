# Security Policy

## Supported Versions

Picasu is pre-1.0 and has no formal releases. Fixes are applied to the latest
commit on `main` only; older checkouts receive no backports.

| Version         | Supported |
| --------------- | --------- |
| latest `main`   | ✅        |
| older checkouts | ❌        |

## Reporting a Vulnerability

Please report vulnerabilities through
[GitHub private vulnerability reporting](https://github.com/codesam88/picasu/security/advisories/new).
Reports sent there are visible only to the maintainer — not to the public issue
tracker. There is no email channel and no bounty.

Include as much as you can:

- what the vulnerability is and where it lives (route, component, or file)
- steps to reproduce, ideally a minimal case
- the impact — what an attacker gains, and against whom
- any suggested fix, if you have one

### What to expect

Picasu stores private photo libraries, so authentication, share-token scoping,
filesystem boundaries, and the image/video decoding pipeline are treated as
in-scope security surface.

You should get an acknowledgment within **7 days**. The target is a fix or a
usable mitigation within **30 days** of the initial report. The disclosure
timeline is a coordination tool, not a guarantee: a fix that needs longer stays
in private coordination rather than shipping rushed, and you are free to publish
once a fix is out or the window closes. A heads-up before public disclosure is
appreciated.

Out of scope: vulnerabilities that require physical access to the host,
social-engineering the maintainer or users, and flaws in upstream dependencies
(those belong in the dependency's own tracker, though a heads-up here is
welcome).
