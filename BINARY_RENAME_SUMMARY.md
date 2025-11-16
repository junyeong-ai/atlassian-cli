# Binary Rename: atlassian → atlassian-cli

Complete transformation to `atlassian-cli` as if designed from the beginning.

## Changes Summary

### Core Configuration
- **Cargo.toml**: Binary name `atlassian-cli`
- **src/main.rs**: Command name `atlassian-cli`
- **src/config.rs**: Error messages updated

### CI/CD Workflows
- **.github/workflows/ci.yml**: Binary paths `target/release/atlassian-cli`
- **.github/workflows/release.yml**: Archive names `atlassian-cli-v${version}-${target}.tar.gz`

### Installation Scripts
- **scripts/install.sh**: `BINARY_NAME="atlassian-cli"`, archive pattern updated
- **scripts/uninstall.sh**: `BINARY_NAME="atlassian-cli"`, legacy path removed

### Documentation
- **README.md**: All commands → `atlassian-cli`
- **README.en.md**: All commands → `atlassian-cli`
- **CLAUDE.md**: All commands → `atlassian-cli`
- **.claude/skills/jira-confluence/SKILL.md**: All commands → `atlassian-cli`

### Build Artifacts
- Old binary removed
- Clean rebuild completed
- Tests: 120 passed

## Verification

```bash
$ ./target/release/atlassian-cli --version
atlassian-cli 0.1.0

$ ./target/release/atlassian-cli --help
Usage: atlassian-cli [OPTIONS] <COMMAND>

$ cargo test
120 passed; 0 failed

$ ls -lh target/release/atlassian-cli
3.8M atlassian-cli
```

## No Legacy References

All occurrences of standalone `atlassian` command changed to `atlassian-cli`.
Only legitimate references remain:
- `atlassian.net` (Atlassian domain)
- `Atlassian` (company/product name)
- `atlassian-cli` (our binary)
