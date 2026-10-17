## Summary

<!-- One or two sentences. What does this PR do? -->

## Motivation

<!-- Why is this change needed? Link any related issues. -->

## Changes

<!-- Bullet the meaningful changes. Skip cosmetic ones. -->

## Test plan

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- [ ] `cargo nextest run --workspace --all-features --locked`
- [ ] `cargo test --workspace --all-features --doc --locked`
- [ ] `cargo deny check advisories bans licenses sources` (dependency changes)
- [ ] CLI reference regenerated when commands change
- [ ] Verified against a real Atlassian Cloud workspace (if applicable)

## Auth / config impact

<!-- Does this touch AuthResolver, secret handling, config schema, or the
     JQL/CQL filter injection? Call it out explicitly. Otherwise: "None." -->

## User-visible changes

<!-- Any change a downstream user would notice: CLI flag, output format,
     config field, default behaviour. Otherwise: "None." -->
