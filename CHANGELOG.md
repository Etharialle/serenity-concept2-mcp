# Changelog

## 0.1.0

Initial read-only implementation.

### Added

- Local stdio MCP server in Rust with the official `rmcp` SDK.
- Profile, workout listing/detail, stroke data, and deterministic summary tools.
- Explicit source and normalized units, equipment grouping, bounded pagination, and partial-coverage metadata.
- Personal-token authentication through `CONCEPT2_ACCESS_TOKEN`, fixed Concept2 environments, and sanitized failures.
- Synthetic unit, API contract, and MCP tests.
- Setup, tool reference, data semantics, contributor and security documentation.
- Native CI and release packaging for Windows x64, Linux x64, and macOS arm64/x64, including license notices and SHA-256 checksums.

### Known limits

- Real-account and desktop-client acceptance remain pending; fixtures are synthetic.
- No writes, OAuth browser flow, hosted transport, disk cache, or telemetry.
- API pagination cannot guarantee a consistent snapshot while records are being edited.
- Release binaries are unsigned and are not notarized on macOS. Older OS versions than the CI environments have not been validated.
