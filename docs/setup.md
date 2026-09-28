# Setup

## 1. Install the executable

### Prebuilt archives

Use assets attached to a [GitHub release](https://github.com/Etharialle/serenity-concept2-mcp/releases). If no release is available, build from source below.

| Platform | Archive target | Native CI environment |
| --- | --- | --- |
| Windows x64 | `x86_64-pc-windows-msvc.zip` | Windows Server 2022 |
| Linux x64 | `x86_64-unknown-linux-gnu.tar.gz` | Ubuntu 22.04 |
| macOS Apple Silicon | `aarch64-apple-darwin.tar.gz` | macOS 15 arm64 |
| macOS Intel | `x86_64-apple-darwin.tar.gz` | macOS 15 x64 |

Full filenames begin with `serenity-concept2-mcp-v0.1.0-`. Select the architecture of your machine. Releases are gated by native tests in these environments; older operating systems have not been validated. Linux uses GNU libc and is not an Alpine/musl build. The CI logs record linked runtime libraries. Windows builds may require the Microsoft Visual C++ runtime. The binaries are unsigned, and macOS builds are not notarized.

Download the archive and its `.sha256` sidecar (or the combined `SHA256SUMS`). Compare the digest before extracting.

PowerShell:

```powershell
$archive = 'serenity-concept2-mcp-v0.1.0-x86_64-pc-windows-msvc.zip'
$expected = ((Get-Content -LiteralPath "$archive.sha256").Trim() -split '\s+')[0]
$actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash
if ($actual -ne $expected) { throw 'Archive checksum did not match' }
Expand-Archive -LiteralPath $archive -DestinationPath "$env:LOCALAPPDATA\SerenityConcept2"
```

Linux:

```sh
sha256sum -c serenity-concept2-mcp-v0.1.0-x86_64-unknown-linux-gnu.tar.gz.sha256
tar -xzf serenity-concept2-mcp-v0.1.0-x86_64-unknown-linux-gnu.tar.gz
```

macOS Apple Silicon:

```sh
shasum -a 256 -c serenity-concept2-mcp-v0.1.0-aarch64-apple-darwin.tar.gz.sha256
tar -xzf serenity-concept2-mcp-v0.1.0-aarch64-apple-darwin.tar.gz
```

Keep the extracted executable, license, and third-party notices together. Unix archives preserve executable permission; if it is lost while copying, restore it with `chmod +x /absolute/path/to/serenity-concept2-mcp`. On macOS, use the system's normal security review flow for unsigned software you trust.

Verify the extracted executable by running its absolute path with `--version`. This does not require a token.

### Build from source

Install [Rust with rustup](https://rustup.rs/) and a native linker (Visual Studio C++ Build Tools on Windows, Xcode Command Line Tools on macOS, or a C/C++ build toolchain on Linux). This checkout selects Rust 1.96.0.

```sh
git clone https://github.com/Etharialle/serenity-concept2-mcp.git
cd serenity-concept2-mcp
cargo build --release --locked
```

The binary is `target/release/serenity-concept2-mcp`, with `.exe` on Windows. For an installed copy, run `cargo install --path . --locked` from a reviewed release checkout. It installs in Cargo's binary directory, normally `~/.cargo/bin`. There is no published crates.io package at this stage.

## 2. Supply a Concept2 credential privately

Create a personal token through your [Concept2 Logbook account](https://log.concept2.com/), following the [Concept2 API documentation](https://log.concept2.com/developers/documentation/). Where scopes can be selected, use only `user:read` and `results:read`. The server enforces read-only operations even if the credential has broader permissions.

Supply the token to the server through `CONCEPT2_ACCESS_TOKEN`. Use your MCP client's secret or environment configuration, or arrange for the client to inherit the variable when launched. Environment inheritance differs by client and operating system; a desktop app already running will not pick up a newly set shell variable. A shell-launched server waiting silently is normal: it expects MCP protocol messages on stdin.

For a temporary PowerShell session, prompt without writing the token into shell history:

```powershell
$secret = Read-Host 'Concept2 access token' -AsSecureString
$env:CONCEPT2_ACCESS_TOKEN = [System.Net.NetworkCredential]::new('', $secret).Password
# Launch your MCP client from this session so its child server can inherit the variable.
```

For a temporary Bash session:

```bash
read -rsp 'Concept2 access token: ' CONCEPT2_ACCESS_TOKEN
printf '\n'
export CONCEPT2_ACCESS_TOKEN
# Launch your MCP client from this session so its child server can inherit the variable.
```

Use a secret store supported by your client for persistent setup. Never paste the token into chat, a tool call, an issue, or a repository file. If a client requires a plaintext local configuration, keep it outside this checkout and restrict access to your user account. The server does not load `.env` files or accept tokens as command-line arguments. Rotate a token by revoking it in Concept2, replacing the local value, and restarting the server.

## 3. Configure your MCP client

Use the client's stdio server configuration. The following generic JSON illustrates a common shape; consult your client's documentation for the exact settings location and secret handling. These examples assume the client supplies or inherits `CONCEPT2_ACCESS_TOKEN` separately.

Windows:

```json
{
  "mcpServers": {
    "concept2": {
      "command": "C:\\Users\\YOUR_USER\\.cargo\\bin\\serenity-concept2-mcp.exe",
      "args": []
    }
  }
}
```

macOS/Linux:

```json
{
  "mcpServers": {
    "concept2": {
      "command": "/home/YOUR_USER/.cargo/bin/serenity-concept2-mcp",
      "args": []
    }
  }
}
```

Replace `command` with the real absolute path; on macOS a home directory normally starts with `/Users/`. Keep the executable path separate from `args` so spaces are handled correctly. The server uses production by default. To use a separate Concept2 development account/token, set `"args": ["--environment", "development"]`. A custom API URL is not supported.

Restart or reconnect the client. It should discover the five tools listed in the [tool reference](tool-reference.md).

## 4. Optional live acceptance

Automated tests use synthetic data and do not establish real-account compatibility. To perform a private acceptance check:

1. Retrieve your profile and confirm the username while checking that email and birth date are absent.
2. List a small date range containing known workouts. Follow a second page with a small `page_size` and confirm filters, IDs, and totals against the Logbook UI.
3. Retrieve one steady workout and one interval workout, comparing meters, work duration, rest, and splits.
4. Request a small stroke window for a workout known to contain stroke data. Check a workout without strokes as well.
5. Summarize that same date range. Compare the equipment totals with the logbook and inspect `coverage.complete`, `coverage.reason`, and warnings.
6. Record only the server/client versions, operating system, and pass/fail results in a private note. Keep credentials and returned records outside Git.

These calls only read account data. They make that data available to your connected client and assistant. Use a client whose data handling you accept.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| Server exits at startup | Confirm the executable path, executable permission, and `CONCEPT2_ACCESS_TOKEN` in the actual child process environment. |
| Authentication or permission error | Check whether the token is current, has read scopes, and belongs to the selected environment. |
| No visible terminal output | Stdio mode waits for an MCP client; diagnostics are written to stderr. |
| Timeout or rate limiting | Retry later or narrow the request; do not repeatedly query the same large interval. |
| Partial summary | Read `coverage.reason`, reduce the date range, and retry. Never describe partial totals as a complete history. |
| No stroke data | A workout can exist without uploaded stroke records. `available: false` is distinct from a missing workout or failed request. |
| Unexpected data shape | Report a sanitized example with synthetic values. Do not attach the raw account response. |

For private security reports, see [SECURITY.md](../SECURITY.md).
