# Contributing

This project implements a local, read-only Concept2 Logbook MCP server. Changes should preserve accurate units, explicit partial coverage, and private handling of account data.

## Local setup

Install Rust with [rustup](https://rustup.rs/) and your platform's native linker. The checkout pins Rust 1.96.0, which is also the minimum supported Rust version. On Windows, use the MSVC toolchain and Visual Studio C++ build tools.

```sh
git clone https://github.com/Etharialle/serenity-concept2-mcp.git
cd serenity-concept2-mcp
cargo build --locked
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

Tests use synthetic fixtures and local mock servers. You do not need a Concept2 account or token. Dependency downloads need network access; the tests do not call the Concept2 API.

## Changes and pull requests

- Keep changes focused. Open an issue first for write support, remote hosting, OAuth, or changes to summary semantics.
- Include a regression test for a meaningful bug fix. Use independently calculated expected values for numerical tests.
- Keep source units and normalized units explicit; preserve absent values.
- Update the tool reference and data semantics when an output or calculation changes.
- Commit `Cargo.lock` when dependencies change. Review new dependencies, their licenses, and feature flags.
- Keep private logs, tokens, identifiers, and captured API data out of commits, fixtures, issues, and PRs. Use synthetic records.
- Explain what changed, why, and which checks passed. Mention checks you could not run.

The [development plan](DEVELOPMENT_PLAN.md) describes the intended architecture and deferred features. The [Concept2 API documentation](https://log.concept2.com/developers/documentation/) is the upstream contract; preserve unknown fields safely and validate assumptions with fixtures.

## Native package verification

Python 3.12 or later is required for the packaging helper, but not for the server itself.

```sh
cargo build --release --locked
python scripts/package.py --binary target/release/serenity-concept2-mcp --target x86_64-unknown-linux-gnu
```

Use the native target reported by `rustc --version --verbose`. On Windows, pass `target/release/serenity-concept2-mcp.exe` and `x86_64-pc-windows-msvc`; use `aarch64-apple-darwin` or `x86_64-apple-darwin` on macOS.

The helper collects dependency notices, creates the archive and SHA-256 sidecar, checks the extracted file list, runs `--version`, and exercises MCP initialization, discovery, read-only annotations, invalid input, and shutdown. A synthetic token and unreachable local proxy prevent real API use during these calls. Output is written to `dist/`.

Run the release verification regression tests with `python -m unittest discover -s scripts -p 'test_*.py'`. They check modified archives, unexpected assets, substituted checksum filenames, and mismatched tags using synthetic files.

## Releases

Only maintainers publish releases. Before tagging, review the changelog, package version, dependency notices, CI results, and known acceptance gaps. A `v*` tag triggers the full native CI matrix. The release job verifies the tag matches `Cargo.toml`, checks all four archive checksums, and publishes only after the checks succeed.

The repository does not publish to crates.io. A release is unsigned and not notarized unless its notes explicitly state otherwise. Minimum runtime support is limited to the environments actually exercised by CI; see [setup](docs/setup.md).
