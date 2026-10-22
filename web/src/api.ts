let csrf = "";
export function setCsrf(value: string) {
  csrf = value;
}
const messages: Record<string, string> = {
  mcp_host_operation_failed: "MCP 설치 또는 초기화에 실패했습니다. 패키지 버전·실행 파일·Vault 변수 연결을 확인하세요.",
  mcp_package_install_failed: "MCP 패키지를 설치하지 못했습니다. 패키지 이름·정확한 버전·런타임 의존성을 확인하세요.",
  mcp_process_closed: "MCP 프로세스가 초기화 또는 실행 중 종료됐습니다. 실행 파일·인자·Vault 변수 연결을 확인하세요.",
  mcp_timeout: "MCP가 실행 시간 안에 응답하지 않았습니다. 변경 작업은 외부 상태 확인 전까지 다시 실행하지 않습니다.",
  mcp_not_ready: "이 MCP는 연결 확인이 필요합니다.",
  mcp_credentials_require_vault: "인자·주소에 비밀번호나 토큰을 넣을 수 없습니다. Vault 변수 연결을 사용하세요.",
  mcp_version_conflict: "MCP 설정이 변경됐습니다. 현재 설정을 다시 열어 주세요.",
  mcp_outcome_unknown_do_not_retry: "MCP 실행 결과가 불명확합니다. 외부 상태 확인 전에는 자동 재실행하지 않습니다.",
  unauthorized: "운영 서버 연결을 새로 확인해 주세요.",
  csrf_denied: "연결 정보를 새로 확인해 주세요.",
  origin_denied: "로컬 운영 화면에서 요청해 주세요.",
  model_connection_required: "Settings에서 사용할 모델 계정을 먼저 연결해 주세요.",
  goal_launch_conflict: "업무 목표나 첨부파일이 변경됐습니다. 새 요청으로 다시 시작해 주세요.",
  attachment_budget_exceeded: "첨부파일은 최대 16개, 합계 4 MiB까지 올릴 수 있습니다.",
  attachment_duplicate: "같은 이름이나 요청 ID의 첨부파일이 있습니다.",
  artifact_budget_exceeded: "파일당 1 MiB 이하여야 합니다.",
  credential_format_unsupported: "지원하는 단일 행 KEY=VALUE 형식의 기밀 파일인지 확인해 주세요.",
  credential_assignment_invalid: "기밀 변수 이름이 잘못됐거나 중복되어 있습니다.",
  credential_connector_invalid: "서비스 주소와 API 토큰 변수 이름을 확인해 주세요.",
  credential_connector_changed: "연결 프리셋이 변경됐습니다. Handler가 현재 버전으로 설정을 확인해야 합니다.",
  credential_connector_not_granted: "이 Work Order에서 선택한 연결 프리셋을 찾을 수 없습니다. Handler가 Task 연결 설정을 확인해야 합니다.",
  credential_binding_invalid: "Task 연결의 서비스 주소와 기밀 변수 이름을 Handler가 확인해야 합니다.",
  credential_binding_changed: "Task 연결이 변경됐습니다. Handler가 현재 설정으로 Work Order를 만들어야 합니다.",
  credential_binding_not_in_work_order: "Work Order에 해당 Task 연결이 없습니다. Handler가 연결 설정을 확인해야 합니다.",
  credential_target_not_in_task: "연결 대상이 업무 목표나 선택한 설정 파일과 일치하지 않습니다. Handler가 원본 주소를 확인해야 합니다.",
  credential_material_not_for_shell: "기밀 원문은 shell에 전달할 수 없습니다. 전용 Vault 가져오기 도구를 사용해 주세요.",
  credential_version_unconfirmed: "Vault의 기밀 버전을 확인할 수 없습니다. 기존 기록을 확인해 주세요.",
  pause_execution_before_edit: "실행을 일시정지한 뒤 계약을 수정해 주세요.",
  invalid_execution_transition: "현재 상태에서는 선택한 전환을 할 수 없습니다.",
  execution_budget_exceeded: "설정한 실행 예산을 소진했습니다.",
  execution_admission_busy:
    "다른 실행을 등록 중입니다. 잠시 후 다시 시도해 주세요.",
  execution_queue_full: "실행 대기열이 가득 찼습니다.",
  knowledge_version_conflict:
    "새 개정이 있습니다. 최신 내용을 확인한 뒤 다시 제출해 주세요.",
  protocol_version_conflict:
    "프로토콜이 변경됐습니다. 최신 버전으로 다시 검토해 주세요.",
  external_model_policy_denied: "외부 모델 전송이 금지된 정보입니다.",
  source_model_policy_changed:
    "참조 정보의 전송 정책이 변경되어 실행을 중단했습니다.",
  model_not_in_account_catalog:
    "선택한 모델을 현재 계정에서 확인할 수 없습니다.",
  plan_usage_not_granted: "ChatGPT 플랜 사용 동의가 필요합니다.",
  inference_failed: "모델 실행이 완료되지 않았습니다.",
  inference_rejected:
    "모델이 요청을 수락하지 않았습니다. 계정 상태와 사용 한도를 확인해 주세요.",
  knowledge_storage_invalid:
    "저장 방식에 맞는 출처 또는 참조 관계를 지정해 주세요.",
  negative_evidence_required:
    "거짓·실패 정보에는 조건과 관찰·재시도 근거가 필요합니다.",
  task_execution_active: "이 Task에는 진행 중인 실행이 있습니다.",
  work_order_budget_exceeded: "Task의 Work Order 예산을 소진했습니다.",
  work_url_denied: "이 주소는 Work Order에 허용되지 않았습니다.",
  broker_network_scope_denied: "대상의 네트워크 범위가 허용되지 않았습니다.",
};
export class ApiError extends Error {
  constructor(
    public code: string,
    public status: number,
  ) {
    super(messages[code] ?? `요청을 처리하지 못했습니다. (${code})`);
  }
}
export async function api<T = any>(
  path: string,
  method = "GET",
  body?: unknown,
): Promise<T> {
  const response = await fetch(path, {
    method,
    credentials: "same-origin",
    headers: {
      ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
      ...(method !== "GET" && csrf ? { "X-Echelon-Csrf": csrf } : {}),
    },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const data = await response.json().catch(() => ({}));
  if (!response.ok) {
    if (response.status === 401)
      window.dispatchEvent(new Event("echelon-auth-lost"));
    throw new ApiError(data.error?.code ?? "request_failed", response.status);
  }
  return data as T;
}
export function query(
  values: Record<string, string | number | boolean | undefined | null>,
) {
  return new URLSearchParams(
    Object.entries(values)
      .filter(([, v]) => v !== undefined && v !== null)
      .map(([k, v]) => [k, String(v)]),
  ).toString();
}
export function domain(
  tenant: string,
  task: string,
  tool: string,
  input: unknown,
) {
  return api("/v1/operator/domain", "POST", {
    tenant_id: tenant,
    task_id: task,
    tool,
    input,
  });
}
export function key() {
  if (typeof crypto.randomUUID === "function") return crypto.randomUUID();
  // Plain HTTP LAN origins lack randomUUID; getRandomValues remains available.
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  bytes[6] = (bytes[6] & 0x0f) | 0x40;
  bytes[8] = (bytes[8] & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}
export function short(id?: string) {
  return id?.slice(0, 8) ?? "—";
}
export function taskTitle(goal: string) {
  const title = goal.split(/\n|(?<=[.!?])\s/)[0];
  return title.length > 90 ? `${title.slice(0, 90)}…` : title;
}
export function formatNumber(n?: number) {
  return (n ?? 0).toLocaleString("ko-KR");
}
export function date(s?: string) {
  return s
    ? new Date(s).toLocaleString("ko-KR", {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      })
    : "—";
}
export const labels: Record<string, string> = {
  draft: "초안",
  ready: "준비",
  queued: "대기열",
  running: "실행 중",
  waiting_input: "입력 대기",
  paused: "일시정지",
  blocked: "차단됨",
  completed: "완료",
  done: "처리 완료",
  delivered: "전달됨",
  recorded: "기록됨",
  input: "입력 파일",
  failed: "실패",
  cancelled: "취소",
  interrupted: "중단됨",
  exhausted: "예산 소진",
  pending: "검토 대기",
  accepted: "최초 중앙 채택",
  local: "Task 지역 보관",
  task: "Task 지역 지식",
  central: "중앙 지식",
  rejected: "반려",
  applied: "적용",
  candidate: "후보",
  testing: "검증 중",
  active: "활성",
  suspended: "일시중지",
  retired: "폐기",
  unverified: "미검증",
  supported: "근거 있음",
  refuted: "반증됨",
  conflicted: "상충",
  success: "성공",
  partial: "부분 성공",
  stored: "Vault 보관됨",
  verified: "인증 확인",
  authentication_rejected: "인증 거부",
  access_denied: "권한 부족",
  not_applicable: "해당 없음",
  current: "유효",
  review_needed: "재확인 필요",
  expired: "만료",
  superseded: "대체됨",
};
export const actions: Record<string, string> = {
  "model.started": "모델 판단 시작",
  "model.completed": "모델 판단 완료",
  "tool.pending": "도구 요청",
  "tool.completed": "도구 처리 완료",
  "tool.denied": "권한 거부",
  "tool.failed": "도구 처리 실패",
  "protocol.direct_load": "프로토콜 정보셋 직접 로드",
  "context.request": "정보 요청",
  "context.curate": "근거 선별",
  "context.read": "고정 근거 조회",
  "task.plan": "계획 수립",
  "agent.delegate": "Work Order 지시",
  "work_order.read": "Work Order·결과 조회",
  "work_order.complete": "결과 반환",
  "artifact.write": "산출물 보관",
  "artifact.read": "파일 조회",
  "external.fetch": "허용된 API 읽기",
  "shell.exec": "샌드박스 실행",
  "knowledge.submit": "지식 제출",
  "knowledge.publish": "지식 채택·반려",
  "knowledge.optimize": "지식 정리",
  "protocol.manage": "프로토콜 제안",
  "run.finish": "업무 결과 확정",
  "run.configure": "Handler 실행 설정",
  "run.configure_contract": "Handler 업무 계약 확정",
  "run.retention_done": "재사용 지식 선별 완료",
  "run.adoption_done": "중앙 반영 검토 완료",
  "run.dream_done": "Master 감사 완료",
};
