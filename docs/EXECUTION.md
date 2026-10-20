# 실제 역할 LLM 실행 루프

베타 0.2는 이미 연결한 ChatGPT 계정으로 Handler → Analyst → Handler → Agent → Handler를 실행하고, 선택한 지식 제출·Analyst 채택과 별도 Master 감사까지 연결한다. Rust 서버가 역할·단계·회사·Task·Work Order·근거 범위를 검사한다. 프롬프트는 권한 부여 수단이 아니다. 전체 운영 계약은 [베타 가이드](BETA.md)를 따른다.

## 실행 경로

콘솔에서 새 Task를 시작할 때 업무 목표 외의 설정을 요구하지 않는다. Handler가 사용 가능한 모델·프로토콜·기밀 참조의 메타데이터를 보고 `run.configure`로 실행 설정과 초안 계약을 정한다. Analyst가 근거를 전달하면 Handler가 레퍼런스를 조회하고 `run.configure_contract`로 계약을 확정해야 계획·지시할 수 있다. 모델은 설치된 도구나 서버 상한을 늘릴 수 없으며, 내부망·HTTP URL은 목표에 명시되거나 등록된 출처여야 한다. 기밀 값은 전달하지 않는다.

1. Handler가 목표에 맞는 `context.request` 조건을 작성한다. 서버가 멱등 요청 키를 지정한다.
2. Analyst가 해당 요청의 후보를 페이지 단위로 보고 기존 ID·버전을 골라 `context.curate`로 전달한다. 보지 않은 후보는 선택할 수 없다. 거짓·실패 이력과 의존 근거의 포함은 기존 Context Pack 서버가 보장한다. Analyst는 업무 결론이나 새 원문을 작성하지 않는다.
3. Handler가 Pack과 필요한 레퍼런스를 조회한다. 모든 caution 근거를 조회하고 계획을 저장해야 지시할 수 있다. Work Order에는 목표·예상 산출물·조회한 레퍼런스를 넣으며 모든 caution 참조를 포함해야 한다.
4. Agent가 자기 Work Order를 조회한다. 계약에 지정한 버전의 문서·해시·출처·주의 이력이 증거 스냅샷으로 전달된다. Agent는 중앙 DB와 다른 Work Order를 탐색할 수 없다. 관찰을 남기고 근거가 연결된 결과를 반환한다.
5. Handler가 저장된 Agent 결과를 다시 조회하고 목표 달성 여부를 판단한다. Agent의 partial/blocked/failed 결과를 completed로 올릴 수 없다. 루프 완료와 업무 결과의 completed/partial/blocked/failed는 별도 값이다.

Agent는 허용한 URL의 읽기, 격리 코드 실행과 실제 산출물 회수를 수행할 수 있다. `retain_knowledge`를 선택하면 Handler가 재사용 항목을 제출하고 Analyst가 지정한 제출만 채택·반려한다. `dream_after`는 이후 별도 Master 감사·정리 검토안 단계를 실행한다. 후처리 오류가 이미 확정된 업무 결과를 바꾸지 않는다. 한 Task의 Work Order는 순차 처리하며 여러 Task 실행은 서버의 동시성 한도 안에서 진행한다.

## 운영자 API

콘솔의 기본 경로는 `POST /v1/operator/tasks/launch`다. 업무 입력은 `goal`과 선택 사항인 `attachments`이며 `tenant_id`와 `request_id`는 클라이언트가 지정하는 범위·멱등 메타데이터다.

```json
{
  "tenant_id": "22222222-2222-4222-8222-222222222222",
  "request_id": "11111111-1111-4111-8111-111111111111",
  "goal": "이번 달 진행 현황을 근거와 함께 한국어 Markdown 보고서로 작성해 주세요."
}
```

응답은 `task_id`, `run_id`, `replayed`를 반환한다. 같은 요청 키와 목표의 재전송은 같은 Task와 실행을 반환한다. 다른 목표로 키를 재사용할 수 없다. 설정 단계가 실패해도 초안을 보존한다. `GET /v1/operator/runs/{id}`의 `handler_settings`와 `contract_finalized`로 Handler 결정을 확인한다. 설정 효과는 체크포인트와 감사 기록에 남고 명시적 재개 시 동일 효과를 복원한다. 자동 업무 실행의 호출 예산은 32–48회, Dreaming은 16–48회 안에서 Handler가 결정한다. 누적 토큰 상한은 적용하지 않고 사용량을 수집한다. 이전 요청·설정의 `max_total_tokens`는 호환을 위해 받지만 중단 조건에 쓰지 않는다. 새 Handler 설정은 이 값을 요구하지 않는다. 연결 계정 선택은 저장된 업무실 기본값 또는 기존 OAuth 연결을 사용하고 임의 API 키로 전환하지 않는다.

기존 Task는 `POST /v1/operator/tasks/{id}/launch`로 설정 입력 없이 실행한다. 새 방식으로 생성한 Task는 Handler 설정 단계를 다시 수행한다. 기존 수동 계약으로 만든 Task는 기존 제한을 보존한다. 아래 명시적 설정 API는 자동화와 검증용으로 유지한다.

