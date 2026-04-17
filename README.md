# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Lint](https://github.com/junyeong-ai/atlassian-cli/workflows/Lint/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.1.0-blue?style=flat-square)](https://github.com/junyeong-ai/atlassian-cli/releases)

> **🌐 한국어** | **[English](README.en.md)**

Atlassian Cloud CLI — Jira + Confluence. 단일 Rust 바이너리, OAuth 2.0 서비스 계정과 Basic (API token) 둘 다 지원.

---

## 빠른 시작

```bash
# 1. 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash

# 2. 설정 초기화
atlassian-cli config init --global

# 3. 설정 편집 (아래 "설정" 섹션 참고)
atlassian-cli config edit --global

# 4. 검증
atlassian-cli config validate

# 5. 사용
atlassian-cli jira search "project = PROJ" --limit 5
```

---

## 주요 기능

### Jira
```bash
# 읽기
atlassian-cli jira get PROJ-123 --format markdown
atlassian-cli jira search "assignee = currentUser()" --limit 10
atlassian-cli jira search "project = PROJ" --all --stream > issues.jsonl
atlassian-cli jira comments PROJ-123 --format markdown
atlassian-cli jira transitions PROJ-123

# 쓰기 (plain text는 자동으로 ADF로 변환됨)
atlassian-cli jira create PROJ "Summary" Bug --description "Plain text"
atlassian-cli jira update PROJ-123 '{"summary":"New title"}'
atlassian-cli jira comment add PROJ-123 "Comment"
atlassian-cli jira comment update PROJ-123 10042 "Edited"
atlassian-cli jira transition PROJ-123 31
```

### Confluence
```bash
# 읽기
atlassian-cli confluence search "space = TEAM" --limit 10
atlassian-cli confluence get 123456 --format markdown
atlassian-cli confluence children 123456
atlassian-cli confluence comments 123456 --format markdown

# 쓰기 (HTML storage format)
atlassian-cli confluence create TEAM "Title" "<p>Content</p>"
atlassian-cli confluence update 123456 "Title" "<p>Updated</p>"
```

### 공통
```bash
atlassian-cli --pretty jira get PROJ-123 | jq -r '.fields.summary'
atlassian-cli --profile work jira search "..."
atlassian-cli -v jira search "..."           # -v/-vv/-vvv: info/debug/trace
```

---

## 설치

**자동 설치** (권장):
```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash
```

**소스 빌드**:
```bash
git clone https://github.com/junyeong-ai/atlassian-cli
cd atlassian-cli
cargo build --release
cp target/release/atlassian-cli ~/.local/bin/
```

**지원 플랫폼**: Linux (x86_64, aarch64), macOS (Apple Silicon), Windows (x86_64).
**Requirement**: Rust 1.91.1+ (소스 빌드 시).

---

## 인증

두 가지 방식을 **명시적**으로 선택해야 합니다. 자동 판별 없음.

### OAuth 2.0 (서비스 계정 — API token이 차단된 환경에 권장)

```bash
export ATLASSIAN_AUTH_METHOD=oauth
export ATLASSIAN_CLIENT_ID="..."
export ATLASSIAN_CLIENT_SECRET="..."
# ATLASSIAN_CLOUD_ID 는 선택 (미지정 시 자동 검색)
```

또는 `~/.config/atlassian-cli/config.toml` (chmod 600 권장):
```toml
[default]
# domain은 OAuth에선 선택 사항 (cloud_id로 라우팅)

[default.auth]
method = "oauth"
client_id = "..."
client_secret = "..."
# cloud_id = "..."   # optional
```

### Basic (API token)

```bash
export ATLASSIAN_AUTH_METHOD=basic
export ATLASSIAN_DOMAIN=company.atlassian.net
export ATLASSIAN_EMAIL=user@example.com
export ATLASSIAN_API_TOKEN=...
```

또는:
```toml
[default]
domain = "company.atlassian.net"

[default.auth]
method = "basic"
email = "user@example.com"
token = "..."
```

API token 발급: <https://id.atlassian.com/manage-profile/security/api-tokens>

### 다중 프로파일

```toml
[default]
[default.auth]
method = "oauth"
client_id = "..."
client_secret = "..."

[work]
domain = "work.atlassian.net"
[work.auth]
method = "basic"
email = "..."
token = "..."
```

사용: `atlassian-cli --profile work ...`

### 우선순위

필드 단위로 `CLI 플래그 > 환경 변수 > 설정 파일`. `ATLASSIAN_AUTH_METHOD` 환경 변수는 설정 파일의 method를 덮어씁니다 (새 method의 자격증명이 env/CLI로 제공되어야 함).

