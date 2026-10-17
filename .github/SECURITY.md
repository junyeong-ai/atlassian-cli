# Security Policy

## Supported Versions

Only the most recent minor release receives security fixes. Older versions
are not patched — upgrade to the latest tag to receive security updates.

## Reporting a Vulnerability

Please report security issues **privately** through GitHub's Security
Advisory workflow:

1. Open the repository's **Security** tab.
2. Click **Report a vulnerability** and fill in the form.

Include a clear description, reproduction steps, and an impact assessment.
We aim to acknowledge reports within **3 business days** and to ship a fix
or mitigation within **30 days** of confirmation, scaled by severity.

Do **not** open a public issue or pull request for security matters until a
coordinated disclosure date has been agreed.

## Supply-chain Hardening

- All GitHub Actions are pinned to commit SHAs. Dependabot opens weekly
  PRs to refresh both Cargo dependencies and Action pins.
- Every Cargo PR runs `cargo-deny` (`advisories` + `licenses` + `bans` +
  `sources`), so new transitive dependencies are policy-checked before merge.
  GitHub Dependabot Alerts cover real-time CVE notifications.
- Workflow definitions are linted with `zizmor` on pushes to `main`, relevant
  pull requests, merge groups, and a weekly schedule.
- Releases include SLSA build provenance attestations
  (`actions/attest-build-provenance`), per-artifact SHA-256 checksums, and an
  SPDX SBOM (`anchore/sbom-action`).
- The OpenSSF Scorecard for this repository is published on every push to
  `main`.

## Credential Handling

`atlassian-cli` resolves credentials in strict precedence order:
**CLI flag → environment variable → config file**, mediated by
`config::AuthResolver` (see [AGENTS.md](../AGENTS.md)).
Credential storage depends on the authentication method:

- API tokens and OAuth client secrets are supplied by CLI, environment or a
  user-managed config file. Auth configuration excludes secrets when serialized.
- Service-account access tokens are cached in memory.
- OAuth access and refresh tokens are stored in the OS keychain, with an atomic
  `credentials.json` fallback (0600 file inside a 0700 directory on Unix).
  `ATLASSIAN_NO_KEYCHAIN` selects file-only access and leaves existing keychain
  entries untouched.

Resolved config output masks secrets. Authentication Debug output and HTTP
Authorization headers are redacted; CLI help hides environment variable values.
OAuth callbacks validate the path, state and parameter uniqueness, and escape
provider text before displaying it in the browser.

Config files at permissive permissions trigger a runtime warning. We
recommend `chmod 600` on any file containing an API token or service
account secret.
