<!--
Thanks for contributing to Pulse! See CONTRIBUTING.md.
The PR title becomes one line of the in-app release notes, which are in Spanish.
If you don't write Spanish, write the title in English and the maintainer will adapt it on merge.
-->

## Summary

<!-- What changes, in one or two sentences, from the point of view of someone using Pulse. -->

## Motivation

<!-- Why. Link the issue: "Closes #123". -->

## Changes

-

## How it was tested

<!-- Commands you ran and what you checked by ear or by eye. Tests run in release: `cargo test --release`. -->

- [ ] `cargo test --release -p <crate>` for every crate touched (and `-p app` if the engine changed)
- [ ] `cargo check --release --target x86_64-pc-windows-msvc --workspace --all-targets` (if you have the target installed)

### Platforms verified

- [ ] macOS (Apple Silicon)
- [ ] Windows x64 (native machine, not only CI)
- [ ] Only through CI

## Possible regressions

<!-- What could this break? Projects saved by older versions, plugins, performance, sound... -->

## Screenshots

<!-- Required for UI changes: before / after. -->

## Real-time audio thread

<!--
⚠️ If this PR touches code that runs on the audio thread (crates/engine: Engine::process,
instruments, effects, plugin processing, workers.rs), it must not allocate, lock, do I/O
or wait for the OS there. Explain how you checked, e.g.
`cargo test --release -p engine --test stress`, and the buffer-size / thread-count tests.
Delete this section if it doesn't apply.
-->

- [ ] This PR does not touch the audio thread
- [ ] It does, and it allocates nothing, takes no lock and the stress tests pass

## AI use

<!-- See "AI-assisted contributions" in CONTRIBUTING.md. One line is enough. -->

- [ ] No AI tools used
- [ ] AI tools used for: <!-- e.g. translation, a first draft of the tests -->
- [ ] I have read every line of this change and will answer the review myself

## Checklist

- [ ] Commits are signed off (`git commit -s`, see CONTRIBUTING.md)
- [ ] `docs/guia.md` updated if something visible or usable in the app changed
- [ ] `CHANGELOG.md` updated under **Unreleased** (if user-facing)
- [ ] `python3 scripts/notices.py` run if dependencies changed
- [ ] No `cargo fmt` over the whole repository (the code keeps its hand style)
