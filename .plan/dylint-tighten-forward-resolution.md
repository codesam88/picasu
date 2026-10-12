---
status: backlog
type: chore
priority: low
area: backend
---

## Problem

`AUTH3_NO_FORWARD_DENIAL`'s resolution arm fires on any variant of an
enum whose def path runs through a `request` module — including
`Success`/`Error`, not just `Forward`. Harmless today (Rocket defines
`Outcome` at `rocket::outcome::Outcome`, verified in registry source,
so the arm never fires in production and real findings come from the
spelling arm), but a hand-written `request::Outcome`-shaped enum would
warn spuriously. Found by the lint's own UI fixtures during step 7.

## Goal

Tighten the arm to `Forward` variants only, or document why the broad
shape stays. The `fail_forward.rs` fixture pins current behavior
either way.

## Steps

1. Narrow the resolution to the `Forward` variant (ctor path ends in
   `Forward`) or record the deliberate choice in the folder README.
2. `cargo test` in the lint crate green; `just dylint-check`
   unchanged.
