# atlassian-cli

Rust 2024 library and single binary for Atlassian Cloud. `rust-toolchain.toml`
pins the development compiler; `Cargo.toml` declares the minimum supported Rust
version. Keep the minimum version unless a dependency or language feature
requires raising it.

## Development

```bash
cargo build --release --locked
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features --locked
cargo test --workspace --all-features --doc --locked
cargo deny check advisories bans licenses sources
```

After changing CLI definitions, regenerate the command reference:

```bash
cargo run --locked --example cli_reference > docs/cli.md
```

Use the existing wiremock fixtures for HTTP behavior. Test the production entry
point, request and observable result. Keep tests deterministic and offline;
real Atlassian workspaces are optional manual validation. Add coverage for changed
contracts and failure paths rather than repeating implementation details.

## Module boundaries

- `src/cli.rs`: Clap command definitions, defaults and option relationships.
- `src/main.rs`: command dispatch, process output and installation orchestration.
- `src/commands/`: Jira and Confluence command execution.
- `src/client.rs`: authenticated HTTP requests, API errors and rate-limit retries.
- `src/config.rs`: profile loading, validation and per-field auth resolution.
- `src/editor.rs`: editor command parsing and literal file argument construction.
- `src/jira/api/`: issue, comment, link, worklog and agile operations.
- `src/confluence/api/`: page, content and space operations; `api.rs` owns search,
  comment traversal and shared cursor pagination.
- `src/dist/`: installation lifecycle and GitHub releases, independent of Atlassian.
- `src/markdown/`: ADF and Confluence storage conversion.

Read the relevant module guide before changing its contracts:
[authentication](src/auth/CLAUDE.md), [Jira](src/jira/CLAUDE.md),
[Confluence](src/confluence/CLAUDE.md), [distribution](src/dist/CLAUDE.md),
[Markdown](src/markdown/CLAUDE.md).

Preserve the public exports through `jira::api`, `confluence::api`, and their
parent modules. Add functionality within its owning module and share helpers
only where their contracts are the same.

## User contracts

- Auth selection is explicit: `basic`, `scoped_token`, `service_account`, `oauth`.
  For the selected profile, the highest-priority file defining an auth section
  replaces that section entirely; omitted sections preserve the earlier auth.
  Resolve each credential field in order: CLI > environment > config file.
- Configuration management honors `--config`; `--global` selects the global
  target for init, path and edit. Render resolved configuration through
  `Config::redacted_toml`, preserving TOML keys and values with masked secrets.
  File-layer operational DTOs retain field presence; only runtime settings apply
  defaults. Omitted fields inherit; explicit empty lists clear earlier lists.
  `EDITOR` uses POSIX quoting on every platform; pass an absolute config path as
  its own argument.
- Root options precede the subcommand. `--format markdown` converts content
  inside the JSON envelope. Search streaming emits JSONL without a trailing
  summary; progress and diagnostics use stderr. Retain search results only in
  non-streaming mode; streaming keeps a cumulative count and cursor history.
- Runtime failures emit one JSON error object to stderr. Clap usage errors use
  text and exit 2. Exit codes: 1 generic, 3 auth/permission, 4 missing resource,
  5 rate limit, 6 server error. Output may precede a later failure.
- Lists use an `items` envelope. Creating writes return the identifiers callers
  need; side-effect-only writes return `{}`. Extract required response fields
  with `response` helpers; malformed successful responses are errors.
- Whole-resource deletion requires `--yes` at the CLI boundary. Targeted
  sub-resource removals require an unambiguous identifier.

## HTTP and authentication

- API functions take service-relative paths and send through `ApiClient::execute`.
  Request hosts come only from the configured auth strategy. Pagination links
  contribute their path and query, never their authority.
- Percent-encode identifiers with `http_utils::encode_path_segment`; use the
  request query builder for parameter values. Validate cloud IDs before routing.
- Retry only replayable 429 responses, at most three times, refreshing
  authorization before each retry. Respect both forms of `Retry-After`; surface
  waits over 60 seconds to the caller instead of retrying early. Multipart
  request bodies are not replayed. Never automatically retry ambiguous writes.
- Pagination validates the raw collection and end signal before filtering each
  result. Use the existing Jira page contracts and Confluence cursor walker.
- Jira search uses `/rest/api/3/search/jql`. Agile uses `/rest/agile/1.0`.
  Confluence CQL search uses v1; v2 reads coexist with supported v1 label and
  attachment writes. Check endpoint support before changing versions.
- Apply the configured request timeout to API calls and OAuth token exchanges.
  Credential-bearing requests must not follow redirects.
- Search scope detection distinguishes fields from values, including quoted field
  names and logical operators; preserve the original sorting suffix. Share this
  handling through `query_utils` rather than adding endpoint-specific detection.
- Secrets must stay out of Debug output, logs, help and resolved config output.
  Preserve `SecretString`, redacted Debug implementations and serialization
  exclusions on auth configuration.
- OAuth uses PKCE S256, CSRF state validation and a literal loopback IP. Token
  persistence uses the OS keychain or an atomic restricted-permission file.
  Serialize file mutations with the persistent sidecar lock. Blocking keychain
  operations and file-lock waits run outside Tokio reactor threads.

## Releases and documentation

Text files use LF on every platform through `.gitattributes`; embedded skill and
generated reference content must remain byte-stable. Commit `Cargo.lock`; pin
GitHub Actions to complete commit SHAs. Keep the release
matrix and `dist::target` aligned. Preserve checksum verification before staging
or running a downloaded binary and verify its version before replacement.

The distributable skill is `.claude/skills/jira-confluence/SKILL.md` and is
compiled into the binary. Its version and both README release versions must
match `Cargo.toml`. Keep the README languages consistent. Put exhaustive CLI
syntax in the generated reference and task guidance in the skill. Document
external service policies with authoritative links and distinguish API-token,
OAuth app and service-account behavior.
