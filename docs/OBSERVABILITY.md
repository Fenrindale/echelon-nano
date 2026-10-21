# Task 관찰 화면

Path, Broadcast, Sessions는 기존 서버 기록을 읽는 운영자 화면이다. 조회로 역할 권한, 원문, 제출·심사 결과, 고정 근거 또는 실행 체크포인트를 바꾸지 않는다. 새로운 모델 요청을 만들거나 실행을 재전송하지 않는다.

| 화면 | 기준 |
| --- | --- |
| Path | 목표·실행·정보 요청·Context Pack·프로토콜·Work Order·관찰·결과·본문 구간·제출·심사·지식 개정·감사 제안을 실제 ID 관계로 연결 |
| Broadcast | 같은 업무 기록을 생성 시각과 ID의 내림차순으로 탐색. 상태와 중앙/지역 보관 범위는 현재 값 |
| Sessions | 일반 실행·본문 분석·재심사의 역할별 공개 실행 이벤트, 도구 입력·출력·영수증과 사용량 |

단순 `model.started`/`model.completed`는 Sessions에서 확인한다. Broadcast는 도구 호출 전체를 방송하지 않는다. 업무 지시·재계획·입력 대기·실패·거부와 같은 중요 실행 기록은 관계도에 포함한다. Broadcast는 과거의 모든 상태 변경을 재구성하는 event-sourced 이력이 아니다. 원래 생성 시각과 현재 상태를 구분해 표시하며, 실제 변경 이벤트가 존재하는 경우 별도 항목으로 표시한다.

## 조회 API

모든 경로는 기존 `operator` 인증과 tenant/Task 소유권 검사를 거친다. 모델 역할 토큰으로 운영 화면의 원본 영수증을 조회할 수 없다. 읽기 응답에는 `read_only:true`가 있다.

- `GET /v1/operator/tasks/{task_id}/path?tenant_id=...`
- `GET /v1/operator/tasks/{task_id}/broadcast?tenant_id=...`
- `GET /v1/operator/tasks/{task_id}/sessions?tenant_id=...`
- `GET /v1/operator/tasks/{task_id}/session?tenant_id=...&source=execution|ingestion|retention&source_id=...&role=handler|analyst|agent|master`

Path/Broadcast는 `kind`, `q`, `offset`, `limit`(기본 40, 최대 100), `focus`(노드 ID)를 받는다. `items`는 해당 페이지, `nodes`/`edges`는 페이지 항목과 한 단계 인접 관계다. 이웃이 200개를 넘으면 `neighbours_truncated:true`로 명시하고 선택 노드를 먼저 보존한다. 전체 회사 그래프를 재귀 조회하지 않는다. `total`/`has_more`로 페이지 밖의 항목을 구분한다. 관계도는 확대·축소·이동·키보드 노드 선택을 지원하며 주변 관계와 고정 지식 개정을 열 수 있다.

Sessions 목록은 실행 소스와 역할로 묶는다. 목록의 상태·단계는 실행 전체의 상태·단계이며, 역할마다 독립 에이전트 프로세스가 실행 중이라는 뜻이 아니다. 세부 기록은 단조 증가하는 원래 이벤트 `seq`의 역순 커서(`before`, `next_before`, `has_more`)로 조회한다. 예전 250개 창 밖의 기록도 불러온다. `tool.pending`과 별도로 완료·실패·거부 이벤트에 저장된 도구 요청과 실제 결과를 연결한다.

본문 분석은 `ordinal`(0부터)을 지정하면 Agent의 추출, Handler의 제안·제출 또는 Analyst의 확정 판단을 원문 구간의 상태·출처 위치와 함께 조회한다. 재심사는 같은 매개변수로 저장된 심사 스냅샷·결정·반영 영수증을 조회한다. UI의 번호는 1부터다. 원시 모델 응답 캐시, 프롬프트·비공개 추론은 노출하지 않는다.

표시용 JSON은 깊이·배열·문자열 길이를 제한하고 생략을 명시한다. 기밀 키·인증 헤더·바이너리 base64·모델 내부 이력을 제외하고 문자열도 자격증명 패턴으로 정제한다. 원본 기록과 아티팩트 다운로드는 기존 기능으로 보존된다. 저장된 지식의 본문은 고정 ID/버전의 지식 상세에서 조회한다. 영수증·조회·인용 기록은 모델의 내부 사고나 인과적 기여를 증명하지 않는다.

마이그레이션 0016은 조회 인덱스만 추가하며 과거 이력을 수정하지 않는다. 검증은 Docker checks, 격리 `knowledge_db` 테스트의 긴 이력·출처·고정 버전·범위·역할별 산출물 검사, React DOM 테스트와 격리 `Verify-Beta.ps1`의 실제 HTTP 권한 검사로 수행한다. 운영 Task에 합성 자료를 등록하지 않는다.
