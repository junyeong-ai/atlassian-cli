# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Lint](https://github.com/junyeong-ai/atlassian-cli/workflows/Lint/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Version](https://img.shields.io/badge/version-0.1.0-blue?style=flat-square)](https://github.com/junyeong-ai/atlassian-cli/releases)

> **🌐 한국어** | **[English](README.en.md)**

---

> **⚡ 빠르고 강력한 Atlassian Cloud 명령줄 도구**
>
> - 🚀 **3.8MB 단일 바이너리** (별도 런타임 불필요)
> - 📊 **14개 작업** (Jira 8개 + Confluence 6개)
> - 🎯 **필드 최적화** (60-70% 응답 크기 감소)
> - 🔧 **4단계 설정** (CLI → ENV → Project → Global)

---

## ⚡ 빠른 시작 (1분)

```bash
# 1. 설치
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash

# 2. 설정 초기화
atlassian-cli config init --global

# 3. 토큰 설정
# ~/.config/atlassian-cli/config.toml 편집
# domain, email, token 입력

# 4. 사용 시작! 🎉
atlassian-cli jira search "status = Open" --limit 5
atlassian-cli confluence search "type=page AND space=TEAM"
```

**Tip**: [API Token 생성](https://id.atlassian.com/manage-profile/security/api-tokens) 필요

---

## 🎯 주요 기능

### Jira 작업
```bash
# 이슈 검색 (JQL)
atlassian-cli jira search "project = TMS AND status = Open" --limit 10
atlassian-cli jira search "assignee = currentUser() AND status != Done"

# 이슈 조회
atlassian-cli jira get PROJ-123

# 이슈 생성
atlassian-cli jira create PROJ "버그 수정" Bug --description "상세 내용"

# 이슈 수정
atlassian-cli jira update PROJ-123 '{"summary":"새 제목"}'

# 댓글 추가
atlassian-cli jira comment add PROJ-123 "작업 완료"

# 상태 전환
atlassian-cli jira transitions PROJ-123
atlassian-cli jira transition PROJ-123 31
```

### Confluence 작업
```bash
# 페이지 검색 (CQL)
atlassian-cli confluence search 'type=page AND space="TEAM"' --limit 10

# 페이지 조회
atlassian-cli confluence get 123456

# 페이지 생성
atlassian-cli confluence create TEAM "API 문서" "<p>내용</p>"

# 페이지 수정
atlassian-cli confluence update 123456 "API 문서 v2" "<p>새 내용</p>"

# 하위 페이지 목록
atlassian-cli confluence children 123456

# 댓글 조회
atlassian-cli confluence comments 123456
```

### 설정 & 최적화
```bash
# 설정 관리
atlassian-cli config show            # 설정 표시 (토큰 마스킹)
atlassian-cli config path            # 설정 파일 경로
atlassian-cli config edit            # 에디터로 수정

# 필드 최적화 (60-70% 크기 감소)
atlassian-cli jira search "project = PROJ" --fields key,summary,status
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
export JIRA_SEARCH_CUSTOM_FIELDS="customfield_10015"

# JSON 출력
atlassian-cli jira get PROJ-123 | jq -r '.fields.summary'
```

**중요 사항**:
- 필드 최적화: 기본 17개 필드 (`description`, `id`, `renderedFields` 제외)
- 프로젝트 필터: `projects_filter`로 접근 제어 가능
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

스킬을 설치하면 Claude Code에서 자연어로 Jira/Confluence 조회가 가능합니다.

---

## 🔑 API Token 생성

1. [Atlassian API Tokens](https://id.atlassian.com/manage-profile/security/api-tokens) 접속
2. "Create API token" 클릭
3. 라벨 입력 (예: "atlassian-cli")
4. 토큰 복사하여 설정 파일에 추가

**보안**: Token은 비밀번호와 동일하게 취급. 노출 시 즉시 재생성.

---

## ⚙️ 설정

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

[performance]
request_timeout_ms = 30000
```

### 환경 변수

```bash
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="your-token"

# 필드 최적화
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
export JIRA_SEARCH_CUSTOM_FIELDS="customfield_10015"
export CONFLUENCE_CUSTOM_INCLUDES="ancestors,history"
```

### 설정 우선순위

```
CLI 플래그 > 환경 변수 > 프로젝트 설정 > 전역 설정
```

**예시**:
```bash
# 설정 파일 오버라이드
atlassian-cli --domain company.atlassian.net --email user@example.com \
  jira search "status = Open"
```

---

## 🏗️ 핵심 구조

4단계 우선순위 설정, ADF 자동 변환, 필드 최적화 (17개 기본 필드).
상세한 아키텍처는 [CLAUDE.md](CLAUDE.md) 참고.

---

## 🔧 문제 해결

### 설정을 찾을 수 없음

**확인 사항**:
- [ ] 설정 파일 존재: `atlassian-cli config path`
- [ ] 설정 내용 확인: `atlassian-cli config show`
- [ ] Domain 형식: `company.atlassian.net` (https:// 없이)

**해결**:
```bash
atlassian-cli config init --global
```

### API 인증 실패

**확인 사항**:
- [ ] Email 형식 유효
- [ ] Token 정확 (복사/붙여넣기 공백 주의)
- [ ] Domain 형식 확인

**Token 테스트**: `atlassian-cli config show`로 마스킹된 토큰 확인

### 필드 필터링 안 됨

**우선순위 확인**:
1. CLI `--fields` (최우선)
2. `JIRA_SEARCH_DEFAULT_FIELDS` 환경변수
3. 기본 17개 필드 + `JIRA_SEARCH_CUSTOM_FIELDS`

```bash
# 테스트
JIRA_SEARCH_DEFAULT_FIELDS="key,summary" atlassian-cli jira search "project = PROJ"
```

### 프로젝트 접근 제한

`projects_filter` 설정 시 JQL에 자동 주입:
```
입력: status = Open
실행: project IN (PROJ1,PROJ2) AND (status = Open)
```

---

## 📚 명령어 참조

### Jira 명령어 (8개)

| 명령어 | 설명 | 예제 |
|--------|------|------|
| `get <KEY>` | 이슈 조회 | `atlassian-cli jira get PROJ-123` |
| `search <JQL>` | JQL 검색 | `atlassian-cli jira search "status = Open" --limit 10` |
| `create <PROJECT> <SUMMARY> <TYPE>` | 이슈 생성 | `atlassian-cli jira create PROJ "Title" Bug --description "Text"` |
| `update <KEY> <JSON>` | 이슈 수정 | `atlassian-cli jira update PROJ-123 '{"summary":"New"}'` |
| `comment add <KEY> <TEXT>` | 댓글 추가 | `atlassian-cli jira comment add PROJ-123 "Comment"` |
| `comment update <KEY> <ID> <TEXT>` | 댓글 수정 | `atlassian-cli jira comment update PROJ-123 123 "Updated"` |
| `transitions <KEY>` | 가능한 전환 목록 | `atlassian-cli jira transitions PROJ-123` |
| `transition <KEY> <ID>` | 상태 전환 | `atlassian-cli jira transition PROJ-123 31` |

### Confluence 명령어 (6개)

| 명령어 | 설명 | 예제 |
|--------|------|------|
| `search <CQL>` | CQL 검색 | `atlassian-cli confluence search "type=page" --limit 10` |
| `get <ID>` | 페이지 조회 | `atlassian-cli confluence get 123456` |
| `create <SPACE> <TITLE> <CONTENT>` | 페이지 생성 | `atlassian-cli confluence create TEAM "Title" "<p>HTML</p>"` |
| `update <ID> <TITLE> <CONTENT>` | 페이지 수정 | `atlassian-cli confluence update 123456 "Title" "<p>HTML</p>"` |
| `children <ID>` | 하위 페이지 목록 | `atlassian-cli confluence children 123456` |
| `comments <ID>` | 댓글 조회 | `atlassian-cli confluence comments 123456` |

### Config 명령어 (5개)

| 명령어 | 설명 | 예제 |
|--------|------|------|
| `init [--global]` | 설정 초기화 | `atlassian-cli config init --global` |
| `show` | 설정 표시 (토큰 마스킹) | `atlassian-cli config show` |
| `list` | 설정 위치 나열 | `atlassian-cli config list` |
| `path [--global]` | 설정 파일 경로 | `atlassian-cli config path` |
| `edit [--global]` | 에디터로 수정 | `atlassian-cli config edit` |

### 공통 옵션

| 옵션 | 설명 | 적용 범위 |
|------|------|-----------|
| `--domain <DOMAIN>` | Domain 오버라이드 | 모든 명령어 |
| `--email <EMAIL>` | Email 오버라이드 | 모든 명령어 |
| `--token <TOKEN>` | Token 오버라이드 | 모든 명령어 |
| `--profile <NAME>` | 프로필 선택 | 모든 명령어 |
| `--fields <FIELDS>` | 필드 지정 (쉼표 구분) | jira search, jira get |
| `--limit <N>` | 결과 개수 제한 | jira search, confluence search |
| `--description <TEXT>` | 설명 (ADF 자동 변환) | jira create, jira update |

**참고**:
- Domain 형식: `company.atlassian.net` (https:// 없이)
- ADF 자동 변환: 일반 텍스트 → JSON ADF
- 필드 최적화: 기본 17개 필드 (`key,summary,status,...`)

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
