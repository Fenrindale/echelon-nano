# 첨부 기밀의 적재와 인증 확인

목표와 첨부파일만으로 Handler가 안전한 목록을 확인하고 Agent에게 Vault 적재를 지시한다. 운영자가 실제 첨부파일을 대신 등록하거나 모델에 값을 붙여 넣을 필요가 없다. 인증 대상의 승인 설정은 업무실의 Vault 화면에서 한 번 준비하며, Handler는 그중 필요한 연결과 버전을 고른다.

## 실행 흐름

1. Handler가 `credential.inspect {artifact_id}` 또는 `artifact.read`로 변수 이름·개수·빈 값 목록을 본다. `.env` 기밀의 모든 값과 주석, 원본 base64는 역할 응답에서 제외한다.
2. Handler가 `credential.import`, 필요한 `credential.check`, `artifact.write`와 정확한 입력 파일을 계약에 넣고 Agent에게 위임한다. 중앙 지식의 유무는 설치된 도구의 사용 가능 여부와 별개다.
3. Agent가 `credential.import {artifact_id}`를 호출한다. 서버가 리터럴 환경변수를 파싱해 Vault에 보관하고 다시 읽어 내용과 버전을 확인한다. 반환값은 불투명 묶음 ID, 원본 해시, 변수 이름과 저장 검증 여부다.
4. 승인된 커넥터가 있으면 `credential.check {bundle_id,connector_id}`로 고정된 읽기 전용 신원 API를 호출한다. 값과 응답 본문은 서버 밖으로 반환하지 않고 검증 상태·HTTP 상태·관찰 ID만 남긴다.
5. Agent는 비밀값 없는 보고서 파일을 생성하고 Handler가 결과를 평가한다. 저장 성공과 외부 인증 성공을 구분한다. 일부 인증 어댑터나 커넥터가 없으면 가능한 적재를 수행하고 남은 범위를 `partial`로 보고한다. 기존 차단 실행·Work Order를 덮어쓰지 않는다. Brief의 `Handler 다시 실행`은 같은 목표와 첨부를 보존하며 새 실행을 만든다. 새 Work Order 예산은 이전 지시 수를 포함한다.

`partial`은 업무 결과와 Task 상태다. 실행 루프의 `completed`와 다르며, 파일 산출을 요구한 계약은 부분 완료에서도 실제 산출물 ID가 필요하다. Handler는 Agent의 미완료 결과를 전체 완료로 올릴 수 없다.

## 지원 형식과 연결

입력은 UTF-8 리터럴 `KEY=VALUE` 형식이다. 빈 줄·주석·`export `·단일/이중 따옴표를 허용하고, 셸 실행이나 환경변수 확장은 하지 않는다. 파일당 1 MiB, 최대 256개 변수이며 중복 변수, 여러 줄 값, 제어 문자는 거부한다. 임의 문서·PEM 파일·셸 스크립트를 자동 해석해 등록하지 않는다. 환경변수 이름 기반 탐지는 모든 파일 형식의 비밀을 알아내는 범용 탐지기가 아니다.

| 커넥터 | 서버의 고정 요청 | 사용 필드 |
| --- | --- | --- |
| GitLab 토큰 | `GET /api/v4/user` | 토큰 변수 이름 |
| Jenkins API 토큰 | `GET /whoAmI/api/json` | 사용자 이름·API 토큰 변수 이름 |
| Databricks 토큰 | `GET /api/2.0/preview/scim/v2/Me` | 토큰 변수 이름 |

커넥터의 기본 주소는 설치 경로를 포함할 수 있다. 쿼리·URL 내 인증정보·프래그먼트는 허용하지 않는다. 내부망·HTTP는 운영자가 명시적으로 승인해야 하며 기존 DNS 고정·리다이렉트 금지·내부 제어 서비스 차단을 적용한다. 모델이 임의 주소, 인증 헤더나 토큰 필드를 요청 인수로 추가할 수 없다. 인증 관찰은 해당 신원 API에 대한 시점별 증거이며 다른 서비스의 권한을 증명하지 않는다.

기본 주소와 변수 이름을 실제 서비스에 맞게 설정해야 한다. 하나의 파일에 여러 계정이 있으면 계정별 커넥터를 등록한다. AWS, SSH, DB, SMTP, 비밀번호 로그인 등의 인증 어댑터는 현재 미지원이다. 값의 보관은 가능하지만 실제 접근 검증 성공으로 표시하지 않는다. OAuth 모델 연결과 이 업무용 기밀 흐름은 별개다.

API 의미는 [GitLab 사용자 API](https://docs.gitlab.com/api/users/), [GitLab 토큰 인증](https://docs.gitlab.com/api/rest/authentication/), [Jenkins whoAmI 설명](https://www.jenkins.io/doc/book/security/access-control/permissions/), [Databricks 현재 사용자 API](https://docs.databricks.com/api/scim/v1/workspace-user)를 따른다.

## 권한·보존·재사용

- Analyst와 Master는 적재·인증 확인을 실행할 수 없다. Handler는 목록을 보며 위임하고, Agent만 Work Order에 승인된 입력과 도구로 적재한다. 모든 역할의 원시 Vault 읽기는 계속 금지한다.
- Vault 화면의 `인증 커넥터`는 운영자만 등록·수정한다. 값은 입력하지 않고 사용할 환경변수 이름을 지정한다. 계약은 `credential_connectors:[{id,version}]`으로 버전을 고정하며, 수정되거나 비활성화된 연결은 이전 계약에서 사용할 수 없다.
- 다음 Task는 `credential_bundles:[uuid]`로 같은 업무실의 기존 묶음을 명시적으로 승인받아 참조한다. 다른 업무실이나 모델 접근이 차단된 원본 Task의 묶음은 선택할 수 없다.
- 같은 원본 파일의 적재는 같은 Vault 묶음으로 조정하고 버전을 불필요하게 늘리지 않는다. 같은 Work Order·묶음·커넥터 버전의 인증 확인은 저장된 관찰을 반환한다. 관찰에는 당시 연결 종류·기본 주소·변수 이름의 값 없는 스냅샷도 고정한다. 인증 거부·실패·미검증 이력도 삭제하거나 성공으로 바꾸지 않는다.
- PostgreSQL에는 묶음·관찰 메타데이터를 저장한다. 최초 업로드 원본은 기존 불변 아티팩트 저장소에 보존되므로, Vault로 옮겼다는 이유로 원본 DB·백업에 비밀이 없다고 간주하지 않는다. 운영자 다운로드는 원래 파일을 반환한다. 역할 미리보기와 Worker에는 이 원본을 전달하지 않는다.

운영자 API는 `GET /v1/operator/credentials?tenant_id=...`, `POST /v1/operator/credential-connectors`다. 수정은 `id`와 `expected_version`을 요구한다. 결과 목록에는 묶음 참조·변수 이름·저장 버전과 최근 인증 관찰만 제공한다.

## 검증 방법

`scripts/Verify-Credentials.ps1 -Live`는 포트 18432의 별도 검증 Compose 프로젝트에서만 동작한다. 소유한 합성 파일과 Jenkins 신원 fixture로 역할 거부·다른 Task 입력 거부·Vault 왕복·멱등 재사용·커넥터 버전 고정·익명 HTTP 200 거부·원본 마스킹·Worker 차단을 검사한다. 실제 연결 계정의 LLM으로 승인 연결이 있는 경우와 없는 경우를 각각 실행한다. 모델 체크포인트와 보고서에 합성 비밀값이 없는지도 확인한다. 이것은 실제 회사 서비스나 사용자 첨부파일의 인증 결과가 아니다.
