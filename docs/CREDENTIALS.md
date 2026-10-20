# 첨부 기밀의 적재와 인증 확인

목표와 첨부파일만으로 Handler가 안전한 목록과 서비스 주소를 확인하고 Agent에게 Vault 적재·인증 확인을 지시한다. 별도 커넥터 등록·승인이나 운영자의 기술 설정이 필요하지 않다. Handler가 Task별 연결을 정하며, 같은 업무실의 기존 Vault 묶음도 직접 선택한다. Work Order의 참조·연결 범위는 내부 실행 계약이지 사용자 승인 절차가 아니다. 저장된 연결 프리셋은 선택 사항이다.

## 실행 흐름

1. Handler가 `credential.inspect {artifact_id}` 또는 `artifact.read`로 변수 이름·개수·빈 값과 `targets`를 본다. `URL`·`HOST`·`URI`로 끝나는 설정 필드에서 HTTP(S) 서비스 주소만 선별한다. URL 안의 인증정보·쿼리와 알려진 기밀값을 포함한 주소는 제외한다. 기밀값·주석·원본 base64는 역할 응답에서 제외한다.
2. Handler가 `credential.import`, 필요한 `credential.check`, `artifact.write`와 정확한 입력 파일을 계약에 넣고 Agent에게 위임한다. 중앙 지식의 유무는 설치된 도구의 사용 가능 여부와 별개다.
3. Agent가 `credential.import {artifact_id}`를 호출한다. 서버가 리터럴 환경변수를 파싱해 Vault에 보관하고 다시 읽어 내용과 버전을 확인한다. 반환값은 불투명 묶음 ID, 원본 해시, 변수 이름과 저장 검증 여부다.
4. Handler는 `credential_bindings`에 연결 종류·주소·사용할 환경변수 이름과 설정 원본을 정한다. Agent는 `credential.check {bundle_id,binding_index:0}`로 Work Order에 고정된 연결의 신원 API를 호출한다. 저장된 프리셋 없이 실행할 수 있다. 값과 응답 본문은 서버 밖으로 반환하지 않고 검증 상태·HTTP 상태·관찰 ID를 남긴다.
5. Agent는 비밀값 없는 보고서를 생성한다. Handler는 `work_order.read.tool_evidence`에서 서버가 보관한 저장 영수증(`audit_seq`)·인증 관찰(`check_id`)을 직접 조회해 결과를 평가한다. 저장 성공과 외부 인증 성공은 구분한다. 어댑터 자체가 없거나 실제 업무 대상이 빠진 경우만 그 한계를 명시하고 `partial`로 보고한다. 프리셋·중앙 지식이 없다는 이유로 설치된 도구를 포기하지 않는다. 기존 차단 실행·Work Order는 보존한다. Brief의 `Handler 다시 실행`은 같은 목표와 첨부로 새 실행을 만든다. 새 지시 예산은 이전 Work Order 수를 포함한다.

`partial`은 업무 결과와 Task 상태다. 실행 루프의 `completed`와 다르며, 파일 산출을 요구한 계약은 부분 완료에서도 실제 산출물 ID가 필요하다. Handler는 Agent의 미완료 결과를 전체 완료로 올릴 수 없다.

## 지원 형식과 연결

입력은 UTF-8 리터럴 `KEY=VALUE` 형식이다. 빈 줄·주석·`export `·단일/이중 따옴표를 허용하고, 셸 실행이나 환경변수 확장은 하지 않는다. 파일당 1 MiB, 최대 256개 변수이며 중복 변수, 여러 줄 값, 제어 문자는 거부한다. 임의 문서·PEM 파일·셸 스크립트를 자동 해석해 등록하지 않는다. 환경변수 이름 기반 탐지는 모든 파일 형식의 비밀을 알아내는 범용 탐지기가 아니다.

| 커넥터 | 서버의 고정 요청 | 사용 필드 |
| --- | --- | --- |
| GitLab 토큰 | `GET /api/v4/user` | 토큰 변수 이름 |
| Jenkins API 토큰 | `GET /whoAmI/api/json` | 사용자 이름·API 토큰 변수 이름 |
| Databricks 토큰 | `GET /api/2.0/preview/scim/v2/Me` | 토큰 변수 이름 |

