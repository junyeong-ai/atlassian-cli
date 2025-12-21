# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Lint](https://github.com/junyeong-ai/atlassian-cli/workflows/Lint/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.1.0-blue?style=flat-square)](https://github.com/junyeong-ai/atlassian-cli/releases)
[![DeepWiki](https://img.shields.io/badge/DeepWiki-junyeong--ai%2Fatlassian--cli-blue.svg?logo=data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAACwAAAAyCAYAAAAnWDnqAAAAAXNSR0IArs4c6QAAA05JREFUaEPtmUtyEzEQhtWTQyQLHNak2AB7ZnyXZMEjXMGeK/AIi+QuHrMnbChYY7MIh8g01fJoopFb0uhhEqqcbWTp06/uv1saEDv4O3n3dV60RfP947Mm9/SQc0ICFQgzfc4CYZoTPAswgSJCCUJUnAAoRHOAUOcATwbmVLWdGoH//PB8mnKqScAhsD0kYP3j/Yt5LPQe2KvcXmGvRHcDnpxfL2zOYJ1mFwrryWTz0advv1Ut4CJgf5uhDuDj5eUcAUoahrdY/56ebRWeraTjMt/00Sh3UDtjgHtQNHwcRGOC98BJEAEymycmYcWwOprTgcB6VZ5JK5TAJ+fXGLBm3FDAmn6oPPjR4rKCAoJCal2eAiQp2x0vxTPB3ALO2CRkwmDy5WohzBDwSEFKRwPbknEggCPB/imwrycgxX2NzoMCHhPkDwqYMr9tRcP5qNrMZHkVnOjRMWwLCcr8ohBVb1OMjxLwGCvjTikrsBOiA6fNyCrm8V1rP93iVPpwaE+gO0SsWmPiXB+jikdf6SizrT5qKasx5j8ABbHpFTx+vFXp9EnYQmLx02h1QTTrl6eDqxLnGjporxl3NL3agEvXdT0WmEost648sQOYAeJS9Q7bfUVoMGnjo4AZdUMQku50McDcMWcBPvr0SzbTAFDfvJqwLzgxwATnCgnp4wDl6Aa+Ax283gghmj+vj7feE2KBBRMW3FzOpLOADl0Isb5587h/U4gGvkt5v60Z1VLG8BhYjbzRwyQZemwAd6cCR5/XFWLYZRIMpX39AR0tjaGGiGzLVyhse5C9RKC6ai42ppWPKiBagOvaYk8lO7DajerabOZP46Lby5wKjw1HCRx7p9sVMOWGzb/vA1hwiWc6jm3MvQDTogQkiqIhJV0nBQBTU+3okKCFDy9WwferkHjtxib7t3xIUQtHxnIwtx4mpg26/HfwVNVDb4oI9RHmx5WGelRVlrtiw43zboCLaxv46AZeB3IlTkwouebTr1y2NjSpHz68WNFjHvupy3q8TFn3Hos2IAk4Ju5dCo8B3wP7VPr/FGaKiG+T+v+TQqIrOqMTL1VdWV1DdmcbO8KXBz6esmYWYKPwDL5b5FA1a0hwapHiom0r/cKaoqr+27/XcrS5UwSMbQAAAABJRU5ErkJggg==)](https://deepwiki.com/junyeong-ai/atlassian-cli)

> **🌐 한국어** | **[English](README.en.md)**

---

> **⚡ 빠르고 강력한 Atlassian Cloud 명령줄 도구**
>
> - 🚀 **단일 바이너리** (런타임 불필요)
> - 🎯 **60-70% 응답 최적화** (필드 필터링)
> - 📄 **전체 페이지네이션** (`--all`로 모든 결과 조회)
> - 📝 **Markdown 변환** (`--format markdown`으로 HTML→Markdown)
> - 🔧 **4단계 설정** (CLI → ENV → Project → Global)

---

## ⚡ 빠른 시작 (1분)

```bash
# 1. 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash

# 2. 설정 초기화
atlassian-cli config init --global

# 3. 설정 편집 (domain, email, token 입력)
atlassian-cli config edit --global

# 4. 사용 시작! 🎉
atlassian-cli jira search "status = Open" --limit 5
atlassian-cli confluence search "type=page" --limit 10
```

**Tip**: [API Token 생성](https://id.atlassian.com/manage-profile/security/api-tokens) 필요

---

## 🎯 주요 기능

### Jira 작업
```bash
# 이슈 검색 (JQL)
atlassian-cli jira search "project = PROJ AND status = Open" --limit 10
atlassian-cli jira search "assignee = currentUser()" --fields key,summary,status
atlassian-cli jira search "status = Open" --format markdown  # ADF → Markdown 변환
atlassian-cli jira search "project = PROJ" --all             # 전체 결과 조회
atlassian-cli jira search "project = PROJ" --all --stream    # JSONL 스트리밍

# 이슈 조회/생성/수정
atlassian-cli jira get PROJ-123
atlassian-cli jira get PROJ-123 --format markdown  # description을 Markdown으로
atlassian-cli jira create PROJ "버그 수정" Bug --description "상세 내용"
atlassian-cli jira update PROJ-123 '{"summary":"새 제목"}'

# 댓글/상태 전환
atlassian-cli jira comment add PROJ-123 "작업 완료"
atlassian-cli jira transitions PROJ-123
atlassian-cli jira transition PROJ-123 31
```

### Confluence 작업
```bash
# 페이지 검색 (CQL)
atlassian-cli confluence search "type=page AND space=TEAM" --limit 10
atlassian-cli confluence search "type=page" --all           # 전체 결과 조회
atlassian-cli confluence search "type=page" --all --stream  # JSONL 스트리밍
atlassian-cli confluence search "type=page" --format markdown  # Markdown 변환 (body 기본 포함)

# 페이지 조회 (Markdown 변환)
atlassian-cli confluence get 123456 --format markdown

# 페이지 조회/생성/수정
atlassian-cli confluence get 123456                          # HTML 형식 (기본)
atlassian-cli confluence get 123456 --format markdown        # Markdown 변환
atlassian-cli confluence create TEAM "API 문서" "<p>내용</p>"
atlassian-cli confluence update 123456 "새 제목" "<p>새 내용</p>"

# 하위 페이지/댓글
atlassian-cli confluence children 123456
atlassian-cli confluence comments 123456 --format markdown
```

### 설정 & 최적화
```bash
# 설정 관리
atlassian-cli config show            # 설정 표시 (토큰 마스킹)
atlassian-cli config path            # 설정 파일 경로
atlassian-cli config edit            # 에디터로 수정

# JSON 출력
atlassian-cli jira get PROJ-123 | jq -r '.fields.summary'
```

**중요 사항**:
- 필드 최적화: 기본 17개 필드 (`description`, `id`, `renderedFields` 제외)
- 프로젝트 필터: `projects_filter`로 JQL 자동 주입
- ADF 자동 변환: 일반 텍스트 → Atlassian Document Format

---

## 📦 설치

### 방법 1: Prebuilt Binary (권장) ⭐

**자동 설치**:
```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash
```

**수동 설치**:
1. [Releases](https://github.com/junyeong-ai/atlassian-cli/releases)에서 바이너리 다운로드
2. 압축 해제: `tar -xzf atlassian-cli-*.tar.gz`
3. PATH에 이동: `mv atlassian-cli ~/.local/bin/`

**지원 플랫폼**:
- Linux: x86_64, aarch64
- macOS: Intel (x86_64), Apple Silicon (aarch64)
- Windows: x86_64

### 방법 2: 소스 빌드

```bash
git clone https://github.com/junyeong-ai/atlassian-cli
cd atlassian-cli
cargo build --release
cp target/release/atlassian-cli ~/.local/bin/
```

**Requirements**: Rust 1.91.1+

### 🤖 Claude Code Skill (선택사항)

`./scripts/install.sh` 실행 시 Claude Code 스킬 설치 여부를 선택할 수 있습니다:

- **User-level** (권장): 모든 프로젝트에서 사용 가능
- **Project-level**: Git을 통해 팀 자동 배포
- **Skip**: 나중에 수동 설치

---

## 🔑 API Token 생성

1. [Atlassian API Tokens](https://id.atlassian.com/manage-profile/security/api-tokens) 접속
2. "Create API token" 클릭
3. 라벨 입력 (예: "atlassian-cli")
4. 토큰 복사하여 설정에 추가

**보안**: Token은 비밀번호와 동일하게 취급. 노출 시 즉시 재생성.

---

## ⚙️ 설정

### 환경 변수

```bash
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="your-token"

# 필드 최적화
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
export JIRA_SEARCH_CUSTOM_FIELDS="customfield_10015"
```

### 설정 파일

**위치**:
- macOS/Linux: `~/.config/atlassian-cli/config.toml`
- Windows: `%APPDATA%\atlassian-cli\config.toml`
- Project: `./.atlassian.toml`

**기본 설정** (`atlassian-cli config init`로 생성):
```toml
[default]
domain = "company.atlassian.net"
email = "user@example.com"
token = "your-api-token"

[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["TEAM", "DOCS"]

[default.performance]
request_timeout_ms = 30000
rate_limit_delay_ms = 200
```

### 설정 우선순위

```
CLI 플래그 > 환경 변수 > 프로젝트 설정 > 전역 설정
```

---

## 🏗️ 핵심 구조

4단계 설정 우선순위, ADF 자동 변환, 필드 최적화 (17개 기본 필드), 커서 기반 페이지네이션.
상세한 아키텍처는 [CLAUDE.md](CLAUDE.md) 참고.

---

## 🔧 문제 해결

### 설정을 찾을 수 없음

```bash
# 설정 확인
atlassian-cli config path
atlassian-cli config show

# 재초기화
atlassian-cli config init --global
```

### API 인증 실패

**확인 사항**:
- [ ] Domain 형식: `company.atlassian.net` (https:// 없이)
- [ ] Email 형식 유효
- [ ] Token 정확 (복사/붙여넣기 공백 주의)

### 필드 필터링 안 됨

**우선순위 확인**:
1. CLI `--fields` (최우선)
2. `JIRA_SEARCH_DEFAULT_FIELDS` 환경변수
3. 기본 17개 필드 + `JIRA_SEARCH_CUSTOM_FIELDS`

```bash
# 테스트
JIRA_SEARCH_DEFAULT_FIELDS="key,summary" atlassian-cli jira search "project = PROJ"
```

### 프로젝트 필터 자동 주입

`projects_filter` 설정 시 JQL에 자동 주입:
```
입력: status = Open
실행: project IN (PROJ1,PROJ2) AND (status = Open)
```

---

## 📚 명령어 참조

### Jira 명령어

| 명령어 | 설명 | 예제 |
|--------|------|------|
| `get <KEY>` | 이슈 조회 | `jira get PROJ-123` |
| `get <KEY> --format markdown` | 이슈 조회 (Markdown) | `jira get PROJ-123 --format markdown` |
| `search <JQL>` | JQL 검색 | `jira search "status = Open" --limit 10` |
| `search <JQL> --all` | 전체 결과 조회 | `jira search "project = PROJ" --all` |
| `search <JQL> --all --stream` | JSONL 스트리밍 | `jira search "project = PROJ" --all --stream` |
| `search <JQL> --format markdown` | JQL 검색 (Markdown) | `jira search "status = Open" --format markdown` |
| `create <PROJECT> <SUMMARY> <TYPE>` | 이슈 생성 | `jira create PROJ "Title" Bug` |
| `update <KEY> <JSON>` | 이슈 수정 | `jira update PROJ-123 '{"summary":"New"}'` |
| `comment add <KEY> <TEXT>` | 댓글 추가 | `jira comment add PROJ-123 "Done"` |
| `transitions <KEY>` | 전환 목록 | `jira transitions PROJ-123` |
| `transition <KEY> <ID>` | 상태 전환 | `jira transition PROJ-123 31` |

### Confluence 명령어

| 명령어 | 설명 | 예제 |
|--------|------|------|
| `search <CQL>` | CQL 검색 | `confluence search "type=page" --limit 10` |
| `search <CQL> --format markdown` | CQL 검색 (Markdown) | `confluence search "type=page" --format markdown` |
| `get <ID>` | 페이지 조회 | `confluence get 123456` |
| `get <ID> --format markdown` | 페이지 조회 (Markdown) | `confluence get 123456 --format markdown` |
| `create <SPACE> <TITLE> <CONTENT>` | 페이지 생성 | `confluence create TEAM "Title" "<p>HTML</p>"` |
| `update <ID> <TITLE> <CONTENT>` | 페이지 수정 | `confluence update 123456 "Title" "<p>HTML</p>"` |
| `children <ID>` | 하위 페이지 | `confluence children 123456` |
| `comments <ID>` | 댓글 조회 | `confluence comments 123456` |
| `comments <ID> --format markdown` | 댓글 조회 (Markdown) | `confluence comments 123456 --format markdown` |

### Config 명령어

| 명령어 | 설명 | 예제 |
|--------|------|------|
| `init [--global]` | 설정 초기화 | `config init --global` |
| `show` | 설정 표시 | `config show` |
| `edit [--global]` | 에디터로 수정 | `config edit` |
| `path [--global]` | 파일 경로 | `config path` |
| `list` | 위치 나열 | `config list` |
| `validate` | API 연결 검증 | `config validate` |

### 공통 옵션

| 옵션 | 설명 | 적용 범위 |
|------|------|-----------|
| `--domain` | Domain 오버라이드 | 모든 명령어 |
| `--email` | Email 오버라이드 | 모든 명령어 |
| `--token` | Token 오버라이드 | 모든 명령어 |
| `--limit <N>` | 결과 개수 제한 | search |
| `--all` | 전체 결과 (페이지네이션) | jira search, confluence search |
| `--stream` | JSONL 스트리밍 | jira search, confluence search (--all 필요) |
| `--expand` | 추가 확장 필드 (ancestors 등, body.storage는 기본 포함) | confluence search |
| `--format` | 출력 형식 (html, markdown) | jira get/search, confluence search/get/comments |
| `--fields` | 필드 지정 | jira search, jira get |

---

## 🚀 개발자 가이드

**아키텍처, 디버깅, 기여 방법**: [CLAUDE.md](CLAUDE.md) 참고

---

## 💬 지원

- **GitHub Issues**: [문제 신고](https://github.com/junyeong-ai/atlassian-cli/issues)
- **개발자 문서**: [CLAUDE.md](CLAUDE.md)

---

<div align="center">

**🌐 한국어** | **[English](README.en.md)**

**Version 0.1.0** • Rust 2024 Edition

Made with ❤️ for productivity

</div>
