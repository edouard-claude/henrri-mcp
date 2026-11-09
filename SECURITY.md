# Security

Please report vulnerabilities privately, through GitHub's
[security advisories](https://github.com/edouard-claude/henrri-mcp/security/advisories/new),
not in a public issue. You should get an answer within a week.

In scope: anything that lets this server leak credentials or tokens, act on
Henrri without the configured guards (`confirm`, `HENRRI_READ_ONLY`,
`HENRRI_HTTP_TOKEN`), accept a forged webhook, or write outside
`HENRRI_DOWNLOAD_DIR`.

Out of scope: the Henrri API itself. Report those to Henrri.
