# Fork Baseline and CI Policy

Mihoto starts as a safety-focused fork of
[spencerwooo/mihoro](https://github.com/spencerwooo/mihoro). This document fixes
the fork provenance, naming boundary, and CI expectations for early Mihoto work.

## Provenance

- `origin` is `Pectics/mihoto`.
- `upstream` is `spencerwooo/mihoro`.
- `mihoro-v0.14.0-base` marks upstream `v0.14.0` at commit
  `e31827ed257e7ce97217cd0a2dbcd1ef96dbac7f`.
- The baseline tag is intended to be immutable. Do not move or force-update it.
- Ported upstream code should preserve authorship and include upstream references
  in issues, commits, or PR descriptions.

## Rename Matrix

Mihoto is the fork and roadmap name. The inherited CLI remains `mihoro` until a
dedicated rename PR changes each surface deliberately.

| Surface | Current value | Baseline decision |
| --- | --- | --- |
| GitHub repository | `Pectics/mihoto` | Fork identity |
| Upstream repository | `spencerwooo/mihoro` | Attribution and sync source |
| Cargo package | `mihoro` | Keep until mechanical rename |
| Binary | `mihoro` | Keep until mechanical rename |
| Config file | `~/.config/mihoro.toml` | Keep until migration plan exists |
| User agent | `mihoro` | Keep until compatibility impact is reviewed |
| systemd service | `mihomo.service` | Keep; it manages Mihomo core |
| Release assets | `mihoro-<version>-<target>.tar.gz` | Keep until release rename plan |

Rename work must be split from behavior changes so that user-visible migrations
can be reviewed, tested, and rolled back independently.

## CI and Branch Protection Expectations

All Mihoto implementation work should use issue-linked pull requests against
`main`. The protected CI baseline is the `CI` workflow with these checks:

- `cargo fmt --all -- --check`
- `cargo clippy`
- `cargo check --all-targets`
- `cargo test --all-targets`

Branch protection is a repository setting, not a source-controlled file. The
expected GitHub setting for `main` is to require PRs and the `CI` status before
merge. If branch protection is changed, record that change in the linked issue or
PR.

## Safety Baseline

Early Mihoto work follows these rules:

- Updates, service changes, TUN/DNS changes, and migrations must be explicit,
  validated, auditable, and recoverable.
- Subscription URLs, secrets, and authentication material must not appear in
  logs, issues, or PR artifacts.
- Documentation-only PRs should say why no runtime tests were added.
- Rollback for documentation-only changes is a normal revert. The baseline tag
  should not be deleted or moved as part of rollback.
