# AGENTS.md

Notes for agents working in this repo.

## Project overview

Mihoto is a Rust CLI for managing Mihomo on Linux. It handles:
- Initializing and updating the Mihomo binary
- Managing remote configuration subscriptions (YAML configs)
- Bootstrapping config interactively via `mihoto init`
- Applying config overrides via TOML (local settings override remote YAML)
- Managing the system-level systemd service
- Managing optional web dashboard assets
- Self-upgrading to the latest GitHub release

## Build and development commands

```bash
# Build
cargo build
cargo build --release

# Run
cargo run -- [args]

# Check code
cargo check --all-targets

# Format
cargo fmt --all
cargo fmt --all -- --check  # Verify formatting

# Lint
cargo clippy --all-targets -- -D warnings

# Run tests
cargo test --all-targets
cargo test --all-targets --no-default-features

# Local installation
cargo install --path .
```

## CI commands

From `.github/workflows/ci.yml`:
```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo check --all-targets
cargo test --all-targets
scripts/check-system-scope.sh
```

## Architecture

### Module structure

```
src/
├── main.rs           # Tokio entry point and top-level error handling only
├── cli/              # Clap arguments, command dispatch, prompts, and terminal rendering
├── application/      # Init/update/apply/service/timer/uninstall/upgrade workflows
│   └── ports/        # Interfaces implemented by infrastructure adapters
├── domain/           # Config/UI models and pure validation/merge rules
└── infrastructure/   # HTTP, files, processes, systemd, assets, and self-upgrade
```

Dependencies point inward: CLI invokes application workflows; application depends on domain and
its own ports; infrastructure implements those ports. Domain code must not access files, network,
processes, or terminal output.

### Key pieces

1. **Config override system**: merges local TOML overrides with remote YAML configs
   - `Config`: Main TOML config at `/etc/mihoto.toml`
   - `MihomoConfig`: Mihomo-specific settings using `#[serde(default)]` extensively
   - `MihomoYamlConfig`: Parses remote YAML with `#[serde(flatten)]` to preserve unrecognized fields
   - Only mihomo_config fields are overridden; remote YAML fields pass through unchanged

2. **Systemctl builder**: method chaining for systemd commands
   ```rust
   Systemctl::new().start("mihomo.service").execute()?
   ```

3. **Init flow**: `mihoto init` is the main onboarding path
   - `bootstrap_config()` creates the default TOML config if missing
   - Interactive runs prompt for `remote_config_url` and continue in the same command
   - `--yes` is for non-interactive use and expects required fields to already be present
   - Stage reports make repeat runs safe and easier to follow

4. **Mihoto infrastructure facade**: holds config and derived managed paths
   - All methods return `anyhow::Result<T>` for consistent error handling
   - Uses Tokio async for downloads
   - Lives under `infrastructure/mihomo/`; application workflows reach it through ports

5. **Self-upgrade**: updates from GitHub releases
   - `upgrade::run_upgrade()`: Downloads and replaces the current binary
   - `upgrade::check_for_update()`: Checks for new versions without installing
   - Uses `self_update` crate with GitHub backend
   - Runs in `tokio::task::spawn_blocking` to avoid async runtime conflicts
   - Release artifacts must be named `mihoto-<version>-<target>.tar.gz`

### Configuration flow

1. `mihoto init` creates `/etc/mihoto.toml` if it does not exist
2. Interactive init prompts for the remote subscription URL when `remote_config_url` is empty
3. Remote YAML config is downloaded from the subscription URL
4. Local TOML overrides are merged into the final `config.yaml`
5. The system service is written, enabled, and started

### Runtime paths

- Config: `/etc/mihoto.toml`
- Mihomo binary: `/usr/local/bin/mihomo`
- Mihomo config: `/etc/mihomo/config.yaml`
- Systemd service: `/etc/systemd/system/mihomo.service`

## Dependencies

- `clap` 4.5: CLI argument parsing with derive macros
- `tokio` 1.44: Async runtime (full features)
- `serde` + `serde_yaml`: Serialization/deserialization
- `reqwest` 0.12: HTTP client with streaming support
- `anyhow`: Error handling
- `colored`: Terminal colors
- `indicatif`: Progress bars for downloads
- `self_update` 0.42: Self-upgrade functionality with GitHub releases backend

## Code style

- Edition: Rust 2021
- Formatting: `rustfmt.toml` (max line width 100, hard tabs)
- Linting: `clippy.toml` sets thresholds for complexity/argument count
- Unit tests live alongside the modules under `src/`
- `cargo test` currently discovers 31 tests

## System integration

State-changing commands require explicit root execution and use system-level systemd units.
