let csrf = "";
export function setCsrf(value: string) {
  csrf = value;
}
const messages: Record<string, string> = {
  unauthorized: "운영 서버 연결을 새로 확인해 주세요.",
  csrf_denied: "연결 정보를 새로 확인해 주세요.",
  origin_denied: "로컬 운영 화면에서 요청해 주세요.",
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
  return crypto.randomUUID();
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
  failed: "실패",
  cancelled: "취소",
  interrupted: "중단됨",
  exhausted: "예산 소진",
  pending: "검토 대기",
  accepted: "채택",
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
  "run.retention_done": "재사용 지식 선별 완료",
  "run.adoption_done": "중앙 반영 검토 완료",
  "run.dream_done": "Master 감사 완료",
};