연결의 기본 주소는 설치 경로를 포함할 수 있다. Handler는 목표에 명시된 주소 또는 선택한 설정 파일의 주소를 사용한다. `source_artifact_id`와 `target_variable`을 지정하면 서버가 원본 설정의 주소와 대조한다. 다른 Task의 입력은 같은 업무실에서 선택한 기존 Vault 묶음의 원본인 경우에만 재사용한다. Handler가 Task 안에서 내부망·HTTP 여부를 정하며 기존 DNS 고정·리다이렉트 금지·내부 제어 서비스 차단을 유지한다. 모델이 호출 인수에서 임의 주소·인증 헤더를 덧붙일 수 없다. 관찰은 해당 신원 API의 시점별 증거이며 다른 서비스 권한까지 증명하지 않는다.

하나의 파일에 여러 계정이 있으면 Handler가 필요한 Task 연결을 각각 선택한다. AWS, SSH, DB, SMTP, 비밀번호 로그인 등의 인증 어댑터는 아직 구현하지 않았다. 이를 권한 부족이나 승인 필요로 설명하지 않는다. 값의 보관은 가능하지만 실제 접근 검증 성공으로 표시하지 않는다. OAuth 모델 연결과 업무용 기밀은 별개다.

API 의미는 [GitLab 사용자 API](https://docs.gitlab.com/api/users/), [GitLab 토큰 인증](https://docs.gitlab.com/api/rest/authentication/), [Jenkins whoAmI 설명](https://www.jenkins.io/doc/book/security/access-control/permissions/), [Databricks 현재 사용자 API](https://docs.databricks.com/api/scim/v1/workspace-user)를 따른다.

## 권한·보존·재사용

- Analyst와 Master는 적재·인증 확인을 실행할 수 없다. Handler는 목록을 보며 위임하고, Agent만 Work Order에 승인된 입력과 도구로 적재한다. 모든 역할의 원시 Vault 읽기는 계속 금지한다.
- 기본 실행은 `credential_bindings`를 사용한다. Handler가 연결을 정하고 Work Order가 그 설정·순서를 고정한다. Task에서 설정을 바꾸거나 제거하면 이전 지시로 실행할 수 없다. `credential_connectors:[{id,version}]`과 `connector_id`는 이전 저장 프리셋과의 호환용이며 필수 조건이 아니다.
- 다음 Task의 Handler는 `credential_bundles:[uuid]`로 같은 업무실의 기존 묶음을 직접 선택한다. 사용자에게 별도 등록·승인을 요청하지 않는다. 다른 업무실이나 모델 접근이 차단된 원본 Task의 묶음은 사용할 수 없다.
- 같은 원본 파일의 적재는 같은 Vault 묶음으로 조정하고 버전을 불필요하게 늘리지 않는다. 같은 Work Order·묶음·커넥터 버전의 인증 확인은 저장된 관찰을 반환한다. 관찰에는 당시 연결 종류·기본 주소·변수 이름의 값 없는 스냅샷도 고정한다. 인증 거부·실패·미검증 이력도 삭제하거나 성공으로 바꾸지 않는다.
- PostgreSQL에는 묶음·관찰 메타데이터를 저장한다. 최초 업로드 원본은 기존 불변 아티팩트 저장소에 보존되므로, Vault로 옮겼다는 이유로 원본 DB·백업에 비밀이 없다고 간주하지 않는다. 운영자 다운로드는 원래 파일을 반환한다. 역할 미리보기와 Worker에는 이 원본을 전달하지 않는다.

조회 API는 `GET /v1/operator/credentials?tenant_id=...`다. 묶음 참조·변수 이름·저장 버전과 최근 관찰을 제공한다. 선택적 프리셋의 기존 `POST /v1/operator/credential-connectors`는 호환용으로 유지한다. Task 연결의 예시는 다음과 같다.

```json
{"credential_bindings":[{"kind":"jenkins","base_url":"https://ci.example/jenkins","username_variable":"JENKINS_USER","token_variable":"JENKINS_API_TOKEN","source_artifact_id":"11111111-1111-4111-8111-111111111111","target_variable":"JENKINS_URL"}]}
```

## 검증 방법

`scripts/Verify-TaskBindings.ps1 -Live`는 별도 검증 Compose 프로젝트·포트 18432에서 소유한 합성 파일과 Jenkins fixture를 사용한다. 프리셋 0개 상태의 설정 주소 선택·실제 신원 요청·Work Order 증거·다음 Task 재사용·주소 불일치·연결 변경 거부를 검사한다. 실제 LLM에는 업무 목표와 첨부만 제공하고 Handler 설정부터 보고서·독립 결과 조회까지 확인한다. 기존 `Verify-Credentials.ps1`는 이전 프리셋 호환·역할·Vault 왕복·실패 보존·마스킹을 검사한다. 실제 사용자 기밀의 적재·인증을 대신 수행하는 검사는 아니다.
