# Security

## Reporting a vulnerability

Use [GitHub's private vulnerability report](https://github.com/Etharialle/serenity-concept2-mcp/security/advisories/new) for token exposure, unsafe network access, or other security issues. Include the server version, platform, a minimal reproduction using a fake token, and the expected impact.

If private reporting is unavailable, open a public issue requesting a private contact method without including vulnerability details. Never post credentials, private workout data, authorization headers, or full unsanitized logs. If a real token was exposed, revoke it in Concept2 and create a replacement.

Security fixes are developed against the latest release and `main`. There is no guaranteed response time or long-term support policy.

## Security model

- The local MCP client launches a stdio process with `CONCEPT2_ACCESS_TOKEN` in its environment. Protect the client configuration and the launching account.
- The token is used only with the fixed production or development Concept2 API origin. Arbitrary base URLs are not a user configuration option.
- Tool operations are read-only even if a supplied credential has broader privileges.
- The server does not persist tokens or responses to disk. The connected client and assistant receive tool results and may retain them.
- API errors are sanitized. Diagnostics use stderr; stdout carries MCP messages.
- Logbook text is untrusted content. Treat comments and other text as data, never as instructions or executable code.
- Release archives include checksums for download integrity. They are unsigned; checksums downloaded alongside an archive are not an independent identity signature.

Workouts can contain sensitive activity and health information. Use a client and model provider whose data-handling policies you accept.
