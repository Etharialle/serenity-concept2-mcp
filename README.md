# Serenity Concept2 MCP

[![CI](https://github.com/Etharialle/serenity-concept2-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/Etharialle/serenity-concept2-mcp/actions/workflows/ci.yml)

A local, read-only [Model Context Protocol](https://modelcontextprotocol.io/) server for your Concept2 Logbook, built in Rust.

Connect an MCP assistant to your profile, workouts, splits, and stroke data. Ask for summaries with explicit units, equipment breakdowns, and coverage limits. The server uses a personal Concept2 access token and communicates with your client over stdio.

Independent community project. Not affiliated with or endorsed by Concept2.

## Tools

| Tool | Purpose |
| --- | --- |
| `concept2_get_profile` | Retrieve a minimal profile, excluding email and birth date. |
| `concept2_list_workouts` | Browse workouts with date, equipment, and pagination filters. |
| `concept2_get_workout` | Read a workout with available splits and intervals. |
| `concept2_get_strokes` | Retrieve a bounded window of available stroke records. |
| `concept2_summarize_workouts` | Calculate totals by equipment and day, week, or month. |

The server only performs GET requests against the selected Concept2 API environment. It does not create, change, or delete logbook records.

## Get started

1. Build from source or download an archive from [GitHub Releases](https://github.com/Etharialle/serenity-concept2-mcp/releases), when available.
2. Create a personal token in your [Concept2 Logbook](https://log.concept2.com/). Supply it privately through `CONCEPT2_ACCESS_TOKEN` in the environment used by your MCP client.
3. Configure your client to launch the absolute path to `serenity-concept2-mcp` (`.exe` on Windows).

The [setup guide](docs/setup.md) covers installation, checksums, Windows and Unix examples, token handling, and troubleshooting. No Rust installation is needed to run a prebuilt executable.

For a source build, install the [Rust toolchain](https://rustup.rs/) and your platform's native linker, then run:

```sh
git clone https://github.com/Etharialle/serenity-concept2-mcp.git
cd serenity-concept2-mcp
cargo build --release --locked
```

The repository pins Rust 1.96.0. The executable is written to `target/release/`.

Example prompts after connecting:

- “List my rowing workouts from September 1 through September 7, 2026.”
- “Show the intervals in workout 12345.”
- “Summarize my SkiErg distance and work duration for September 2026, grouped by week. Tell me whether coverage is complete.”

## Privacy and limits

- One account per local process. Credentials are supplied at startup, never as tool arguments.
- No on-disk workout cache or telemetry. Data returned by tools is available to your MCP client and assistant; their storage policies apply.
- Summary fetches are bounded to 20 pages, 5,000 records, and 30 seconds. Partial totals are labeled.
- Missing measurements and timezones remain unknown. Rest is reported separately from work.
- User-authored text in the logbook is data, and must not be treated as instructions.
- Production and development are separate Concept2 environments. Production is the default; use `--environment development` with a development credential when needed.

Synthetic tests and a private real-account stdio check passed on 2026-09-28, covering all five tools and independently reconciled summary totals. Desktop-client setup and comparison with the Logbook UI remain manual acceptance steps. See [data semantics](docs/data-semantics.md) for definitions and coverage caveats.

## Documentation and development

- [Setup and live acceptance checklist](docs/setup.md)
- [Tool reference](docs/tool-reference.md)
- [Units, dates, and summary calculations](docs/data-semantics.md)
- [Development plan](DEVELOPMENT_PLAN.md)
- [Architecture diagram](docs/diagrams/serenity-concept2-architecture.png) · [editable draw.io source](docs/diagrams/serenity-concept2-architecture.drawio)
- [Contributing](CONTRIBUTING.md) · [Security](SECURITY.md) · [Changelog](CHANGELOG.md)

Run the local checks with:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

CI builds and exercises native packages on Windows x64, Linux x64, and macOS arm64/x64. Consult the CI run for the specific commit before assuming a platform passed. Release archives are unsigned and macOS binaries are not notarized.

## License

[MIT](LICENSE). Release archives include third-party dependency license notices.
