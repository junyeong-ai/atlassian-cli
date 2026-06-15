# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/ci.yml)
[![Security](https://github.com/junyeong-ai/atlassian-cli/workflows/Security/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/security.yml)
[![Rust](https://img.shields.io/badge/rust-1.96.0%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.5.1-blue?style=flat-square)](https://github.com/junyeong-ai/atlassian-cli/releases)

> **🌐 한국어** | **[English](README.en.md)**

Atlassian Cloud CLI — Jira + Confluence. 단일 Rust 바이너리. OAuth 2.0 (3LO, ⭐ 권장), Service Account, Basic (API token) 세 가지 인증 방식 지원.

---

## 빠른 시작

```bash
# 1. 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash

# 2. 설정 초기화
atlassian-cli config init --global

# 3. 설정 편집 — 아래 "인증" 섹션에서 oauth / service_account / basic 중 선택
atlassian-cli config edit --global

# 4. (oauth 인 경우만) 브라우저로 로그인
atlassian-cli auth login

# 5. 검증
atlassian-cli config validate

# 6. 사용
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
atlassian-cli jira comment list PROJ-123 --format markdown
atlassian-cli jira transition list PROJ-123

# 쓰기 (plain text는 자동으로 ADF로 변환됨)
atlassian-cli jira create PROJ "Summary" Bug --description "Plain text"
atlassian-cli jira update PROJ-123 '{"summary":"New title"}'
atlassian-cli jira comment add PROJ-123 "Comment"
atlassian-cli jira comment update PROJ-123 10042 "Edited"
atlassian-cli jira transition apply PROJ-123 31
atlassian-cli jira delete PROJ-123 --yes          # 영구 삭제 (--yes 필수)

# 링크 · 작업시간 · 와처
atlassian-cli jira link add PROJ-1 PROJ-2 --type Blocks
atlassian-cli jira worklog add PROJ-123 "2h 30m" --comment "조사"
atlassian-cli jira watcher add PROJ-123

# 애자일 — 보드 · 스프린트 · 에픽
atlassian-cli jira sprint list --project PROJ
atlassian-cli jira sprint move 55 PROJ-1 PROJ-2
atlassian-cli jira epic assign EPIC-1 PROJ-1
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
최신 릴리스의 사전 빌드 바이너리를 설치하고, `jira-confluence` Claude Code skill을 사용자 레벨(`~/.claude/skills`)로 설치할 수 있습니다. repo checkout 없이 실행해도 skill은 GitHub에서 직접 가져옵니다.

```bash
# 특정 릴리스 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | ATLASSIAN_CLI_VERSION=v0.5.1 bash

# 제거 (비대화형 기본값은 바이너리만 제거하고 skill/config는 보존)
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/uninstall.sh | bash

# skill과 글로벌 설정까지 제거
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/uninstall.sh | bash -s -- --yes
```

**소스 빌드**:
```bash
git clone https://github.com/junyeong-ai/atlassian-cli
cd atlassian-cli
cargo +1.96.0 build --release
cp target/release/atlassian-cli ~/.local/bin/
```

**지원 플랫폼**: Linux (x86_64, aarch64), macOS (Intel, Apple Silicon), Windows (x86_64). `install.sh` 자동 설치는 Linux/macOS용이며, Windows는 Release 바이너리를 수동 설치합니다.
**Requirement**: Rust 1.96.0+ (소스 빌드 시).

---

## 인증

세 가지 방식을 **명시적**으로 선택해야 합니다 (자동 판별 없음):

| 방식 | 행위자 | 특징 |
|---|---|---|
| `basic` | 본인 (API token 발급자) | 가장 단순. 모든 행위가 본인 이름으로 기록 |
| `service_account` | 비인간 service account | 자동화·CI 용. 별도 principal |
| `oauth` | 본인 (대화형 로그인) | 사용자 권한 그대로 + refresh token 자동 관리 |

### OAuth 2.0 (3LO — 본인 계정으로 로그인) ⭐ 권장

브라우저로 한 번 로그인하면 access/refresh 토큰이 OS keychain (또는 0600 파일) 에 저장되고 만료 5분 전에 자동 refresh 됩니다.

```toml
# ~/.config/atlassian-cli/config.toml
[default]

[default.auth]
method = "oauth"
client_id = "..."          # developer.atlassian.com 에서 발급
client_secret = "..."      # 권장: ATLASSIAN_CLIENT_SECRET env var
redirect_port = 8976       # OAuth app 에 등록한 redirect URI 와 일치 (127.0.0.1:8976/callback)
# scopes 미지정 시 기본값 = ["read:jira-user", "read:jira-work", "write:jira-work", "offline_access"]
# Confluence 도 함께 쓰려면 OAuth app 에 해당 scope 를 부여한 뒤 명시:
#   scopes = ["read:jira-user", "read:jira-work", "write:jira-work",
#             "read:confluence-content.all", "read:confluence-space.summary",
#             "write:confluence-content", "offline_access"]
# board/sprint/epic (애자일) 명령은 기본 scope 로 부족함 — Jira Software scope 추가 필요:
#   "read:board-scope:jira-software", "read:sprint:jira-software",
#   "write:sprint:jira-software", "read:epic:jira-software"
# cloud_id = "..."   # 여러 site 접근 가능할 때 1개로 고정
```

> **헤드리스 / AI 에이전트**: 데스크톱 OS 에서 키체인이 GUI 프롬프트로 블로킹될 수 있습니다. `ATLASSIAN_NO_KEYCHAIN=1` 을 설정하면 키체인을 건너뛰고 0600 파일 저장소를 사용합니다. 환경 단위 설정으로 쓰세요(켰다 껐다 X) — 플래그가 켜진 동안 `auth logout`은 파일만 지웁니다. 이전에 키체인으로 로그인한 적이 있으면 플래그 없이 `auth logout`을 한 번 실행해 키체인을 비우세요.

```bash
atlassian-cli auth login         # 브라우저 열림 → Atlassian 로그인 → 자동 토큰 저장
atlassian-cli auth status        # 만료 시간, scope, 저장 위치 확인
atlassian-cli auth refresh       # 수동 갱신 (디버깅용)
atlassian-cli auth logout        # 토큰 폐기
```

준비물 — Atlassian developer console (<https://developer.atlassian.com/console/myapps/>) 에서:
1. OAuth 2.0 (3LO) app 생성
2. Callback URL 에 `http://127.0.0.1:8976/callback` 등록 (port 는 위 config 와 일치)
3. Permissions 에서 사용할 scope 부여 (Jira API + 필요 시 Confluence API). 부여하지 않은 scope 를 config 에 적으면 사용자 동의 단계에서 거부됨.
4. Settings 에서 client_id / client_secret 복사

### Service account (자동화·CI 용)

```bash
export ATLASSIAN_AUTH_METHOD=service_account
export ATLASSIAN_CLIENT_ID="..."
export ATLASSIAN_CLIENT_SECRET="..."
# ATLASSIAN_CLOUD_ID 는 선택 (미지정 시 자동 검색)
```

또는 `~/.config/atlassian-cli/config.toml` (chmod 600 권장):
```toml
[default.auth]
method = "service_account"
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
method = "service_account"
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

**Classic ↔ Granular scope 분리**: 한 OAuth 토큰은 classic과 granular scope를 혼용할 수 없습니다(Atlassian 규칙). 두 모델을 모두 쓰려면 **모델별로 프로파일을 나누고** `--profile`로 선택하세요. `scopes`는 자유 목록이라 앱이 부여한 무엇이든 넣을 수 있습니다. 예: classic `default`(core Jira) + granular `agile`(board/sprint/epic). 각 프로파일은 자체 토큰을 저장하므로 프로파일마다 `auth login` 합니다. granular는 **완전한 세트**여야 하며(빠진 scope는 해당 명령이 401), 정확한 문자열은 developer.atlassian.com 앱 Permissions에서 복사하세요.

### 우선순위

필드 단위 우선순위는 `CLI 플래그 > 환경 변수 > --config 파일 > 프로젝트 설정 > 글로벌 설정`입니다. `ATLASSIAN_AUTH_METHOD` 환경 변수는 설정 파일의 method를 덮어씁니다 (새 method의 자격증명이 env/CLI로 제공되어야 함).

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

[default.optimization]
response_exclude_fields = ["self", "avatarUrls", "iconUrl"]
```

`REQUEST_TIMEOUT_MS` 환경 변수가 `request_timeout_ms`를, `RESPONSE_EXCLUDE_FIELDS` 환경 변수가 `response_exclude_fields`를 덮어씁니다.

---

## 문제 해결

**API 토큰 관리자 차단 (403)** — OAuth 2.0 service account로 전환. 관리자에게 OAuth 2.0 client credentials 요청.

**Service account token request failed (400: invalid_client)** — `client_id`/`client_secret` 오타. `config validate`로 확인.

**Multiple Atlassian sites found** — service account credential이 여러 사이트 접근 권한을 가짐. `cloud_id`를 명시.

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
| `delete <KEY> --yes [--delete-subtasks]` | 이슈 영구 삭제 (비가역) |
| `comment add <KEY> <TEXT>` | 댓글 추가 |
| `comment update <KEY> <COMMENT_ID> <TEXT>` | 댓글 수정 |
| `comment list <KEY>` | 댓글 목록 |
| `comment delete <KEY> <COMMENT_ID>` | 댓글 삭제 |
| `transition list <KEY>` | 전환 목록 |
| `transition apply <KEY> <ID>` | 상태 전환 |
| `link add/remove/list <KEY...>`, `link types` | 이슈 링크 |
| `worklog add/list/update/remove <KEY> ...` | 작업시간 기록 |
| `watcher add/remove/list <KEY>` | 와처 |
| `list types/priorities/statuses/labels` | 전역 메타데이터 조회 |
| `board list --project <KEY>` | 애자일 보드 목록 |
| `sprint list/move/backlog ...` | 스프린트 / 백로그 이동 |
| `epic assign/unassign <EPIC> <KEY...>` | 에픽 연결 / 해제 |

### Confluence
| 명령어 | 설명 |
|--------|------|
| `search <CQL>` | CQL 검색 |
| `get <ID>` | 페이지 조회 |
| `create <SPACE> <TITLE> <CONTENT>` | 페이지 생성 (HTML) |
| `update <ID> <TITLE> <CONTENT>` | 페이지 수정 (HTML) |
| `delete <ID> --yes` | 페이지 삭제 (휴지통) |
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
| `validate` | 인증/Cloud 접근 검증 (개별 API는 scope/권한 필요) |

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

설치 스크립트가 `jira-confluence` 스킬을 사용자 레벨(`~/.claude/skills/jira-confluence`)로 선택 설치할 수 있습니다. `curl | bash` 실행 시에는 GitHub에서 skill을 가져오고, checkout 안에서 실행하면 로컬 skill 정의를 사용합니다.

---

## 지원

- **GitHub Issues**: <https://github.com/junyeong-ai/atlassian-cli/issues>

---

<div align="center">

**🌐 한국어** | **[English](README.en.md)**

Made with ❤️ for productivity

</div>
