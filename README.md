# Atlassian CLI

> ⚡ **빠르고 강력한 Atlassian Cloud CLI 도구**

[![Rust](https://img.shields.io/badge/rust-1.91.1%2B%20(2024%20edition)-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-120%20passing-brightgreen?style=flat-square)](https://github.com/yourusername/atlassian-cli)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

**[한국어](README.md)** | **[English](README.en.md)** | **[AI Agent Guide](CLAUDE.md)**

---

## ⚡ 빠른 시작 (3단계)

```bash
# 1. 설치
git clone https://github.com/yourusername/atlassian-cli
cd atlassian-cli
./install.sh

# 2. 설정
atlassian config init --global
# ~/.config/atlassian-cli/config.toml 편집

# 3. 사용 시작!
atlassian jira search "project = TMS AND status = Open" --limit 10
atlassian confluence search "type=page AND space=TEAM"
```

---

## 🎯 왜 Atlassian CLI인가?

### 🚀 빠르고 효율적
- **3.8MB 단일 바이너리**: Rust 기반 네이티브 실행
- **즉시 실행**: 별도 런타임 불필요
- **낮은 리소스**: 메모리 효율적

### 💪 완벽한 기능
- **14개 작업**: 8개 Jira + 6개 Confluence 명령
- **JQL/CQL 지원**: 강력한 쿼리 언어
- **ADF 자동 변환**: 일반 텍스트 → Atlassian Document Format
- **필드 최적화**: 60-70% 응답 크기 감소

### 🔧 유연한 설정
- **4단계 우선순위**: CLI 플래그 → 환경변수 → 프로젝트 설정 → 전역 설정
- **멀티 프로필**: 여러 Atlassian 인스턴스 관리
- **프로젝트/스페이스 필터링**: 접근 제어

### ✅ 프로덕션 준비 완료
- **120개 테스트**: 모두 통과
- **타입 안전**: Rust의 강력한 타입 시스템
- **제로 경고**: 엄격한 코드 품질 정책

---

## 📦 설치

### 방법 1: 설치 스크립트 (권장)

```bash
git clone https://github.com/yourusername/atlassian-cli
cd atlassian-cli
cargo build --release
./install.sh
```

바이너리가 `~/.local/bin/atlassian`에 설치됩니다.

### 방법 2: Cargo

```bash
cargo install --path .
```

### 방법 3: 수동 빌드

```bash
cargo build --release
# 바이너리: target/release/atlassian
```

### 제거

```bash
./uninstall.sh
```

---

## ⚙️ 설정

### 빠른 설정

```bash
# 전역 설정 초기화
atlassian config init --global

# 설정 파일 편집
atlassian config edit --global
```

### 설정 파일 위치

- **전역**: `~/.config/atlassian-cli/config.toml`
- **프로젝트**: `./.atlassian.toml`

### 기본 설정

```toml
[default]
domain = "company.atlassian.net"
email = "user@example.com"
token = "your-api-token"

[default.jira]
projects_filter = ["PROJ1", "PROJ2"]

[default.confluence]
spaces_filter = ["TEAM", "DOCS"]
```

### API Token 생성

1. [Atlassian API Tokens](https://id.atlassian.com/manage-profile/security/api-tokens) 접속
2. "Create API token" 클릭
3. 토큰 복사하여 설정 파일에 추가

### 설정 우선순위

```
CLI 플래그 > 환경변수 > 프로젝트 설정 > 전역 설정
```

**예시**:
```bash
# 설정 파일 대신 CLI 플래그 사용
atlassian --domain company.atlassian.net --email user@example.com --token TOKEN \
  jira search "status = Open"

# 환경변수 사용
export ATLASSIAN_DOMAIN="company.atlassian.net"
export ATLASSIAN_EMAIL="user@example.com"
export ATLASSIAN_API_TOKEN="your-token"
```

---

## 💡 사용 예시

### Jira 작업

```bash
# 이슈 조회
atlassian jira get PROJ-123

# JQL 검색
atlassian jira search "project = PROJ AND status = Open" --limit 10
atlassian jira search "assignee = currentUser() AND status != Done"

# 이슈 생성
atlassian jira create PROJ "버그 수정" Bug --description "상세 내용"

# 이슈 수정
atlassian jira update PROJ-123 '{"summary":"새 제목"}'

# 댓글 추가
atlassian jira comment add PROJ-123 "작업 완료했습니다"

# 상태 전환
atlassian jira transitions PROJ-123
atlassian jira transition PROJ-123 31
```

### Confluence 작업

```bash
# 페이지 검색
atlassian confluence search 'type=page AND space="TEAM"' --limit 10

# 페이지 조회
atlassian confluence get 123456

# 페이지 생성
atlassian confluence create TEAM "API 문서" "<p>내용</p>"

# 페이지 수정
atlassian confluence update 123456 "API 문서 v2" "<p>새 내용</p>"

# 하위 페이지 목록
atlassian confluence children 123456

# 댓글 조회
atlassian confluence comments 123456
```

### 설정 관리

```bash
# 현재 설정 표시 (토큰 마스킹)
atlassian config show

# 설정 파일 위치
atlassian config path --global
atlassian config path

# 설정 파일 편집
atlassian config edit --global

# 모든 설정 위치 나열
atlassian config list
```

### 고급 기능

#### 필드 최적화 (60-70% 크기 감소)

```bash
# 기본 17개 필드로 검색 (description 제외)
atlassian jira search "project = PROJ"

# 커스텀 필드 지정
atlassian jira search "project = PROJ" --fields key,summary,status,assignee

# 환경변수로 기본값 변경
export JIRA_SEARCH_DEFAULT_FIELDS="key,summary,status"
atlassian jira search "project = PROJ"

# 기본 필드에 커스텀 필드 추가
export JIRA_SEARCH_CUSTOM_FIELDS="customfield_10015,customfield_10016"
```

**기본 17개 필드**:
```
key, summary, status, priority, issuetype,
assignee, reporter, creator, created, updated,
duedate, resolutiondate, project, labels,
components, parent, subtasks
```

#### 멀티 프로필

```toml
[default]
domain = "company.atlassian.net"
email = "user@company.com"

[work]
domain = "work.atlassian.net"
email = "user@work.com"
```

```bash
atlassian --profile work jira search "project = WORK"
```

#### JSON 출력

```bash
# JSON 출력
atlassian jira get PROJ-123 --pretty

# jq와 함께 사용
atlassian jira search "assignee = currentUser()" | jq -r '.items[].key'
```

---

## 🏗️ 아키텍처

### 프로젝트 구조

```
src/
├── main.rs          # CLI 진입점 (clap)
├── config.rs        # 4단계 우선순위 설정
├── http.rs          # HTTP 클라이언트
├── jira/
│   ├── api.rs       # 8개 Jira 작업
│   ├── adf.rs       # ADF 자동 변환
│   └── fields.rs    # 필드 최적화
└── confluence/
    ├── api.rs       # 6개 Confluence 작업
    └── fields.rs    # 필드 최적화
```

### 핵심 기술

- **언어**: Rust 2024 Edition (MSRV 1.91.1)
- **CLI**: clap 4.5 (derive API)
- **비동기**: Tokio 1.48
- **HTTP**: Reqwest 0.12 (rustls-tls)
- **JSON**: serde_json 1.0

### API 버전

- **Jira**: REST API v3
- **Confluence**: REST API v2 (검색만 v1)

---

## 🔧 문제 해결

### 설정을 찾을 수 없음

**확인 사항**:
- 설정 파일 경로: `atlassian config path`
- 설정 내용 확인: `atlassian config show`

**해결**:
```bash
# 전역 설정 초기화
atlassian config init --global --domain company.atlassian.net \
  --email user@example.com --token YOUR_TOKEN
```

### API 인증 실패

**확인 사항**:
- Domain 형식: `company.atlassian.net` (https:// 없이)
- Email 형식: 유효한 이메일 주소
- Token: [API Tokens 페이지](https://id.atlassian.com/manage-profile/security/api-tokens)에서 생성

### 필드 필터링 작동 안 함

**우선순위 확인**:
1. CLI `--fields` 파라미터
2. `JIRA_SEARCH_DEFAULT_FIELDS` 환경변수
3. 기본 17개 필드 + `JIRA_SEARCH_CUSTOM_FIELDS`

```bash
# 테스트
JIRA_SEARCH_DEFAULT_FIELDS="key,summary" atlassian jira search "project = PROJ"
```

### 프로젝트 접근 제한

```toml
[default.jira]
projects_filter = ["PROJ1", "PROJ2"]
```

JQL에 프로젝트가 없으면 자동 추가:
```
입력: status = Open
실행: project IN (PROJ1,PROJ2) AND (status = Open)
```

---

## 📚 참고 자료

### Atlassian API
- [Jira REST API v3](https://developer.atlassian.com/cloud/jira/platform/rest/v3/)
- [Confluence REST API v2](https://developer.atlassian.com/cloud/confluence/rest/v2/)
- [Atlassian Document Format (ADF)](https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/)

### 개발 문서
- [CLAUDE.md](CLAUDE.md) - AI Agent 개발자 가이드
- [Rust 공식 문서](https://www.rust-lang.org)

---

## 🚀 개발

### 빌드

```bash
cargo build              # 개발 빌드
cargo build --release    # 릴리스 빌드 (최적화)
cargo test               # 테스트 실행 (120개)
cargo clippy             # 린트
cargo fmt                # 포맷팅
```

### 릴리스 프로필

```toml
[profile.release]
opt-level = 3       # 최대 최적화
lto = true          # Link-time optimization
codegen-units = 1   # 단일 코드 생성
strip = true        # 디버그 심볼 제거
```

**결과**: 3.8MB 최적화된 바이너리

---

## 📝 라이센스

MIT License

---

## 🤝 기여

Issue 및 Pull Request 환영합니다!

1. Fork
2. Feature 브랜치 생성 (`git checkout -b feature/amazing`)
3. 변경사항 커밋 (`git commit -m 'Add amazing feature'`)
4. 브랜치 푸시 (`git push origin feature/amazing`)
5. Pull Request 생성

---

<div align="center">

**Rust로 만든 빠르고 강력한 Atlassian CLI 도구** 🦀

Version 0.1.0 • Made with ❤️ for productivity

</div>
