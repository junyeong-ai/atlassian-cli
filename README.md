# Atlassian CLI

[![CI](https://github.com/junyeong-ai/atlassian-cli/workflows/CI/badge.svg)](https://github.com/junyeong-ai/atlassian-cli/actions)
[![Rust](https://img.shields.io/badge/rust-1.91.1%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)

**[English](README.en.md)** | 한국어

Jira와 Confluence를 터미널에서 빠르게 사용하세요.

## 특징

- **단일 바이너리** — 런타임 없이 바로 실행
- **60-70% 응답 최적화** — 필요한 필드만 가져오기
- **전체 페이지네이션** — `--all` 옵션으로 모든 결과 조회
- **4단계 설정** — CLI > ENV > 프로젝트 > 전역

## 설치

```bash
curl -fsSL https://raw.githubusercontent.com/junyeong-ai/atlassian-cli/main/scripts/install.sh | bash
```

또는 [Releases](https://github.com/junyeong-ai/atlassian-cli/releases)에서 바이너리 다운로드

## 시작하기

```bash
# 1. 설정 초기화
atlassian-cli config init --global

# 2. 설정 편집 (~/.config/atlassian-cli/config.toml)
atlassian-cli config edit --global
```

```toml
[default]
domain = "company.atlassian.net"
email = "user@example.com"
token = "your-api-token"  # https://id.atlassian.com/manage-profile/security/api-tokens
```

## 사용법

### Jira

```bash
# 검색
atlassian-cli jira search "status = Open" --limit 10
atlassian-cli jira search "project = PROJ" --fields key,summary,status

# 조회/생성/수정
atlassian-cli jira get PROJ-123
atlassian-cli jira create PROJ "제목" Bug --description "내용"
atlassian-cli jira update PROJ-123 '{"summary":"새 제목"}'

# 댓글/상태
atlassian-cli jira comment add PROJ-123 "완료"
atlassian-cli jira transition PROJ-123 31
```

### Confluence

```bash
# 검색
atlassian-cli confluence search "type=page" --limit 10
atlassian-cli confluence search "space=TEAM" --all          # 전체 결과
atlassian-cli confluence search "space=TEAM" --all --stream # JSONL 스트리밍

# 조회/생성/수정
atlassian-cli confluence get 123456
atlassian-cli confluence create TEAM "페이지 제목" "<p>내용</p>"
atlassian-cli confluence update 123456 "새 제목" "<p>새 내용</p>"
```

### 설정 관리

```bash
atlassian-cli config show   # 현재 설정 (토큰 마스킹)
atlassian-cli config path   # 설정 파일 경로
atlassian-cli config edit   # 편집기로 열기
```

## 환경 변수

```bash
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="your-token"

# 필드 최적화
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
```

## 명령어 요약

| Jira | Confluence | Config |
|------|------------|--------|
| `get` `search` `create` `update` | `get` `search` `create` `update` | `init` `show` `edit` |
| `comment add/update` | `children` `comments` | `path` `list` |
| `transition` `transitions` | | |

### 주요 옵션

| 옵션 | 설명 |
|------|------|
| `--limit N` | 결과 개수 제한 |
| `--all` | 전체 결과 (페이지네이션) |
| `--stream` | JSONL 스트리밍 (`--all` 필요) |
| `--fields` | 필드 지정 (Jira) |

## 지원

- [Issues](https://github.com/junyeong-ai/atlassian-cli/issues)
- [개발자 가이드](CLAUDE.md)
