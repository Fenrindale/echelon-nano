# Phase 0 구현과 검증

이 문서는 Phase 0 시점의 기록이다. 후속 중앙 지식 구현과 현재 API 범위는 [Phase 1 문서](KNOWLEDGE.md)를 따른다.

## 구현 범위

Rust 1.90.0, Axum, Tokio, PostgreSQL, Vault를 사용하는 제어 서버와 Docker Compose 기반을 구현한다. 공개 저장소에는 소스와 재현 가능한 검증 명령을 보관하고 실제 계정 자격증명과 실행 증거는 .runtime에 분리한다.

OAuth는 Echelon Nano의 새 등록으로 수행한다. 다른 애플리케이션의 인증 파일을 가져오지 않는다. HTTP loopback, state, nonce, PKCE S256, 발급된 client ID, RS256 JWKS 검증을 사용하며 계정 식별과 plan usage 권한을 각각 확인한다.

토큰 갱신은 PostgreSQL의 세션별 advisory lock과 Vault KV v2 CAS를 사용한다. 같은 generation에 대한 동시 요청은 한 번의 갱신으로 합친다. 갱신된 access token, refresh token, scope, 만료시간을 하나의 Vault 기록으로 교체한다.

## 증명할 항목

| 항목 | 증거 |
| --- | --- |
| Rust 빌드와 정적 검사 | Docker checks 단계의 fmt, Clippy, 테스트 결과 |
| 서명과 OIDC 검증 | 실행 시 생성한 RSA 키와 JWKS를 사용하는 부정 테스트 |
| 역할 권한 | 네 역할과 15개 기능의 행렬, 금지된 실제 dispatch의 HTTP 403 |
| 범위 제한 | 다른 tenant의 dispatch 거부와 Work Order·target 검사 |
| 런타임 격리 | 읽기 전용 파일시스템, capability 제거, 내부 네트워크, 직접 외부 접속 실패 |
| 감사 보존 | PostgreSQL UPDATE 거부 |
| OAuth callback | 잘못된 state, 사용자 거부, 소비된 callback 재사용 거부 |
| 실제 OAuth 로그인 | 사용자 브라우저 승인 후 저장된 검증 완료 계정 |
| 실제 추론 | 모델 목록과 별개로 response.completed 및 고정 응답 확인 |
| 실제 갱신 | 동시 요청 4개에서 한 generation 증가, 갱신 후 두 번째 추론 성공 |

로컬 단위 테스트, 컨테이너 검증, 실제 계정 검증은 서로 다른 결과로 기록한다. 실제 로그인과 사용자 동의를 마치지 않은 상태에서 추론과 갱신을 통과로 표시하지 않는다.

## 2026년 10월 19일 로컬 검증 결과

Windows, PowerShell 7, Docker Desktop의 Linux 컨테이너에서 검증했다. 아래 결과는 합성 Task와 고정 추론 요청을 사용한 Phase 0 검증이다.

| 검증 | 결과 |
| --- | --- |
| Rust 정적 검사와 빌드 | rustfmt, Clippy `-D warnings`, release 빌드 통과 |
| Rust 테스트 | 단위 테스트 7개와 RSA/JWKS 통합 테스트 1개 통과 |
| 역할 정책 | 4개 역할 × 15개 기능, 60건 통과 |
| 실제 dispatch | 허용 동작 5건 성공, 금지 동작 46건 HTTP 403 |
| 범위·운영자 제한 | 다른 tenant 접근 4건 거부, 역할 토큰의 운영자 API 접근 8건 거부, 익명 운영자 접근 거부 |
| 컨테이너 | 네 역할의 직접 외부 접속 차단, internal 네트워크 확인 |
| 제어 서버 | UID 10001, 읽기 전용 루트, 모든 capability 제거, no-new-privileges 확인 |
| 감사 기록 | PostgreSQL UPDATE 거부 |
| OAuth callback | state 불일치·사용자 거부·callback 재사용 거부 |
| 실제 로그인 | 사용자 브라우저 로그인과 ChatGPT 플랜 사용 동의 후 계정 검증 완료 |
| 실제 추론 | 계정 모델 목록에서 선택한 `gpt-6.1-sol`의 `response.completed`와 고정 응답 일치 확인 |
| 실제 갱신 | 같은 generation으로 보낸 동시 요청 4개가 한 generation 증가로 합쳐짐. 갱신 후 추론 성공 |
| 재시작 복구 | Vault를 재시작하고 DPAPI 복구 자료로 unseal한 뒤 서버 재생성. 기존 generation 유지, 모델 8개 조회, 추론 성공 |
| 비밀 자료 | 복구 자료의 평문 파일 제거, `.runtime`과 `.build`의 Git 제외 및 공개 파일 패턴 검사 통과 |

OAuth 결과는 해당 계정과 시점의 관찰이다. 다른 계정의 모델 제공 여부나 향후 서비스 가용성을 보장하지 않는다. 원본 로컬 증거는 `.runtime/evidence`에 두며 계정 식별자·요청 식별자·자격증명은 이 공개 문서에 포함하지 않는다.

재현 명령은 README에 있다. 재시작 복구는 `docker compose restart vault`, `./scripts/Initialize-Local.ps1 -SkipBuild` 순서로 수행한 후 연결 상태와 추론을 다시 확인한다. GitHub Actions의 정적 검사·빌드는 이 로컬 실계정 결과와 별개의 검증이다.

## 현재 제한

P0에서는 외부 수집·shell 실행·중앙 지식 채택·Master 개정 어댑터를 활성화하지 않는다. 허용된 역할이어도 아직 구현하지 않은 기능은 501로 종료한다. 실제로 동작하는 기능은 Task 계획, Work Order 생성, 관찰 등록, 범위가 제한된 지식 조회와 감사 조회다.

Vault의 원자적 갱신은 Vault 자체 안에서 보장한다. OAuth 제공자의 토큰 회전과 Vault·PostgreSQL 사이를 하나의 분산 트랜잭션으로 만들 수는 없다. 외부 갱신 직후 저장소 장애가 발생하면 해당 세션을 정상 갱신으로 간주하지 않으며 재인증이 필요할 수 있다.

로컬 서비스는 loopback만 게시한다. 다중 사용자·원격 호스팅 전에 설치 형태에 맞는 OAuth 사용 조건과 운영자 인증, TLS, 조직별 데이터 정책을 확장해야 한다.

현재 DB 접근은 제어 서버에 집중돼 있다. 역할 API와 감사 UPDATE/DELETE 방지는 검증했지만, DB 관리자까지 막는 외부 불변 감사 저장소나 완성된 다중 사용자 격리를 구현한 것은 아니다. 프로토콜·태그·참조 그래프와 역할별 실제 LLM 업무 루프는 후속 단계다.