`POST /v1/operator/runs`는 로컬 운영자 인증이 필요하다. 역할 토큰으로 시작하거나 조회할 수 없다.

```json
{
  "request_id": "11111111-1111-4111-8111-111111111111",
  "tenant_id": "22222222-2222-4222-8222-222222222222",
  "task_id": "33333333-3333-4333-8333-333333333333",
  "connection_id": "selected-connected-account",
  "model": "selected-account-model-slug",
  "target": "report:monthly-summary",
  "max_calls": 32
}
```

먼저 기존 API로 실제 Task를 만들고 연결 계정의 모델 카탈로그에서 모델을 선택한다. 같은 Task·request_id·설정의 재요청은 원래 run ID만 반환한다. 설정이 달라지면 409다. 한 Task에서는 한 실행만 진행할 수 있다. 서버가 비동기로 실행하며 `GET /v1/operator/runs/{id}`로 상태, 역할별 호출·도구 이벤트, Pack/Work Order ID와 최종 결과를 조회한다. 내부 모델 대화 전문과 자격증명은 이 응답에 포함하지 않는다.

상태는 `queued`, `running`, `paused`, `cancelled`, `completed`, `failed`, `exhausted`, `interrupted`다. `completed` 상태에서도 `result.outcome`을 확인해야 한다. Work Order 계약·결과, 관찰, 완료 효과와 도구 이벤트는 불변 이력이다. `POST /v1/operator/runs/{id}/control`의 `action`은 `pause`, `resume`, `cancel`이다.

## 모델 연결과 제한

기밀 첨부는 기밀값 없는 목록과 서비스 주소를 반환한다. Handler는 `credential.inspect` 후 Task의 `credential_bindings`를 정하고 `credential.import`·`credential.check`를 Agent에게 위임한다. 별도 커넥터 등록·사용자 승인은 요구하지 않는다. 연결은 목표 또는 선택한 설정 원본과 일치해야 하며 Work Order가 설정을 고정한다. Handler는 `work_order.read.tool_evidence`의 저장 영수증·실제 신원 관찰로 결과를 평가한다. 세부 형식·실행 범위·재사용 계약은 [기밀 처리](CREDENTIALS.md)를 따른다.

선택 첨부는 `attachments:[{request_id,name,media_type,data_base64}]`로 전달한다. 각 파일에 안정된 요청 UUID를 사용하며 최대 16개·파일당 1 MiB·합계 4 MiB다. 실행 등록 경로의 JSON 본문 상한은 base64를 고려해 6 MiB이며 다른 API의 2 MiB 상한은 유지한다. 서버가 파일 검증·저장과 Task 생성을 한 트랜잭션에서 처리하고, 파일 요청 ID·이름·형식·SHA256 목록을 불변으로 고정한다. Handler는 `artifact.read {artifact_id,offset:0,limit:4000}`으로 UTF-8 미리보기를 읽고 계약의 `input_artifacts`를 선택한다. 미리보기는 문자 경계에서 잘라 반환하고 전체 base64를 제외한다. 바이너리는 본문을 해석한 것으로 표시하지 않는다. Agent는 자기 Work Order의 입력 허용 목록을 계속 따른다.

공식 Sign in with ChatGPT의 OAuth 자격증명을 Vault에서 사용한다. 각 추론 전에 기존 만료 검사와 직렬화된 갱신을 거친다. 계정 카탈로그에 없는 모델은 시작 시 거부한다. API 키나 다른 계정으로 자동 전환하지 않는다.

Responses 요청은 `store:false`, `stream:true`, 입력 배열, namespace 안의 엄격한 함수 스키마를 사용한다. 필요 이력과 암호화된 reasoning 항목은 같은 역할 단계 안에서만 다시 전송한다. 역할 전환 시 다음 역할에 필요한 계약·ID만 넘겨 대화를 새로 시작한다. `response.completed` 전에는 함수 호출을 실행하지 않는다. 실패·불완전·끊긴 스트림은 종료한다.

직접 실행 API의 호출 수는 1–48회다. 토큰 사용량으로 실행을 중단하지 않는다. 총 실행 30분, 개별 HTTP 추론 120초, 스트림 2 MiB, 모델 입력 256 KiB와 중지·취소는 유지한다. 연결 계정의 제공자 한도는 별도로 적용된다.

`GET /v1/operator/metrics?tenant_id=...&task_id=...`는 완료 응답의 입력·출력·캐시·총 토큰을 역할·단계·모델·도구 결정별로 집계한다. 도구별 값은 해당 도구를 선택한 모델 응답의 사용량이며 도구 실행 자체의 비용이 아니다. 다음 추론 전 도구 결정이 없으면 `unassigned`로 남긴다. 미완료 응답은 사용량을 추정하지 않고 `calls_without_completed_usage`에 구분한다. 집계 차원 간 수치는 같은 사용량의 다른 분류이므로 합산하지 않는다. 원본 사용량은 불변 실행 이벤트에 보존한다. 실행·집계 응답은 `token_policy:observe_only`, `token_limit_enforced:false`로 현재 정책을 명시하며 잔여 계정 한도·과금은 추정하지 않는다.

