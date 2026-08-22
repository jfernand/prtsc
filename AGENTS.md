# Agent instructions for this repository

## Git policy

**Never create a commit without explicit human review and approval first.**
This applies even when a task, plan step, or prior instruction seems to
imply committing is the natural next action. Make changes, show a diff, and
wait for the human to say to commit. This also covers amending, rebasing,
pushing, and any other history-modifying operation.

## Rust practices

- Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before
  considering a change done. Fix clippy lints rather than `#[allow]`-ing them
  unless there's a specific documented reason.
- Prefer `Result<T, E>` and `?` over `.unwrap()`/`.expect()` outside of
  `main`, tests, and truly-infallible cases (e.g. a regex literal known to
  compile). Where a panic is intentional, `.expect("why")` with a real
  reason beats a bare `.unwrap()`.
- Keep `unsafe` out of this codebase unless a dependency's API leaves no
  safe alternative (e.g. some `softbuffer`/windowing FFI boundaries) — in
  that case isolate it behind a small, well-named safe wrapper function and
  comment the invariant that makes it sound.
- Match the existing module layout; don't introduce a new abstraction layer
  (traits, generics, plugin systems) for something the plan only needs once.
- No new dependency without a reason tied to an actual plan step — check
  `docs/implementation-plan.md` before adding a crate.
- Small, single-purpose commits scoped to one plan step at a time (subject
  to the git policy above — prepare the change, don't commit it).
- Doc comments (`///`) on public items only when the name and signature
  don't already make behavior obvious; skip comments that restate the code.

## Project context

See `docs/implementation-plan.md` for the current build plan and step
sequence. Follow it in order — don't jump ahead to a later step's code
before the current one is verified working.
