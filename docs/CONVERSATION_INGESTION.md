# Conversation bodies into reusable knowledge

Conversation inventories are source metadata. They do not demonstrate that any
message bodies were analyzed or that reusable business knowledge was retained.
Large archives use a durable body-ingestion job attached to an existing Task.
The original Task goal, previous runs, submissions and rejected knowledge remain
unchanged as historical evidence; the job records the latest explicit goal.

The operator transports sources, not authored knowledge. Imported units contain
actual user/assistant message text, original IDs, message ordinals and fragment
offsets. Raw content duplicated in alternate fields, viewer HTML and credential
stores are not source inputs. Original archives remain private and unmodified.
The core removes common credential assignments, private-key blocks and known
token formats before sending body text to the configured model. This is a
conservative filter, not a guarantee that every possible secret format is known.

Each stage uses the Task's configured model connection and a fresh context:

1. Handler plans the analysis focus, tags and candidate density. It may explicitly
   exclude repetitive assistant commentary, which remains in the imported corpus.
2. Agent reads each actual body unit, proposes reusable facts and supplies real
   source quotations. The server rejects quotations absent from that unit.
3. Handler authors substantive Data/Info/Intel documents and submits them through
   the existing Handler-only `knowledge.submit` capability. Inventory-only pointer
   documents are not valid body-ingestion proposals.
4. Analyst reads those actual immutable submission IDs and all grounded candidate
   evidence. It compares related central knowledge and decides acceptance,
   classification and tags. It cannot author or change document content.
5. The server dispatches Analyst-only `knowledge.publish` decisions. Lower-level
   accepted item IDs and versions bind Handler-requested Data → Info → Intel
   derivation edges. Rejected dependencies block dependent proposals; they are
   retained rather than silently removed or replaced by invented references.
6. Agent reports actual coverage and Handler judges the result from adoption
   receipts. Missing bodies, partial archives and historical claims remain
   explicit limitations. Zero adoption cannot produce a successful business result.

The job is independent of the interactive execution loop's short call guard and
small post-task retention batch. Token use is observed by role, phase, model and
decision tool, without a token cap. Each body unit is bounded to keep model
contexts independent. Sources, completed decisions, deterministic effect keys
and append-only events permit explicit pause/resume without duplicate adoption.
A core restart interrupts running jobs. Unknown provider outcomes require an
explicit acknowledgement before another request. Completed invalid model
decisions remain recorded; an explicit resume allows another bounded formatting
attempt without repeating completed effects. No archived command is executed.

## Operator API

All routes use the existing operator/session authentication; role bearer tokens
cannot import corpora or control jobs. Tenant and Task scope are required.

- `POST /v1/operator/ingestions`: immutable `request_id`, `goal`, `source_manifest`.
- `POST /v1/operator/ingestions/{id}/units`: contiguous `ordinal`, `source_id`,
  `locator`, `metadata`, `messages`, `input_sha256`. Each message has `id`, `role`,
  `phase`, `ordinal`, `offset`, `text`. The hash covers the canonical UTF-8 JSON
  message array with sorted keys and compact separators. Source identity and
  sanitized hashes are immutable; identical requests replay safely.
- `POST /v1/operator/ingestions/{id}/start`: `expected_units` must match the entire
  contiguous imported corpus. Sealing and imports serialize on the job row.
- `GET /v1/operator/ingestions?tenant_id=…&task_id=…`: progress, categories,
  proposals, model use, current stage and final coverage.
- `POST /v1/operator/ingestions/{id}/control`: `action` is `pause`, `resume` or
  `cancel`; unknown in-flight inference additionally requires
  `acknowledge_unknown_request: true` on resume.

The Task needs its usual `artifact.write` grant for pinned source/report artifacts.
The job uses an immutable Work Order and the same server-side Agent scope checks
as normal execution. It has no shell, network or credential capability. Starting
normal execution or editing the Task while the corpus is active is rejected.
Consistent backups also refuse active corpus work. The Task console displays
body progress separately from prior execution results and exposes pause/resume.

## Verification

`scripts/Verify-BodyIngestion.ps1` runs only in the isolated port-18432 copy. It
uses actual OAuth inference for all three roles, owned dialogue with failure and
untrusted quoted instructions, source replay/sealing, pause/resume, category
adoption, pinned source/derivation evidence and role usage accounting. The
isolated PostgreSQL suite separately verifies immutable sources, completed
checkpoints, terminal results and role-owned submission/adoption. Synthetic
fixtures must never be registered in the live corpus Task.

## 일시적인 모델 오류 복구

본문 분석의 연결 실패·시간 초과·완료 없는 스트림 단절·일시적 HTTP 거부는 같은 미완료 단계의 모델 생성만 자동 재시도한다. 각 요청을 `model.started`로 집계하고 `model.retry_scheduled`에 오류 코드·대기 시각·미확인 사용량·미실행 효과를 남긴다. 완료 모델 응답 캐시와 완료 구간, 제출·채택 ID, Work Order는 재사용한다. 대기는 최대 60초 간격이며 중지·취소·모델 정책 변경을 확인한다. 인증·정책·DB·형식/근거 검증 실패나 불명확한 업무 도구 효과는 이 자동 재시도 대상이 아니다. 서버 재시작은 기존 명시적 재개 절차를 유지한다.

See [central versus Task-local retention](RETENTION.md) for decision-utility requirements, legacy review, local preservation and verification. Central adoption counts are historical receipts, distinct from current placement.
