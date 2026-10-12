---
status: open
type: bug
priority: low
area: backend
---

## Problem

`POST /post/create_dir_album` answers 500 (`Failed to create
directory: File exists`) when the sanitized name collides with an
existing directory. Found by the Schemathesis pilot (F1). An existing
directory is a client-side conflict, not a server fault; the spec
claims 400/401/405/500, so 500 is "documented" but wrong in spirit.

## Goal

Name collision is rejected as 400/409 with a JSON body; 500 reserved
for genuine storage failures.

## Steps

1. Pin the behavior (colliding name → 400/409 JSON).
2. Fix at the directory-creation path.
3. Re-run new tests + full backend suite; update spec annotations.
