# Serenity Concept2 MCP — Development Plan

Date: 2026-09-28  
Status: Rust implementation and public source repository created; native release verification in progress. Live-account acceptance remains pending a locally configured token.

Architecture diagram: [PNG preview](docs/diagrams/serenity-concept2-architecture.png) · [Editable draw.io source](docs/diagrams/serenity-concept2-architecture.drawio).

## 1. Goal and decisions

Build an open-source MCP server that lets an assistant retrieve a user's Concept2 Logbook history and produce accurate, reproducible workout summaries.

**Confirmed requirements**

- Project and repository name: `serenity-concept2-mcp`.
- Source will live in the user's public GitHub account.
- Implementation language: Rust.
- Implement the plan and publish traceable commits to GitHub (authorized 2026-09-28).

**Adopted implementation defaults**

- Public repository: [Etharialle/serenity-concept2-mcp](https://github.com/Etharialle/serenity-concept2-mcp).
- First release: read-only, one account per local process, using stdio.
- Stable Rust, Cargo, and the official Rust MCP SDK (`rmcp`).
- MIT license for project code, with required dependency notices preserved.
- Distribution through GitHub Releases as native executables; optional crates.io publication after release validation.

The repository was initialized on `main` and published with separate planning and implementation commits. All committed fixtures are synthetic. No credentials or real workout data are needed in the public repository.

### Implementation status

- Implemented all five read-only tools, typed normalization, bounded API requests, summary coverage, input validation, cancellation, and stdio transport.
- Pinned Rust 1.96.0, `rmcp` 3.5.0 and the dependency lockfile. Tests cover mocked HTTP contracts, deterministic totals, output schemas, protocol cancellation, deadlines, and process behavior.
- Added setup and tool guides, MIT and dependency notices, contributor/security guidance, native CI, and checksummed release packaging with extracted-binary smoke tests.
- Native target matrix: Windows x64, Linux x64, macOS arm64 and macOS Intel. Release automation verifies every native target before publishing unsigned archives.
- Pending acceptance: read the user's live profile/results/detail/strokes, compare a bounded summary with the Logbook UI, and verify an intended desktop client's configuration. Synthetic SDK and subprocess tests do not establish live-account compatibility.
- Deferred scope remains hosted OAuth, writes, optional crates.io publication, and an MCP Registry listing.

## 2. First-release scope

The server should support questions such as:

- “Show my workouts from last week.”
- “Explain the intervals in this workout.”
- “Compare this month's rowing volume with last month's.”

Implemented tools:

| Tool | Inputs | Result |
| --- | --- | --- |
| `concept2_get_profile` | None | Minimal account identity and relevant profile settings; omit email and birth date by default. |
| `concept2_list_workouts` | Date range, equipment, page, page size | Compact workout records and explicit pagination. |
| `concept2_get_workout` | Workout ID | Details, available splits/intervals, source fields, and normalized units. |
| `concept2_get_strokes` | Workout ID, output window | Bounded stroke records; distinguish unavailable data from request failure. |
| `concept2_summarize_workouts` | Required date range, equipment, grouping | Session counts, distance, work duration, and breakdowns with coverage metadata. |

Return a validated structured result and a concise text representation. Include fetch time, applied filters, units, warnings, and completeness where relevant. Treat comments and other user-authored fields as untrusted data.

Defer workout creation/editing/deletion, account changes, remote hosting, webhooks, file exports, challenge tools, and personal-best analysis. Direct PM5/Bluetooth connectivity is a separate integration.

## 3. Verified Concept2 integration facts

Concept2 documents personal tokens for reading data and OAuth authorization-code/refresh flows. OAuth requires registered client credentials. Explicit read scopes are `user:read,results:read`; omitted scopes can include writes. Production reads are permitted; production writes require development-server testing and Concept2 approval. [Concept2 API documentation](https://log.concept2.com/developers/documentation/)

Use HTTPS and `Accept: application/vnd.c2logbook.v1+json`. Reads map to `/api/users/me`, `/api/users/me/results`, `/api/users/me/results/{id}`, and its `/strokes` suffix. Results support date/equipment filters and pagination: default 50, maximum 250, pages start at 1. [API reference](https://log.concept2.com/developers/documentation/)

Workout durations use tenths of seconds; workout distance uses meters, while stroke distance uses decimeters. Stroke pace uses 500 m for RowErg/SkiErg and 1,000 m for BikeErg. Work and rest are separate. Workout dates represent the finish; timezone/UTC fields may be absent. `updated_after` uses GMT. [Data definitions](https://log.concept2.com/developers/documentation/)

## 4. Architecture and stack

Use a single Cargo package with a thin executable and a library containing the server and domain logic:

| Component | Responsibility |
| --- | --- |
| CLI and configuration | Validate settings, obtain a credential, start stdio, report startup failures. |
| MCP server constructor | Register tools and schemas; receive dependencies so tests can inject a fake API. |
| Concept2 client | Fixed API endpoints, authentication headers, response validation, pagination, timeouts, error mapping. |
| Domain functions | Normalize records and calculate deterministic summaries without an LLM dependency. |
| Credential provider | Supply a personal token initially; allow a separate OAuth provider later. |

Rust is the selected language. Use the official `rmcp` SDK and its Tokio-based transport support for protocol handling. Native releases let users run the server without installing a Rust toolchain. [Official Rust SDK](https://github.com/modelcontextprotocol/rust-sdk)

Dependency choices:

| Area | Choice |
| --- | --- |
| MCP and async execution | `rmcp` and `tokio`; enable only required server/stdio features in production, with client support for tests. |
| Data and schemas | `serde`, `serde_json`, and an `rmcp`-compatible `schemars` version for typed records and JSON Schema. Validate ranges and cross-field rules explicitly; test that serialized outputs match declared schemas. |
| HTTPS | A shared async `reqwest::Client` with Rustls, explicit timeouts, and controlled redirects. Verify certificate trust on each supported platform. |
| CLI, errors, diagnostics | `clap`, `thiserror`, and `tracing` with a stderr subscriber; secrets must not appear in derived debug output. |
| Tests and quality checks | Rust's test harness, Tokio async tests, mock HTTP responses, `rustfmt`, and Clippy. |

Reqwest supports async requests and configurable TLS and redirect behavior. Choose exact crate versions and feature flags during the integration spike. [Reqwest documentation](https://docs.rs/reqwest/latest/reqwest/)

Pin a tested stable toolchain in `rust-toolchain.toml`, declare the minimum supported Rust version in `Cargo.toml`, and commit `Cargo.lock`. Use `--locked` for dependency-resolving CI commands. Document source installation with `cargo install --path . --locked` from a release checkout. Binary setup instructions must cover target selection, executable permissions, and the absolute executable path in the MCP client's configuration.

Represent units with explicit field names or small newtypes, preserve missing values with `Option`, and return typed errors through `Result`. Use checked integer arithmetic for accumulated source measurements. Avoid panics on external input; deserialization alone does not establish semantic validity.

Proposed layout:

```text
src/
  main.rs
  lib.rs
  server.rs
  config.rs
  api.rs
  domain.rs
  transport.rs
tests/
  fixtures/
  api_contract.rs
  server_protocol.rs
  transport_process.rs
scripts/
  smoke.py
  package.py
  licenses/
docs/
  setup.md
  tool-reference.md
  data-semantics.md
.github/
  workflows/
  ISSUE_TEMPLATE/
Cargo.toml
Cargo.lock
rust-toolchain.toml
README.md
DEVELOPMENT_PLAN.md
CONTRIBUTING.md
SECURITY.md
LICENSE
```

Place unit tests alongside the modules they exercise; integration-test entry points live directly under `tests/`. [Cargo package layout](https://doc.rust-lang.org/cargo/guide/project-layout.html)

## 5. Authentication and data handling

### Local release

- Read `CONCEPT2_ACCESS_TOKEN` from the host-provided environment. Document secure local setup and rotation; never request the token through an MCP tool argument or chat message.
- Offer a file-based secret option only if it can be implemented with clear filesystem-permission guidance. Keep secrets outside the repository.
- Treat the supplied token's privileges as unknown. Enforce read-only behavior with a fixed GET-only client and tool surface.
- Keep all diagnostics on stderr; stdout is reserved for MCP traffic.
- Redact authorization headers, tokens, and personal response bodies from diagnostics. Test redaction with distinctive fake secrets.
- Keep the default operation stateless, with no disk cache or telemetry. Explain that returned logbook data becomes available to the connected assistant/client.
- Limit requests to the configured production or development Concept2 origin. Reject arbitrary URLs and cross-origin redirects; build pagination requests from validated parameters.

### OAuth and hosted access, later

Keep Concept2 account authorization separate from authorization to a remotely hosted MCP service. Never accept an MCP client's bearer token as a Concept2 credential. Use the SDK's Streamable HTTP implementation and verify the current MCP authorization requirements when this phase starts. [MCP authorization](https://modelcontextprotocol.io/specification/latest/basic/authorization)

Before choosing an OAuth design, verify Concept2's behavior for state, PKCE, registered callbacks, and loopback redirects. Those details were not established by the documentation reviewed. Do not embed a shared client secret in source code or a distributed executable. Evaluate user-owned client registration or a separately operated confidential backend. A backend introduces hosting, account isolation, encrypted token storage, refresh coordination, and operating costs.

## 6. Correctness and reliability rules

- Preserve source values alongside normalized fields where useful. Never infer a missing timezone or turn a missing measurement into zero.
- Calculate volume by equipment. Keep mixed-equipment workouts separate unless their components can be attributed reliably.
- Define the summary's date basis, work/rest treatment, and inclusion rules in `data-semantics.md`. Avoid averaging per-workout pace values; any aggregate pace must use compatible total distance and duration.
- Lists default to 50 records. Summaries use 250 records per page, with budgets of 20 pages, 5,000 records, and 30 seconds. HTTP responses are capped at 8 MiB, normalized result envelopes at 1 MiB, incoming MCP lines at 64 KiB, and workout details at 1,000 combined splits and intervals.
- Fix the query's upper date bound before fetching and deduplicate records by workout ID across pages. Check for changing page metadata and flag detected inconsistencies. Until snapshot behavior is verified, disclose that `complete` means all reported pages were retrieved within budget; concurrent edits or deletions may still shift records during retrieval.
- Stop at the first exhausted budget. Return `complete: false`, `records_included`, a reason, and enough context to retry a narrower range. Label partial totals as partial. A failed fetch must never become an empty successful summary.
- Avoid per-workout detail requests when list data is sufficient. Fetch stroke data only when requested. Bound upstream response bytes as well as returned records; slicing a large response alone is insufficient.
- Apply request deadlines, cancellation, and conservative concurrency. Retry idempotent reads only for transient failures, with capped backoff and `Retry-After` handling.
- Distinguish authentication, permission, not-found, validation, throttling, timeout, and upstream-service errors. Sanitize messages before returning them.
- Tolerate additive upstream fields while validating required data. Preserve unknown equipment/workout values and disclose exclusions from calculations.

## 7. Delivery milestones

| Milestone | Deliverables | Exit criteria |
| --- | --- | --- |
| 0 — Validate the contract | Confirm initial client, read-only scope, token setup, real response shapes, data semantics, and native release targets. Record toolchain and crate versions. | A private read-only smoke check retrieves profile, multiple result pages, one detail record, and available stroke data. Identify differences from docs. |
| 1 — Foundation | Cargo package, library/server constructor, thin executable, configuration, schemas, test harness, synthetic fixtures. | `cargo build --locked` succeeds on a clean checkout; an `rmcp` client can initialize the executable and discover its tools. No secrets required for CI. |
| 2 — Core reads | API client, first four tools, normalization, pagination, error handling. | Mocked contract tests and stdio integration tests pass; valid results fit declared output schemas. |
| 3 — Summaries | Deterministic aggregations, bounds, coverage metadata, documented calculation rules. | A known multi-page dataset yields exact expected totals; interrupted and capped queries disclose partial coverage. |
| 4 — Public release | Setup guide, examples, license, contributor/security docs, CI, native archives/checksums, repository and release preparation. | Extracted executables pass smoke tests on each advertised OS/architecture; source builds work with the documented Rust toolchain; release contents contain only intended public files. |

Milestones 1–3 are implemented and verified with synthetic data. Documentation and release automation are implemented for milestone 4. Milestone 0's private live check remains open and must be completed before claiming live-account acceptance.

## 8. Verification plan

**Unit tests:** unit conversion, missing fields, zero values, numeric overflow, work/rest separation, date boundaries, equipment grouping, complete/partial totals, and input limits. Use explicit expected values from independently constructed fixtures. Run with `cargo test --locked`.

**HTTP contract tests:** multiple pages, overlapping records and changing page metadata, empty histories, unexpected fields, malformed payloads, authentication/permission failures, unavailable strokes, transient failures, retry limits, cancellation, and unsafe URLs/redirects. All fixtures must be synthetic or deliberately anonymized.

**MCP integration tests:** use the official `rmcp` client to test the server with injected fake dependencies, invoke every tool, and validate structured results and errors. Separately launch the built executable as a subprocess to test missing-credential failures, initialization, discovery, invalid-input handling, stdout framing, cancellation, and clean shutdown. Test deadlines must prevent a hung child process from blocking CI. Confirm all first-release tools advertise read-only behavior and no write method exists in the API layer.

**Packaging:** extract each release archive into a clean directory, verify its checksum, run `--version`, and exercise MCP initialization/discovery and a tool-validation failure with a synthetic token and network access disabled. Run on matching OS/architecture hosts; cross-compilation alone is insufficient. Inspect runtime-library requirements, minimum supported OS versions, archive contents, and secret checks. CI must never access the user's account. If crates.io distribution is added, verify `cargo package --locked` and installation from the unpacked crate before publication.

**Live acceptance:** opt-in local read-only checks with the user's credential; compare selected records and a bounded summary with the Logbook UI. Use at least one intended desktop client plus the SDK client. Record versions actually tested and keep captured private data out of Git.

## 9. Public GitHub and open-source release

During implementation:

1. Recheck the target owner and repository, initialize Git on `main`, and stage only reviewed project files.
2. Prepare README, MIT license proposal, contribution guide, security-reporting instructions, issue/PR templates, and a changelog. Document that this is an independent community project.
3. Add `.gitignore` rules for `target/`, credentials, local configuration, logs, caches, and private fixtures. Keep `Cargo.lock` tracked. Supply examples containing placeholders only.
4. Configure GitHub Actions for `cargo fmt --all -- --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked`, and `cargo build --release --locked`. Test the pinned toolchain across supported platforms and check the declared minimum Rust version. Inspect dependencies and release archives. Use minimal workflow permissions and keep credentials unavailable to untrusted pull requests.
5. Create the public `Etharialle/serenity-concept2-mcp` repository and push the reviewed initial history when publication is authorized. Enable available secret protection and require passing checks for merges.
6. Tag the first release `v0.1.0`. Proposed binary targets: Windows x86_64 (MSVC), Linux x86_64 (GNU), and macOS arm64/x86_64. Validate target availability and minimum OS/runtime requirements during milestone 0; advertise only tested targets. Publish a ZIP for Windows and tar.gz archives for Unix platforms, with the executable, license/notices, and SHA-256 checksums. Document installation and any signing/notarization status; decide release signing before publishing. Publish reviewed artifacts once release checks pass and publication is authorized.
7. Consider an MCP Registry listing after setup instructions and compatibility are proven.

Optional crates.io publication requires checking the crate name, package metadata, included files, and packaged-source installation first. Document `cargo install --locked` only after a crate release exists. Source builds remain available directly from the GitHub release tag.

The user authorized repository creation, implementation, and GitHub publication on 2026-09-28. The public repository is created; release automation publishes native artifacts only after its validation jobs pass.

## 10. Later roadmap and open choices

**v0.2 candidates:** challenge discovery, optional exports, additional summary metrics, and personal-best queries with precise comparison rules and declared history coverage.

**Optional writes:** require a separate design and the Concept2 prerequisites above. Default writes off; introduce an explicit preview/approval flow tied to the exact proposed change, duplicate protection, and audit-friendly result IDs. Do not equate a model-supplied `confirm: true` value with user approval. Design deletion separately because it removes source records.

**Optional hosted service:** add Streamable HTTP, user isolation, MCP authorization, Concept2 OAuth, deployment guidance, and operational ownership after choosing a hosting model.

Remaining release and integration choices:

- Select the intended desktop client for live acceptance.
- Consider signing/notarization for later releases; initial archives are documented as unsigned.
- Revisit remote hosting or mutations only as separately scoped features.

**Next acceptance step:** configure a personal token locally using `docs/setup.md`, then run the private read-only checks in section 8. Keep captured account data out of Git.