효과 실행 전에 모델 결정과 체크포인트·pending 원장을 저장한다. 효과는 run/call ID에서 고정 키를 얻고 같은 ID의 다른 요청은 거부한다. DB·감사 오류는 즉시 멈춘다. 서버 재시작 시 진행 중 기록은 interrupted로 표시하며 제공자 호출을 자동 반복하지 않는다. 운영자의 명시적 재개는 완료 원장·기존 Pack·Work Order·산출물을 재사용한다. 살아 있는 Worker의 동일 작업은 원래 결과를 회수한다. 지원하는 재조정은 관찰용 GET와 임시 파일 작업뿐이다. 제공자 실패와 불완전 스트림은 멈추고, 완료 응답의 중첩 JSON 형식 오류만 효과 실행 없이 최대 세 번 보정한다.

문서의 명령문은 증거 데이터로 취급한다. 도구 스키마에서 감춘 기능을 모델이 만들어 내도 동일한 서버 권한과 단계 검사를 받는다. 이는 모델이 항상 올바른 판단을 한다는 보장은 아니며, 분석 결과의 의미적 정확성은 별도 검토가 필요하다.

공식 규격: [SIWC 추론](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference), [지원 필드와 도구 제한](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations), [함수 호출과 namespace](https://developers.openai.com/api/docs/guides/function-calling).

## 검증

```powershell
docker build --target checks .
docker compose -f compose.verify.yaml up --build --abort-on-container-exit --exit-code-from tests
docker compose -f compose.verify.yaml down --remove-orphans
docker compose build core
docker compose up -d --no-deps core
./scripts/Verify-LLMLoop.ps1
./scripts/Verify-Roles.ps1
./scripts/Verify-ContextPacks.ps1 -RestartCore
./scripts/Verify-LiveOAuth.ps1
```

LLM 검증은 명시적으로 연결한 계정에 실제 모델 요청을 보낸다. 합성 Data/Info/Intel과 실패한 이전 개정을 별도 tenant에 남긴다. 실제 세 역할의 완료 응답, Context Pack·참조·계획·지시·결과 회수, 과거 실패의 Agent 전달, 권한·Task 범위 거부와 1회 호출 예산 소진을 확인한다. 원본 실행 기록과 합성 자산 목록은 비공개 `.runtime/evidence`에 보관한다. 단위·격리 DB 테스트와 실제 계정 실행 결과는 구분해 보고한다.

## 이전 Phase 3 기술 검증 기록 · 2026년 10월 19일

아래는 베타 확장 전 19기능·세 역할의 기록이다. 최신 22기능·네 역할·실무·복구 결과는 [구현 검증표](IMPLEMENTATION.md)를 따른다.

| 검증 | 관찰 결과 |
| --- | --- |
| Rust | rustfmt, Clippy `-D warnings`, 단위 테스트 16개, RSA/JWKS 테스트 1개 통과 |
| 격리 PostgreSQL | 지식·Context Pack·Work Order/실행 기록 테스트 3개 통과. 다른 Task·Work Order 접근과 결과의 중복 확정·수정 거부, 진행 중 기록의 interrupted 전환 확인 |
| 실제 역할 실행 | 연결 계정의 `gpt-6.1-sol`, Handler 9회·Analyst 2회·Agent 2회, 총 13회의 완료 응답. 누적 usage 21,663 tokens |
| 실제 업무 흐름 | 44개 검증 통과. 과거 실패 근거를 포함한 버전 고정 자료로 Work Order 작성, Agent의 현재 규칙·폐기 규칙 구분과 인용 결과, Handler 회수·완료 확인 |
| 실행 중 OAuth 갱신 | 진행 중 generation 전환 후 Agent를 포함한 후속 실제 추론이 갱신된 자격증명으로 완료됨. 재현 스크립트에도 이 검사를 포함 |
| 호출 예산 | 별도 실제 요청을 1회로 제한해 exhausted 확인. Work Order 지시 전에 종료 |
| 기존 권한 검증 | 4역할 × 19기능 = 76개 정책 검사, 금지 dispatch 57개 거부, tenant·운영자 범위 거부와 역할 컨테이너의 외부 직접 접속 차단 |
| 기존 기능 회귀 | 중앙 지식 HTTP 검사 35개, Context Pack 검사 51개, 각각 재시작 보존 확인 |
| 갱신 동시성 회귀 | 실제 고정 추론 → 같은 generation의 동시 갱신 4개를 한 번으로 통합 → 갱신 후 추론 성공 |
| 실행 결과 보존 | core 재시작 후 완료 상태·최종 결과·모델 호출 수 유지 |

실제 스트림에서 종료 이벤트에 output이 생략되는 경우를 발견해 `response.output_item.done`을 모으도록 수정했다. 개별 항목이 도착해도 `response.completed` 성공 전에는 실행하지 않는 회귀 테스트를 포함한다. 이 결과는 해당 계정과 로컬 환경의 관찰이며, GitHub CI는 계정 없는 정적 검사·격리 DB 검증만 수행한다.
