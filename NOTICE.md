# Mihoto Fork Notice

Mihoto is a fork of [spencerwooo/mihoro](https://github.com/spencerwooo/mihoro),
the Rust-based Mihomo CLI client for Linux.

The fork keeps the upstream Git history and the original MIT license text in
`LICENSE`. The original copyright notice remains:

```text
Copyright (c) 2023 Spencer (Shangbo Wu)
```

## Fork Baseline

- Upstream repository: `spencerwooo/mihoro`
- Fork repository: `Pectics/mihoto`
- Upstream baseline tag: `mihoro-v0.14.0-base`
- Baseline commit: `e31827ed257e7ce97217cd0a2dbcd1ef96dbac7f`
- Baseline upstream release: `v0.14.0`

The `mihoro-v0.14.0-base` tag marks the upstream release baseline used to start
Mihoto governance work. It is intended to be immutable.

## Attribution Policy

- Preserve upstream Git history whenever possible.
- Preserve upstream authorship when porting commits.
- Prefer `git cherry-pick -x` when taking code from upstream commits.
- Reference upstream issues or pull requests in Mihoto issues and PRs when they
  motivate a change.
- Keep mechanical rename work separate from behavior changes and architecture
  changes.