---

## 필터 & 최적화

**프로젝트/스페이스 자동 주입**:
```toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["TEAM1"]
```

JQL `status = Open` → `project IN ("PROJ1","PROJ2") AND (status = Open)`. JQL에 이미 `project` 절이 있으면 주입을 건너뜁니다.

**성능 튜닝**:
```toml
[default.performance]
request_timeout_ms = 30000
rate_limit_delay_ms = 200
```

`REQUEST_TIMEOUT_MS` 환경 변수가 `request_timeout_ms`를 덮어씁니다.

---

## 문제 해결

**API 토큰 관리자 차단 (403)** — OAuth 서비스 계정으로 전환. 관리자에게 OAuth client credentials 요청.

**OAuth token request failed (400: invalid_client)** — `client_id`/`client_secret` 오타. `config validate`로 확인.

**Multiple Atlassian sites found** — OAuth credential이 여러 사이트 접근 권한을 가짐. `cloud_id`를 명시.

**401 Unauthorized; scope does not match** — OAuth scope 부족. 토큰 자체는 유효하니 다른 엔드포인트는 동작할 수 있음. 사용 중인 엔드포인트에 맞는 scope 부여 필요.

**Invalid Atlassian domain format** — `.atlassian.net`으로 끝나야 함. 스푸핑 방지.

**Profile 'X' not found in any loaded config file** — 모든 로드된 config 파일에 해당 profile이 없음. 파일 경로와 철자 확인.

---

## 명령어 참조

### Jira
| 명령어 | 설명 |
|--------|------|
| `get <KEY>` | 이슈 조회 |
| `search <JQL>` | JQL 검색 |
| `create <PROJECT> <SUMMARY> <TYPE>` | 이슈 생성 |
| `update <KEY> <JSON>` | 이슈 수정 |
| `comment add <KEY> <TEXT>` | 댓글 추가 |
| `comment update <KEY> <COMMENT_ID> <TEXT>` | 댓글 수정 |
| `comments <KEY>` | 댓글 목록 |
| `transitions <KEY>` | 전환 목록 |
| `transition <KEY> <ID>` | 상태 전환 |

### Confluence
| 명령어 | 설명 |
|--------|------|
| `search <CQL>` | CQL 검색 |
| `get <ID>` | 페이지 조회 |
| `create <SPACE> <TITLE> <CONTENT>` | 페이지 생성 (HTML) |
| `update <ID> <TITLE> <CONTENT>` | 페이지 수정 (HTML) |
| `children <ID>` | 하위 페이지 |
| `comments <ID>` | 댓글 조회 |

### Config
| 명령어 | 설명 |
|--------|------|
| `init [--global]` | 설정 초기화 |
| `show` | 설정 표시 (secrets 마스킹) |
| `list` | 파일 경로 + 환경변수 상태 |
| `path [--global]` | 활성 설정 파일 경로 |
| `edit [--global]` | `$EDITOR`로 편집 |
| `validate` | API 인증 end-to-end 검증 |

### 공통 옵션
| 옵션 | 설명 |
|------|------|
| `--pretty` | JSON pretty print (글로벌; 서브커맨드 앞에 위치) |
| `--profile <NAME>` | 설정 프로파일 선택 |
| `--config <PATH>` | 설정 파일 경로 오버라이드 |
| `-v` / `-vv` / `-vvv` | 로깅 레벨 (stderr) |
| `--format markdown` | ADF/HTML content 필드를 Markdown으로 변환 (JSON envelope 유지) |
| `--all` | `search`: 전체 페이지네이션 |
| `--stream` | `search --all`: JSONL을 stdout으로 |
| `--fields a,b,c` | `jira search`: 반환 필드 지정 |
| `--expand a,b` | `confluence search`: 확장 필드 |
| `--limit <N>` | `search`: 페이지 크기 |

---

## 개발자 가이드

아키텍처, auth 모델, API 버전 믹스의 이유, 새 명령 추가 워크플로는 [CLAUDE.md](CLAUDE.md) 참고.

## Claude Code Skill (선택)

설치 스크립트가 `jira-confluence` 스킬을 선택적으로 설치할 수 있습니다 (user/project/skip). Skill 문서: `~/.claude/skills/jira-confluence/SKILL.md`.

---

## 지원

- **GitHub Issues**: <https://github.com/junyeong-ai/atlassian-cli/issues>

---

<div align="center">

**🌐 한국어** | **[English](README.en.md)**

Made with ❤️ for productivity

</div>
