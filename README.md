# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/ci.yml)
[![Security](https://github.com/junyeong-ai/atlassian-cli/workflows/Security/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions/workflows/security.yml)
[![Rust](https://img.shields.io/badge/rust-1.98.0%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.14.0-blue?style=flat-square)](https://github.com/junyeong-ai/atlassian-cli/releases)
[![DeepWiki](https://img.shields.io/badge/DeepWiki-junyeong--ai%2Fatlassian--cli-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/junyeong-ai/atlassian-cli)

> **🌐 한국어** | **[English](README.en.md)**

Jira와 Confluence Cloud를 다루는 단일 Rust 바이너리입니다. 사람뿐 아니라 AI 에이전트가 직접 다루도록 설계했습니다. Jira와 Confluence 명령은 JSON을 출력하고, 서식 있는 본문은 Markdown으로 읽고 쓰며, 긴 본문은 표준 입력에서 받고, 실패는 안정된 종료 코드와 함께 JSON 객체 하나로 보고합니다. `jira-confluence` Claude Code 스킬을 함께 배포하며, OAuth 2.0 (3LO), 서비스 계정, scoped API 토큰, classic API 토큰의 네 가지 인증 방식을 지원합니다.

---

## 빠른 시작

```bash
# 1. 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash
export PATH="${INSTALL_DIR:-$HOME/.local/bin}:$PATH"

# 2. 설정을 만들고 인증 방식을 고릅니다 ("인증" 참고)
atlassian-cli config init --global
atlassian-cli config edit --global

# 3. 로그인합니다 (oauth만 해당, 브라우저가 없으면 --no-browser)
atlassian-cli auth login

# 4. 자격 증명을 확인하고 첫 검색을 실행합니다
atlassian-cli config validate
atlassian-cli jira search "assignee = currentUser()" --limit 5
```

---

## 사용법

### Jira

```bash
# 읽기
atlassian-cli jira get PROJ-123 --format markdown
atlassian-cli jira search "assignee = currentUser() AND statusCategory != Done" --format markdown --limit 20
atlassian-cli jira search "project = PROJ" --all --stream > issues.jsonl
atlassian-cli jira comment list PROJ-123 --format markdown

# 쓰기 — Markdown은 ADF로 변환되고, `-`는 본문을 표준 입력에서 읽습니다
atlassian-cli jira create PROJ "Safari에서 로그인 실패" Bug --format markdown --description - < report.md
atlassian-cli jira comment add PROJ-123 "**staging**에서 재현" --format markdown
atlassian-cli jira user search "Kim"                  # 담당자 필드·JQL·CQL에 쓰는 accountId
atlassian-cli jira update PROJ-123 '{"assignee": {"accountId": "5b10a2844c20165700ede21g"}}'
atlassian-cli jira transition list PROJ-123
atlassian-cli jira transition apply PROJ-123 31
atlassian-cli jira delete PROJ-123 --yes               # 영구 삭제

# 링크·작업 시간·와처·애자일
atlassian-cli jira link add PROJ-1 PROJ-2 --type Blocks
atlassian-cli jira worklog add PROJ-123 "2h 30m" --comment "조사"
atlassian-cli jira watcher add PROJ-123
atlassian-cli jira sprint list --project PROJ
atlassian-cli jira sprint move 55 PROJ-1 PROJ-2
atlassian-cli jira epic assign EPIC-1 PROJ-1 PROJ-2    # 각 이슈의 parent를 설정
```

### Confluence

```bash
# 읽기
atlassian-cli confluence search "space = TEAM AND type = page" --format markdown --limit 10
atlassian-cli confluence get 123456 --format markdown
atlassian-cli confluence children 123456               # 페이지·폴더·화이트보드·데이터베이스·임베드
atlassian-cli confluence comment list 123456 --format markdown

# 쓰기 — Markdown은 storage format으로 변환됩니다
atlassian-cli confluence create TEAM "설계 노트" - --format markdown --parent 123456 < notes.md
atlassian-cli confluence update 123456 "설계 노트" - --format markdown --base-version 7 < notes.md
atlassian-cli confluence comment add 123456 "**좋습니다**" --format markdown
atlassian-cli confluence attachment upload 123456 ./diagram.png
atlassian-cli confluence label add 123456 needs-review
atlassian-cli confluence property set 123456 review '{"status":"done"}'
atlassian-cli confluence delete 123456 --yes           # 페이지를 휴지통으로 이동
```

`--base-version`에는 읽어 온 `version.number`를 넣습니다. 그 버전 이후 다른 사람이 저장했다면 수정은 그 변경을 덮어쓰지 않고 409로 거부됩니다.

### 스크립트와 에이전트를 위한 출력

- `jira`, `confluence`, `self` 명령의 결과는 stdout에 JSON으로 출력되고, 목록은 `items`로 감쌉니다. `--pretty`, `--profile`, `--config` 같은 루트 옵션은 하위 명령 앞에 둡니다.
- `--format`은 서식 있는 본문의 표현을 정합니다. 기본값은 Jira `adf`, Confluence `storage`이며, `markdown`은 읽을 때 JSON 출력 안의 본문 필드를 변환하고 쓸 때 본문을 변환해 보냅니다. `html`은 기본값의 별칭으로 남아 있습니다.
- 본문 인자에 `-`를 주면 표준 입력에서 읽으며, 빈 입력은 거부합니다. 구분자를 따옴표로 감싼 heredoc(`<<'EOF'`)은 이스케이프가 필요 없고 인자 크기 제한도 받지 않습니다.
- `--all`은 검색의 모든 페이지를 가져오고, `--all --stream`은 페이지가 도착하는 대로 JSONL을 쓰며 진행 상황을 stderr에 보고합니다.
- Markdown 쓰기는 GitHub 표와 취소선을 포함한 CommonMark를 따릅니다. ADF는 목록 항목이나 인용 안에 제목·인용·표·구분선을 담을 수 없으므로, 그런 Markdown은 위치를 밝혀 거부합니다. 원시 HTML은 글자 그대로 남으며, Confluence 매크로는 `--format storage`로 씁니다.

모든 명령·인자·기본값은 생성된 [CLI 참조](docs/cli.md)에 있고, 설치된 버전의 설명은 `atlassian-cli <명령> --help`로 확인합니다.

```bash
atlassian-cli completions zsh > "${fpath[1]}/_atlassian-cli"   # bash, fish, elvish, powershell도 지원
```

---

## 설치

```bash
# 최신 릴리스. 함께 들어 있는 스킬을 ~/.claude/skills에 배포합니다
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash

# 특정 릴리스나 다른 디렉터리
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | ATLASSIAN_CLI_VERSION=v0.14.0 bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | INSTALL_DIR="$HOME/bin" bash
```

**수동 설치.** [Releases](https://github.com/junyeong-ai/atlassian-cli/releases)에서 플랫폼에 맞는 압축 파일과 `.sha256`을 받아 `sha256sum -c`(Linux)나 `shasum -a 256 -c`(macOS)로 검증하고, `tar -xzf`로 푼 `atlassian-cli`를 `PATH`에 있는 디렉터리로 옮깁니다. Windows에서는 릴리스의 `.zip`에서 `atlassian-cli.exe`를 꺼냅니다. 스킬은 `atlassian-cli self skill install`로 배포합니다.

**소스 빌드.** Rust 1.98.0 이상이 필요하며, 개발에 쓰는 도구체인은 `rust-toolchain.toml`이 고정합니다.

```bash
git clone https://github.com/junyeong-ai/atlassian-cli
cd atlassian-cli
cargo install --path . --locked --root "$HOME/.local" --target "$(rustc --print host-tuple)"
```

다른 방식으로 설치한 바이너리를 교체하려면 `--force`를 더합니다.

릴리스는 Linux(x86_64, aarch64), macOS(Intel, Apple Silicon), Windows(x86_64)용으로 나옵니다. `install.sh`는 Linux와 macOS에서 동작합니다. 모든 플랫폼이 `.tar.gz`를 발행하므로 `self update`는 어디서나 동작합니다.

### 설치 관리

```bash
atlassian-cli self status                          # 버전·경로·스킬 상태·토큰을 가진 프로파일 (네트워크 없음)
atlassian-cli self update                          # 최신 릴리스로 교체
atlassian-cli self update --version 0.13.9         # 특정 릴리스. 이전 버전으로 돌아가려면 버전을 명시
atlassian-cli self update --verify-attestations    # GitHub 빌드 출처 증명까지 검증 (gh CLI 필요)
atlassian-cli self update --force                  # 이미 그 버전을 보고하는 바이너리도 교체
atlassian-cli self skill install                   # 이 바이너리에 든 스킬을 다시 배포
atlassian-cli self skill remove --yes
atlassian-cli self uninstall --yes [--keep-skill] [--keep-credentials] [--purge-config]
```

- `self update`는 교체하기 전에 내려받은 바이너리를 실행해 보고하는 버전을 확인합니다. 이 기계에서 실행되지 않는 바이너리라면 설치는 그대로 남습니다.
- 스킬은 바이너리에 컴파일되어 있습니다. `self status`는 배포된 사본을 바이트 단위로 비교해, 직접 수정했거나 다운그레이드로 달라진 사본을 `stale`로 보고합니다.
- `self uninstall`은 `--keep-credentials`가 없으면 OS 키체인의 OAuth 토큰도 지웁니다. 키체인이 있는데 읽을 수 없으면 거부합니다. 토큰이 남았는데 그 위치를 아는 도구가 사라지는 것이 가장 나쁘기 때문입니다. 키체인이 응답하지 않으면 첫 실행은 `credentials.json`을 지우고 무엇을 지웠는지 밝히며 거부하고, `--keep-credentials`를 준 두 번째 실행이 마무리합니다.
- `auth logout`도 같은 이유로, 응답하지 않는 키체인에 대해서는 0이 아닌 코드로 끝납니다. `ATLASSIAN_NO_KEYCHAIN`이 설정되어 있으면 0으로 끝나며 키체인을 건드리지 않았다고 알립니다.
- `--purge-config`는 이 도구가 쓴 설정 파일을 지우고, 그 결과 디렉터리가 비었을 때만 디렉터리도 지웁니다. 프로젝트의 `.atlassian.toml`은 건드리지 않습니다.

---

## 인증

방식은 명시적으로 고르며, 추측하지 않습니다.

| 방식 | 행위자 | 요청 호스트 | 설명 |
|---|---|---|---|
| `oauth` ⭐ | 로그인한 사용자 | `api.atlassian.com` | 브라우저 로그인. 토큰은 OS 키체인에 보관하고 자동 갱신 |
| `service_account` | 서비스 계정 | `api.atlassian.com` | 자동화와 CI를 위한 OAuth 2.0 client credentials |
| `scoped_token` | 토큰 소유자 | `api.atlassian.com` | scope가 **있는** API 토큰. 그 scope로 권한이 제한됨 |
| `basic` | 토큰 소유자 | `{domain}` | scope가 **없는** classic API 토큰 |

두 토큰 방식은 서로의 토큰을 받지 않습니다. 사이트 호스트는 scope가 있는 토큰에 401을 돌려주고, 게이트웨이는 classic 토큰을 거부합니다. 401에는 만료, 방식 불일치, scope 부족처럼 확인할 원인을 알려 주는 `hint`가 붙습니다. 두 종류 모두 <https://id.atlassian.com/manage-profile/security/api-tokens>에서 만듭니다. Atlassian은 scoped 토큰을 권장하며, 2024-12-15 이전에 발급된 토큰은 2026-03-14부터 2026-05-12 사이에 만료되었습니다([토큰 관리](https://support.atlassian.com/atlassian-account/docs/manage-api-tokens-for-your-atlassian-account/)).

### OAuth 2.0 (3LO) — 권장

```toml
# ~/.config/atlassian-cli/config.toml
[default.auth]
method = "oauth"
client_id = "..."          # developer.atlassian.com에서 발급
client_secret = "..."      # ATLASSIAN_CLIENT_SECRET 환경 변수 권장
redirect_port = 8976       # 앱의 callback URL과 일치해야 함
# scopes 기본값: ["read:jira-user", "read:jira-work", "write:jira-work", "offline_access"]
# cloud_id = "..."         # 계정이 여러 사이트에 접근할 때 하나로 고정
```

<https://developer.atlassian.com/console/myapps/>에서 OAuth 2.0 (3LO) 앱을 만들고, callback URL로 `http://127.0.0.1:8976/callback`을 등록하고, 프로파일이 요청할 scope를 부여한 뒤 client ID와 secret을 복사합니다. 앱이 부여하지 않은 scope는 동의 단계에서 거부됩니다.

```bash
atlassian-cli auth login       # 브라우저를 엽니다. --no-browser면 URL을 출력
atlassian-cli auth status      # 만료·scope·저장소
atlassian-cli auth refresh     # 강제 갱신
atlassian-cli auth logout      # 저장된 세션 삭제
```

`auth login`은 브라우저 로그인을 최대 5분 기다립니다. `--no-browser`는 URL을 출력할 뿐이며, 로그인을 마친 브라우저는 CLI가 실행되는 기기의 `127.0.0.1:8976`(프로파일의 `redirect_port`)에 접속할 수 있어야 합니다. SSH로 접속한 원격 기기에서는 `ssh -L 8976:127.0.0.1:8976 <host>`로 포트를 전달한 뒤 로그인합니다.

refresh token이 있는 세션은 만료 5분 전에 갱신됩니다. 프로파일에는 기본 Jira 권한 외에 사용할 명령에 필요한 scope만 추가합니다.

| 명령 | 추가 scope | API |
|---|---|---|
| `board list`, `sprint list --project` | `read:board-scope:jira-software`, `read:project:jira` | [Board](https://developer.atlassian.com/cloud/jira/software/rest/api-group-board/) |
| `sprint list` | `read:sprint:jira-software` | [Board sprints](https://developer.atlassian.com/cloud/jira/software/rest/api-group-board/#api-rest-agile-1-0-board-boardid-sprint-get) |
| `sprint move` | `write:sprint:jira-software` | [Sprint](https://developer.atlassian.com/cloud/jira/software/rest/api-group-sprint/) |
| `sprint backlog` | `write:board-scope:jira-software` | [Backlog](https://developer.atlassian.com/cloud/jira/software/rest/api-group-backlog/) |
| `confluence search` | `search:confluence`, `read:confluence-content.all` | [Search](https://developer.atlassian.com/cloud/confluence/rest/v1/api-group-search/) |
| `confluence get`, `confluence label list`, `confluence property list` | `read:page:confluence` | [Page](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-page/) |
| `confluence comment` 읽기 | `read:comment:confluence` | [Comment](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-comment/) |
| `confluence attachment list` | `read:attachment:confluence` | [Attachment](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-attachment/) |
| `confluence space` | `read:space:confluence` | [Space](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-space/) |
| `confluence children` | `read:hierarchical-content:confluence` | [Children](https://developer.atlassian.com/cloud/confluence/rest/v2/api-group-children/) |

Confluence 쓰기에는 쓰기 전에 하는 읽기까지 포함해, 호출하는 모든 엔드포인트의 scope가 필요합니다. 속성 변경에는 `read:page:confluence`와 `write:page:confluence`가, 댓글 삭제에는 `delete:comment:confluence`가 필요합니다. v1 API를 쓰는 라벨 추가·삭제에는 `write:confluence-content`가, 첨부 업로드에는 `write:confluence-file`이 필요합니다. 앱에 scope를 허용하고 프로파일에 추가한 뒤에는 `auth login`을 다시 실행합니다. 기존 토큰에는 새 scope가 붙지 않습니다.

> **헤드리스와 AI 에이전트:** 데스크톱 OS에서는 키체인이 GUI 프롬프트에서 멈출 수 있습니다. 실행 환경에 `ATLASSIAN_NO_KEYCHAIN=1`을 설정하면 0600 파일 저장소를 쓰며, 이 값은 계속 유지해야 합니다. 설정되어 있는 동안 `auth logout`은 파일 저장소만 지웁니다. 키체인으로 로그인한 적이 있다면 이 값을 설정하기 전에 `auth logout`을 한 번 실행합니다. 키체인에 남은 세션은 이 값을 해제할 때마다 다시 쓰입니다.

### 서비스 계정

```bash
export ATLASSIAN_AUTH_METHOD=service_account
export ATLASSIAN_CLIENT_ID="..."
export ATLASSIAN_CLIENT_SECRET="..."
# ATLASSIAN_CLOUD_ID는 자격 증명이 여러 사이트에 접근할 때만 필요
```

```toml
[default.auth]
method = "service_account"
client_id = "..."
client_secret = "..."
```

### Scoped API 토큰

```bash
export ATLASSIAN_AUTH_METHOD=scoped_token
export ATLASSIAN_DOMAIN="company.atlassian.net"   # cloud ID를 여기서 해석
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="..."
```

cloud ID는 자격 증명이 필요 없는 `https://{domain}/_edge/tenant_info`에서 읽으며, 이 방식이 사이트 호스트에 보내는 유일한 요청입니다. `ATLASSIAN_CLOUD_ID`나 `[auth].cloud_id`를 지정하면 이 조회를 건너뛰고 domain도 필요 없습니다. `config validate`는 Jira로 신원을 확인하므로, Confluence scope만 있는 토큰은 정상이어도 이 검사에 실패합니다. 그런 토큰은 `confluence space list`로 확인합니다.

### Classic API 토큰 (`basic`)

```toml
[default]
domain = "company.atlassian.net"

[default.auth]
method = "basic"
email = "user@example.com"
token = "..."
```

같은 값을 `ATLASSIAN_AUTH_METHOD=basic`, `ATLASSIAN_DOMAIN`, `ATLASSIAN_EMAIL`, `ATLASSIAN_API_TOKEN`으로 줄 수도 있습니다.

---

## 설정

### 파일과 프로파일

설정은 우선순위가 높은 순서로 `--config <파일>` 경로, 프로젝트 파일(작업 디렉터리에서 위로 올라가며 찾는 `.atlassian.toml` 또는 `.atlassian/config.toml`), 전역 파일 `~/.config/atlassian-cli/config.toml`에서 읽습니다. `config init --global`은 네 가지 방식을 주석으로 담은 시작 파일을 씁니다.

```toml
[default.auth]
method = "oauth"
client_id = "..."

[work]
domain = "work.atlassian.net"

[work.auth]
method = "basic"
email = "user@example.com"
token = "..."
```

프로파일은 `atlassian-cli --profile work ...`로 고릅니다. 계정·사이트·권한이 다르면 프로파일을 나눕니다.

### 우선순위

설정은 CLI 플래그 > 환경 변수 > `--config` 파일 > 프로젝트 파일 > 전역 파일 순서로 정해집니다. 생략한 항목은 다음 파일에서 상속하고, `[]`를 명시하면 상속된 목록을 비웁니다. 빈 환경 변수는 설정하지 않은 것으로 봅니다.

선택한 프로파일에 `[auth]` 섹션을 정의한 파일 가운데 우선순위가 가장 높은 파일이 섹션 전체를 정합니다. 섹션을 생략하면 하위 파일의 인증을 유지하지만, 섹션 안에서 생략한 자격 증명은 상속하지 않습니다. 각 자격 증명은 CLI 플래그 > 환경 변수 > 그 섹션 순서로 정해집니다. `ATLASSIAN_AUTH_METHOD`로 다른 방식을 고르면 파일의 자격 증명은 쓰지 않으므로, 새 방식에 필요한 값은 플래그나 환경 변수로 줍니다.

### 설정 관리

- `config path`와 `config edit`는 `--config`가 있으면 그 파일을, `--global`이 있으면 전역 파일을 대상으로 합니다.
- `atlassian-cli --config <파일> config init`은 그 경로에 시작 파일을 쓰며 기존 파일을 덮어쓰지 않습니다.
- `config list`는 읽은 파일(명시한 파일 포함)과 설정된 환경 변수를 보여 줍니다.
- `config show`는 해석된 프로파일을 비밀 값을 가린 TOML로 출력합니다.
- `EDITOR`는 모든 플랫폼에서 POSIX 인용 규칙을 따르는 실행 파일과 인자입니다(예: `code --wait`). 공백이나 역슬래시가 있는 경로는 값 안에서 따옴표로 감쌉니다: `'C:\Program Files\Microsoft VS Code\Code.exe' --wait`.

### 범위 필터와 응답 크기

```toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]       # project를 지정하지 않은 JQL에 주입
search_default_fields = ["key", "summary", "status"]
search_custom_fields = ["customfield_10015"]

[default.confluence]
spaces_filter = ["TEAM"]                    # space를 지정하지 않은 CQL에 주입

[default.performance]
request_timeout_ms = 30000
rate_limit_delay_ms = 200

[default.optimization]
response_exclude_fields = ["self", "avatarUrls", "iconUrl"]
```

`projects_filter`가 있으면 `status = Open`은 `project IN ("PROJ1","PROJ2") AND (status = Open)`으로 실행되고, 이미 project를 지정한 쿼리는 그대로 실행됩니다. `--fields`가 없으면 `jira search`는 기본 필드 17개를 요청합니다. `search_default_fields`는 이를 대체하고, `search_custom_fields`는 확장합니다. 환경 변수 `JIRA_PROJECTS_FILTER`, `CONFLUENCE_SPACES_FILTER`, `JIRA_SEARCH_DEFAULT_FIELDS`, `JIRA_SEARCH_CUSTOM_FIELDS`, `RESPONSE_EXCLUDE_FIELDS`, `REQUEST_TIMEOUT_MS`가 이 설정을 덮어씁니다.

---

## 오류와 종료 코드

실행 중 실패하면 stderr에 JSON 객체 하나를 출력하고, stdout에는 결과만 나옵니다. 알 수 없는 옵션 같은 사용법 오류는 텍스트를 출력하고 `2`로 끝납니다. 명령은 출력한 뒤에도 실패할 수 있으므로(`--stream`은 페이지가 도착하는 대로 씁니다), 출력이 있는지가 아니라 종료 코드를 확인합니다.

```json
{"error":{"message":"Failed to get issue (404 Not Found): ...","operation":"get issue","status":404}}
```

API 실패에는 `status`와 `operation`이 있고, 알려진 해결책이 있으면 `hint`가 붙습니다. 429에는 Atlassian이 보낸 경우 `retryAfterSeconds`와 `rateLimitReason`도 붙습니다. 읽기와 멱등 요청은 서버가 요청한 시간 이상 기다린 뒤 429를 최대 3번 재시도하며, 60초를 넘는 대기는 기다리지 않고 호출자에게 돌려줍니다. 그 밖의 쓰기는 이미 적용되었을 수 있으므로 바로 반환합니다. 다시 보내기 전에 적용되었는지 확인합니다. 첨부 업로드는 본문을 다시 보낼 수 없어 재시도하지 않으며, 다시 실행하면 같은 파일의 새 버전이 됩니다. 정책은 Atlassian의 [Jira](https://developer.atlassian.com/cloud/jira/platform/rate-limiting/)와 [Confluence](https://developer.atlassian.com/cloud/confluence/rate-limiting/) 속도 제한 문서에 있습니다.

| 종료 코드 | 의미 |
|---|---|
| `0` | 성공 |
| `1` | 일반 실패 |
| `2` | 사용법 오류 |
| `3` | 인증 또는 권한 (401, 403) |
| `4` | 대상 없음 (404) |
| `5` | rate limit (429) |
| `6` | 서버 오류 (5xx) |

---

## 문제 해결

- **`basic`이나 `scoped_token`에서 401** — 토큰이 만료되었거나 토큰 종류가 방식과 맞지 않습니다. scope가 있는 토큰은 `scoped_token`, classic 토큰은 `basic`을 씁니다. 자격 증명을 바꾸기 전에 `hint`를 읽습니다.
- **401 "scope does not match"** — 이 엔드포인트에 필요한 scope가 OAuth 토큰에 없습니다. 다른 명령은 계속 동작합니다. scope를 허용하고 프로파일에 추가한 뒤 `auth login`을 다시 실행합니다.
- **API 토큰에서 403** — 관리자가 API 토큰을 막았을 수 있습니다. 서비스 계정을 씁니다.
- **`Failed to resolve cloud_id`** — `_edge/tenant_info`를 읽지 못했습니다. `cloud_id`를 지정합니다.
- **서비스 계정의 `invalid_client`** — client ID나 secret이 틀렸습니다. `config validate`로 확인합니다.
- **`Multiple Atlassian sites found`** — 자격 증명이 여러 사이트에 접근합니다. `cloud_id`를 지정합니다.
- **`Invalid Atlassian domain`** — domain은 `.atlassian.net`으로 끝나야 합니다.
- **`Profile 'X' not found`** — 읽은 파일 중 그 프로파일을 정의한 곳이 없습니다. `config list`로 확인합니다.
- **검색 필드가 예상과 다름** — `--fields`가 `search_default_fields`보다, `search_default_fields`가 기본 17개와 `search_custom_fields`보다 우선합니다.

---

## 개발

개발 명령, 아키텍처, 호환성 계약은 [AGENTS.md](AGENTS.md)에 있습니다. 문제는 <https://github.com/junyeong-ai/atlassian-cli/issues>에 알려 주십시오.

---

<div align="center">

**🌐 한국어** | **[English](README.en.md)**

**Version 0.14.0** • Rust 2024 Edition

</div>
