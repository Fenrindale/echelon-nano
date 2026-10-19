import { useState } from "react";
import {
  api,
  domain,
  key,
  query,
  date,
  actions,
  formatNumber,
  taskTitle,
} from "./api";
import { useWorkspace } from "./context";
import {
  Badge,
  Empty,
  ErrorBox,
  Field,
  ID,
  JsonView,
  Modal,
  Role,
  SubmitForm,
  Tags,
  useAsync,
  type Json,
} from "./components";

const lines = (v: string) =>
  v
    .split("\n")
    .map((s) => s.trim())
    .filter(Boolean);
export function Tasks({ onNew }: { onNew: () => void }) {
  const w = useWorkspace();
  const [search, setSearch] = useState("");
  const [archived, setArchived] = useState(false);
  const [offset, setOffset] = useState(0);
  const r = useAsync(
    () =>
      api(
        `/v1/operator/tasks?${query({ tenant_id: w.tenant, query: search, archived, offset })}`,
      ),
    [w.tenant, w.revision, search, archived, offset],
  );
  const tasks: Json[] = r.data?.tasks ?? [];
  return (
    <>
      <div className="page-title">
        <div>
          <p className="eyebrow">OPERATIONS / TASKS</p>
          <h1>업무를 시작하세요.</h1>
          <p className="muted">
            목표와 완료 조건을 정하면, 필요한 지식에서 실행까지 이어집니다.
          </p>
        </div>
        <button className="primary" onClick={onNew}>
          ＋ 새 Task
        </button>
      </div>
      <div className="toolbar">
        <input
          aria-label="Task 검색"
          placeholder="업무 목표 검색"
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            setOffset(0);
          }}
        />
        <label className="check">
          <input
            type="checkbox"
            checked={archived}
            onChange={(e) => {
              setArchived(e.target.checked);
              setOffset(0);
            }}
          />
          보관한 업무
        </label>
        <span className="muted">{tasks.length}개 표시</span>
      </div>
      <ErrorBox error={r.error} />
      {!tasks.length ? (
        <Empty title="아직 등록된 업무가 없습니다">
          작은 업무 하나부터 시작해 보세요.
        </Empty>
      ) : (
        <div className="task-list">
          {tasks.map((t) => (
            <button
              className="task-card"
              key={t.id}
              onClick={() => w.openTask(t.id)}
            >
              <div className="task-symbol">◫</div>
              <div className="task-card-body">
                <div className="row">
                  <Badge value={t.status} />
                  <ID id={t.id} />
                  <small>{date(t.updated_at)}</small>
                </div>
                <h3>{taskTitle(t.goal)}</h3>
                <p>
                  {t.contract.output_format || "결과 요약"} ·{" "}
                  {t.contract.completion_conditions?.length ?? 0}개 완료 조건
                </p>
              </div>
              <span className="arrow">↗</span>
            </button>
          ))}
        </div>
      )}
      <div className="pagination">
        <button
          disabled={!offset}
          onClick={() => setOffset(Math.max(0, offset - 100))}
        >
          이전
        </button>
        <button
          disabled={tasks.length < 100}
          onClick={() => setOffset(offset + 100)}
        >
          다음
        </button>
      </div>
    </>
  );
}
export function ContractFields({
  value,
  onChange,
}: {
  value: Json;
  onChange: (v: Json) => void;
}) {
  const w = useWorkspace();
  const secrets = useAsync(
    () => api(`/v1/operator/secrets?${query({ tenant_id: w.tenant })}`),
    [w.tenant, w.revision],
  );
  const set = (k: string, v: unknown) => onChange({ ...value, [k]: v });
  const tools: string[] = value.allowed_tools ?? [];
  return (
    <>
      <div className="form-grid">
        <Field label="산출물 형식">
          <input
            value={value.output_format ?? ""}
            onChange={(e) => set("output_format", e.target.value)}
            placeholder="예: Markdown 보고서, 수정된 Python 파일"
            maxLength={500}
          />
        </Field>
        <Field label="Work Order 최대 개수">
          <input
            type="number"
            min={1}
            max={8}
            value={value.max_work_orders || 2}
            onChange={(e) => set("max_work_orders", Number(e.target.value))}
          />
        </Field>
      </div>
      <Field label="완료 조건 · 한 줄에 하나">
        <textarea
          defaultValue={(value.completion_conditions ?? []).join("\n")}
          onChange={(e) => set("completion_conditions", lines(e.target.value))}
          placeholder="수치에는 출처를 기재한다"
        />
      </Field>
      <Field label="제약 조건 · 한 줄에 하나">
        <textarea
          defaultValue={(value.constraints ?? []).join("\n")}
          onChange={(e) => set("constraints", lines(e.target.value))}
          placeholder="외부 시스템 변경 금지"
        />
      </Field>
      <Field label="허용 도구">
        <div className="checks">
          {[
            ["artifact.write", "파일 작성"],
            ["shell.exec", "격리 코드·파일 실행"],
            ["external.fetch", "지정 URL·API 읽기"],
          ].map(([id, label]) => (
            <label className="check" key={id}>
              <input
                type="checkbox"
                checked={tools.includes(id)}
                onChange={(e) =>
                  set(
                    "allowed_tools",
                    e.target.checked
                      ? [...tools, id]
                      : tools.filter((x) => x !== id),
                  )
                }
              />
              {label}
            </label>
          ))}
        </div>
      </Field>
      {tools.includes("external.fetch") && (
        <>
          <Field
            label="허용 URL · 정확히 일치하는 주소, 한 줄에 하나"
            hint="다른 주소로의 이동은 허용되지 않습니다. 내부 서비스는 별도 범위 승인이 필요합니다."
          >
            <textarea
              defaultValue={(value.urls ?? [])
                .map((u: Json) => u.url)
                .join("\n")}
              onChange={(e) =>
                set(
                  "urls",
                  lines(e.target.value).map((url) => {
                    const old = value.urls?.find((u: Json) => u.url === url);
                    return (
                      old ?? { url, allow_private: false, allow_http: false }
                    );
                  }),
                )
              }
              placeholder="https://example.com/api/status"
            />
          </Field>
          {value.urls?.map((u: Json, i: number) => (
            <div className="url-grant" key={i}>
              <code>{u.url}</code>
              {["allow_private", "allow_http"].map((flag) => (
                <label className="check" key={flag}>
                  <input
                    type="checkbox"
                    checked={u[flag] ?? false}
                    onChange={(e) =>
                      set(
                        "urls",
                        value.urls.map((v: Json, j: number) =>
                          j === i ? { ...v, [flag]: e.target.checked } : v,
                        ),
                      )
                    }
                  />
                  {flag === "allow_private" ? "내부망 범위 승인" : "HTTP 허용"}
                </label>
              ))}
            </div>
          ))}
          <Field label="이 Task에서 사용 가능한 기밀 참조">
            <div className="checks">
              {secrets.data?.secrets
                ?.filter((s: Json) => s.enabled)
                .map((s: Json) => (
                  <label className="check" key={s.id}>
                    <input
                      type="checkbox"
                      checked={value.secret_refs?.includes(s.id) ?? false}
                      onChange={(e) =>
                        set(
                          "secret_refs",
                          e.target.checked
                            ? [...(value.secret_refs ?? []), s.id]
                            : (value.secret_refs ?? []).filter(
                                (id: string) => id !== s.id,
                              ),
                        )
                      }
                    />
                    {s.name} <ID id={s.id} />
                  </label>
                ))}
            </div>
          </Field>
        </>
      )}
      <Field label="검증 방법">
        <textarea
          value={value.verification ?? ""}
          onChange={(e) => set("verification", e.target.value)}
          placeholder="예: 격리 환경에서 단위 검사를 실행하고 결과를 보고한다"
        />
      </Field>
      <label className="check">
        <input
          type="checkbox"
          checked={value.model_access === "blocked"}
          onChange={(e) =>
            set(
              "model_access",
              e.target.checked ? "blocked" : "external_allowed",
            )
          }
        />
        외부 모델 전송 금지 · 이 Task와 파생 지식을 모델에 보내지 않음
      </label>
    </>
  );
}
export function TaskForm({
  onCreated,
  initialGoal = "",
  initialContract = {},
}: {
  onCreated: (id: string) => void;
  initialGoal?: string;
  initialContract?: Json;
}) {
  const w = useWorkspace();
  const [goal, setGoal] = useState(initialGoal);
  const [contract, setContract] = useState<Json>({
    kind: "work",
    model_access: "external_allowed",
    allowed_tools: ["artifact.write"],
    max_work_orders: 2,
    ...initialContract,
  });
  return (
    <SubmitForm
      button="Task 만들기"
      onSubmit={async () => {
        const r = await api("/v1/operator/tasks", "POST", {
          tenant_id: w.tenant,
          tenant_name: w.tenantName,
          goal,
          contract,
        });
        w.refresh();
        onCreated(r.task_id);
      }}
    >
      <Field label="업무 목표">
        <textarea
          autoFocus
          required
          value={goal}
          onChange={(e) => setGoal(e.target.value)}
          maxLength={4000}
          placeholder="무엇을, 어떤 수준까지 완료해야 하나요?"
        />
      </Field>
      <ContractFields value={contract} onChange={setContract} />
    </SubmitForm>
  );
}
export function RunForm({
  task,
  protocol,
  onStarted,
  mode = "work",
}: {
  task: string;
  protocol?: Json;
  onStarted: () => void;
  mode?: string;
}) {
  const w = useWorkspace();
  const [target, setTarget] = useState(`task:${task}`);
  const [calls, setCalls] = useState(48);
  const [tokens, setTokens] = useState(160000);
  const [retain, setRetain] = useState(true);
  const [dream, setDream] = useState(true);
  const [params, setParams] = useState<Json>({});
  return (
    <SubmitForm
      button={mode === "dream" ? "Master 감사 시작" : "실행 시작"}
      onSubmit={async () => {
        if (!w.connection || !w.model)
          throw new Error("설정에서 계정과 모델을 선택해 주세요.");
        await api("/v1/operator/runs", "POST", {
          request_id: key(),
          tenant_id: w.tenant,
          task_id: task,
          connection_id: w.connection,
          model: w.model,
          target,
          max_calls: calls,
          max_total_tokens: tokens,
          mode,
          protocol_id: protocol?.id ?? null,
          protocol_version: protocol?.version ?? null,
          parameters: params,
          retain_knowledge: mode === "work" && retain,
          dream_after: mode === "work" && dream,
        });
        w.refresh();
        onStarted();
      }}
    >
      <p className="muted">
        {w.providers.find((p) => p.id === w.connection)?.name ?? w.connection} /{" "}
        {w.model || "모델 미선택"}
      </p>
      <Field label="계정">
        <select
          value={w.connection}
          onChange={(e) => w.setConnection(e.target.value)}
        >
          <option value="">선택</option>
          {w.providers
            .filter((p) => p.enabled !== false && p.status !== "revoked")
            .map((p) => (
              <option key={p.id} value={p.id}>
                {p.name ?? p.label ?? p.provider ?? "ChatGPT"} ·{" "}
                {p.id.slice(0, 8)}
              </option>
            ))}
        </select>
      </Field>
      <Field label="모델">
        <select value={w.model} onChange={(e) => w.setModel(e.target.value)}>
          <option value="">선택</option>
          {w.models.map((m) => (
            <option key={m.slug} value={m.slug}>
              {m.display_name ?? m.slug}
            </option>
          ))}
        </select>
      </Field>
      <Field label="허용된 작업 대상">
        <input
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          required
          maxLength={500}
        />
      </Field>
      <div className="form-grid">
        <Field label="모델 호출 한도">
          <input
            type="number"
            min={1}
            max={48}
            value={calls}
            onChange={(e) => setCalls(+e.target.value)}
          />
        </Field>
        <Field label="누적 토큰 한도">
          <input
            type="number"
            min={1000}
            max={300000}
            step={1000}
            value={tokens}
            onChange={(e) => setTokens(+e.target.value)}
          />
        </Field>
      </div>
      {protocol && (
        <>
          <p>
            프로토콜:{protocol.document.codename} · v{protocol.version}
          </p>
          {protocol.document.parameters?.map((p: Json) => (
            <Field key={p.name} label={p.name} hint={p.description}>
              <input
                required={p.required}
                value={params[p.name] ?? ""}
                onChange={(e) =>
                  setParams({ ...params, [p.name]: e.target.value })
                }
              />
            </Field>
          ))}
        </>
      )}
      {mode === "work" && (
        <div className="checks">
          <label className="check">
            <input
              type="checkbox"
              checked={retain}
              onChange={(e) => setRetain(e.target.checked)}
            />
            완료 후 Handler 제출 → Analyst 채택
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={dream}
              onChange={(e) => setDream(e.target.checked)}
            />
            완료 후 Master 감사·정리 제안
          </label>
        </div>
      )}
      <small className="muted">
        호출·토큰 예산과 권한은 서버에서 적용합니다. 실제 계정의 사용 한도
        오류는 실행 결과에 표시됩니다.
      </small>
    </SubmitForm>
  );
}
export function TaskDetail({
  id,
  events,
  onBack,
}: {
  id: string;
  events: Json[];
  onBack: () => void;
}) {
  const w = useWorkspace();
  const [tab, setTab] = useState("brief");
  const [runOpen, setRunOpen] = useState(false);
  const [edit, setEdit] = useState(false);
  const [note, setNote] = useState("");
  const [error, setError] = useState<unknown>();
  const [contract, setContract] = useState<Json>({});
  const [clone, setClone] = useState(false);
  const data = useAsync(
    () => api(`/v1/operator/tasks/${id}?${query({ tenant_id: w.tenant })}`),
    [id, w.revision],
  );
  const t = data.data;
  const latest = t?.runs?.[0];
  const run = useAsync(
    () =>
      latest ? api(`/v1/operator/runs/${latest.id}`) : Promise.resolve(null),
    [latest?.id, w.revision],
  );
  const knowledge = useAsync(
    () =>
      domain(w.tenant, w.admin, "knowledge.read", {
        op: "submissions",
        source_task_id: id,
        limit: 100,
      }),
    [id, w.revision],
  );
  async function act(action: string) {
    setError(undefined);
    try {
      await api(`/v1/operator/runs/${latest.id}/control`, "POST", { action });
      w.refresh();
    } catch (e) {
      setError(e);
    }
  }
  if (!t)
    return (
      <>
        <button className="quiet" onClick={onBack}>
          ← Tasks
        </button>
        <ErrorBox error={data.error} />
        <Empty title="Task를 불러오고 있습니다" />
      </>
    );
  const active = latest && ["queued", "running"].includes(latest.status);
  const canResume =
    latest && ["paused", "interrupted", "failed"].includes(latest.status);
  const ev = events.filter((e) => e.task_id === id);
  const result = run.data?.result;
  return (
    <>
      <button className="back quiet" onClick={onBack}>
        ← Tasks
      </button>
      <div className="page-title task-title">
        <div>
          <p className="eyebrow">
            TASK / <ID id={id} />
          </p>
          <h1>{taskTitle(t.goal)}</h1>
          <div className="row">
            <Badge value={t.status} />
            {latest && (
              <>
                <span>실행</span>
                <Badge value={latest.status} />
                <span>
                  {formatNumber(run.data?.total_tokens)} tokens · {latest.calls}{" "}
                  calls
                </span>
              </>
            )}
          </div>
        </div>
        <div className="button-group">
          {active ? (
            <>
              <button onClick={() => act("pause")}>일시정지</button>
              <button className="danger-button" onClick={() => act("cancel")}>
                취소
              </button>
            </>
          ) : (
            <>
              {canResume && (
                <button className="primary" onClick={() => act("resume")}>
                  이어 실행
                </button>
              )}
              {!["completed", "cancelled", "failed", "blocked"].includes(
                t.status,
              ) && (
                <button
                  className="primary"
                  disabled={t.contract.model_access === "blocked"}
                  onClick={() => setRunOpen(true)}
                >
                  실행
                </button>
              )}
              <button onClick={() => setClone(true)}>복제</button>
              <button
                className="quiet"
                onClick={async () => {
                  try {
                    await api(`/v1/operator/tasks/${id}`, "PATCH", {
                      tenant_id: w.tenant,
                      archived: !t.archived,
                      reason: "운영 화면에서 보관 상태 변경",
                    });
                    w.refresh();
                  } catch (e) {
                    setError(e);
                  }
                }}
              >
                {t.archived ? "보관 해제" : "보관"}
              </button>
            </>
          )}
        </div>
      </div>
      <ErrorBox error={error || run.error} />
      {latest?.error_code && (
        <div className="notice warning">
          실행 중단: {latest.error_code} · {run.data?.phase} 단계. 중단 시점과
          예산을 확인하고 이어 실행할 수 있습니다.
        </div>
      )}
      {t.status === "waiting_input" && (
        <div className="notice">
          추가 입력을 기다립니다. 아래에 답변을 남기고 이어 실행하세요.
        </div>
      )}
      <div className="tabs">
        {[
          ["brief", "Brief"],
          ["path", "Path"],
          ["broadcast", "Broadcast"],
          ["knowledge", "Knowledge"],
          ["outputs", "Outputs"],
        ].map(([name, label]) => (
          <button
            key={name}
            className={tab === name ? "selected" : ""}
            onClick={() => setTab(name)}
          >
            {label}
            {name === "outputs" && ` ${t.artifacts.length}`}
            {name === "knowledge" &&
              ` ${knowledge.data?.submissions?.length ?? 0}`}
          </button>
        ))}
      </div>
      <div className="task-workspace">
        <section className="work-pane">
          {tab === "brief" && (
            <>
              <section className="panel">
                <div className="section-heading">
                  <h2>업무 계약</h2>
                  <button
                    disabled={active}
                    onClick={() => {
                      setContract(t.contract);
                      setEdit(true);
                    }}
                  >
                    계약 수정
                  </button>
                </div>
                <p className="prose muted">{t.goal}</p>
                <dl className="brief">
                  <dt>산출물</dt>
                  <dd>{t.contract.output_format || "결과 요약"}</dd>
                  <dt>완료 조건</dt>
                  <dd>
                    <ul>
                      {t.contract.completion_conditions?.map((v: string) => (
                        <li key={v}>{v}</li>
                      ))}
                    </ul>
                  </dd>
                  <dt>제약</dt>
                  <dd>
                    <ul>
                      {t.contract.constraints?.map((v: string) => (
                        <li key={v}>{v}</li>
                      ))}
                    </ul>
                  </dd>
                  <dt>허용 도구</dt>
                  <dd>
                    <Tags tags={t.contract.allowed_tools} />
                  </dd>
                  <dt>외부 모델</dt>
                  <dd>
                    {t.contract.model_access === "blocked"
                      ? "전송 금지"
                      : "계정 기반 실행 허용"}
                  </dd>
                  <dt>검증</dt>
                  <dd>{t.contract.verification || "지정되지 않음"}</dd>
                </dl>
                {t.contract.urls?.length > 0 && (
                  <details>
                    <summary>승인된 URL·기밀 참조</summary>
                    <JsonView
                      value={{
                        urls: t.contract.urls,
                        secret_refs: t.contract.secret_refs,
                      }}
                    />
                  </details>
                )}
              </section>
              {result && (
                <section className="panel">
                  <h2>업무 결과</h2>
                  <Badge value={result.outcome} />
                  <p className="prose">{result.summary}</p>
                </section>
              )}
              <section className="panel">
                <h2>추가 입력</h2>
                {t.inputs.map((n: Json) => (
                  <div className="note" key={n.id}>
                    <small>{date(n.created_at)}</small>
                    <p>{n.content}</p>
                  </div>
                ))}
                <SubmitForm
                  button="입력 전달"
                  onSubmit={async () => {
                    await api(`/v1/operator/tasks/${id}/inputs`, "POST", {
                      tenant_id: w.tenant,
                      content: note,
                    });
                    setNote("");
                    w.refresh();
                  }}
                >
                  <Field label="보충 설명·답변">
                    <textarea
                      value={note}
                      onChange={(e) => setNote(e.target.value)}
                      required
                      maxLength={8000}
                    />
                  </Field>
                </SubmitForm>
              </section>
            </>
          )}
          {tab === "path" && (
            <section className="panel">
              <h2>업무 경로</h2>
              <p className="muted">
                이 Task의 지시·고정 근거·결과만 표시합니다.
              </p>
              <div className="path-start">
                <Role role="Handler" />
                <span>목표 → 정보 요청 → Context Pack</span>
              </div>
              {t.work_orders.map((o: Json) => (
                <div className="path-order" key={o.id}>
                  <div className="row">
                    <Role role="Agent" />
                    <ID id={o.id} />
                    <Badge value={o.status} />
                  </div>
                  <h3>{o.contract.objective}</h3>
                  <p>{o.target}</p>
                  <div className="path-refs">
                    {o.contract.evidence?.map((r: Json, i: number) => {
                      const ref = r.resolved.reference;
                      return (
                        <button
                          key={i}
                          onClick={() => {
                            if (ref.item_id)
                              w.openKnowledge(ref.item_id, ref.version);
                          }}
                        >
                          {r.resolved.document?.title ??
                            ref.title ??
                            `고정 참조 ${i + 1}`}{" "}
                          · v{ref.version ?? ref.item_version ?? "—"}
                        </button>
                      );
                    })}
                  </div>
                  {o.result && (
                    <div className="path-result">
                      <Badge value={o.result.outcome} />
                      <p>{o.result.summary}</p>
                    </div>
                  )}
                  <details>
                    <summary>Work Order 계약·근거</summary>
                    <JsonView value={o.contract} />
                  </details>
                </div>
              ))}
              {!t.work_orders.length && (
                <Empty title="아직 발행된 Work Order가 없습니다" />
              )}
              <div className="path-start">
                <Role role="Analyst" />
                <span>재사용 지식 검토</span>
                <Role role="Master" />
                <span>감사·정리 제안</span>
              </div>
            </section>
          )}
          {tab === "broadcast" && (
            <section className="panel">
              <h2>Broadcast</h2>
              <p className="muted">
                결정·행동·관찰의 기록입니다. 비공개 추론은 포함하지 않습니다.
              </p>
              {run.data?.events?.length ? (
                <div className="timeline">
                  {run.data.events.map((e: Json) => (
                    <div className="event" key={e.seq}>
                      <span className="event-dot" />
                      <div>
                        <div className="row">
                          <Role role={e.role} />
                          <strong>{actions[e.kind] ?? e.kind}</strong>
                          <ID id={e.details?.tool} />
                        </div>
                        {e.details?.usage && (
                          <small>
                            {formatNumber(e.details.usage.input_tokens)} in /{" "}
                            {formatNumber(e.details.usage.output_tokens)} out
                          </small>
                        )}
                        {e.details?.tool && (
                          <p>{actions[e.details.tool] ?? e.details.tool}</p>
                        )}
                        <details>
                          <summary>세부 근거</summary>
                          <JsonView value={e.details} />
                        </details>
                      </div>
                    </div>
                  ))}
                </div>
              ) : (
                <Empty title="아직 실행 기록이 없습니다" />
              )}
              <details>
                <summary>작업 활동 기록 {ev.length}개</summary>
                <JsonView value={ev.slice(-30)} />
              </details>
            </section>
          )}
          {tab === "knowledge" && (
            <section className="panel">
              <h2>지식 제출·채택</h2>
              <p className="muted">
                업무 완료와 중앙 지식 반영은 독립적으로 기록됩니다.
              </p>
              <ErrorBox error={knowledge.error} />
              {knowledge.data?.submissions?.map((s: Json) => (
                <div className="knowledge-row" key={s.id}>
                  <div>
                    <h3>{s.title}</h3>
                    <Tags tags={s.tags} />
                    <p>{s.rationale}</p>
                  </div>
                  <div>
                    <Badge value={s.status} />
                    {s.item_id && (
                      <button
                        onClick={() => w.openKnowledge(s.item_id, s.version)}
                      >
                        채택 근거 v{s.version} ↗
                      </button>
                    )}
                  </div>
                </div>
              ))}
              {!knowledge.data?.submissions?.length && (
                <Empty title="제출된 재사용 지식이 없습니다" />
              )}
            </section>
          )}
          {tab === "outputs" && (
            <section className="panel">
              <h2>산출물</h2>
              <p className="muted">
                등록된 입력과 격리 환경의 결과 파일입니다.
              </p>
              <FileUpload task={id} contract={t.contract} disabled={active} />
              {t.artifacts.map((f: Json) => (
                <div className="file-row" key={f.id}>
                  <span className="file-icon">▤</span>
                  <div>
                    <strong>{f.name}</strong>
                    <small>
                      {f.media_type} · {formatNumber(f.bytes)} bytes · SHA256{" "}
                      {f.sha256.slice(0, 16)}
                    </small>
                    <ID id={f.id} />
                  </div>
                  <a
                    className="button"
                    href={`/v1/operator/artifacts/${f.id}/download?${query({ tenant_id: w.tenant })}`}
                    download
                  >
                    {f.work_order_id ? "산출물" : "입력"} 다운로드
                  </a>
                </div>
              ))}
              {!t.artifacts.length && <Empty title="등록된 파일이 없습니다" />}
            </section>
          )}
        </section>
        <aside className="detail-rail">
          <p className="eyebrow">MISSION STATUS</p>
          <h3>진행 현황</h3>
          <div className="status-large">
            <Badge value={t.status} />
          </div>
          <dl>
            <dt>Work Orders</dt>
            <dd>
              {t.work_orders.length} / {t.contract.max_work_orders || 2}
            </dd>
            <dt>산출물</dt>
            <dd>{t.artifacts.filter((f: Json) => f.work_order_id).length}</dd>
            <dt>지식 제출</dt>
            <dd>{knowledge.data?.submissions?.length ?? 0}</dd>
            <dt>실행 단계</dt>
            <dd>{run.data?.phase ?? "대기"}</dd>
          </dl>
          <hr />
          <h3>최근 활동</h3>
          {(run.data?.events ?? [])
            .slice(-6)
            .reverse()
            .map((e: Json) => (
              <div className="mini-event" key={e.seq}>
                <Role role={e.role} />
                <span>
                  {actions[e.details?.tool] ?? actions[e.kind] ?? e.kind}
                </span>
              </div>
            ))}
          <p className="small muted">
            실행 상태와 지식 반영 내역은 서버 기록을 기준으로 표시합니다.
          </p>
        </aside>
      </div>
      {runOpen && (
        <Modal title="Task 실행" onClose={() => setRunOpen(false)}>
          <RunForm task={id} onStarted={() => setRunOpen(false)} />
        </Modal>
      )}
      {edit && (
        <Modal title="업무 계약 수정" wide onClose={() => setEdit(false)}>
          <SubmitForm
            onSubmit={async () => {
              await api(`/v1/operator/tasks/${id}`, "PATCH", {
                tenant_id: w.tenant,
                contract,
                reason: "운영자 계약 개정",
              });
              setEdit(false);
              w.refresh();
            }}
          >
            <ContractFields value={contract} onChange={setContract} />
          </SubmitForm>
        </Modal>
      )}
      {clone && (
        <Modal title="새 Task로 복제" wide onClose={() => setClone(false)}>
          <TaskForm
            initialGoal={t.goal}
            initialContract={{ ...t.contract, input_artifacts: [] }}
            onCreated={(newId) => {
              setClone(false);
              w.openTask(newId);
            }}
          />
        </Modal>
      )}
    </>
  );
}
function FileUpload({
  task,
  contract,
  disabled,
}: {
  task: string;
  contract: Json;
  disabled: boolean;
}) {
  const w = useWorkspace();
  const [error, setError] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  return (
    <div className="upload">
      <label className={`button ${disabled || busy ? "disabled" : ""}`}>
        {busy ? "등록 중…" : "＋ 입력 파일 등록"}
        <input
          type="file"
          hidden
          disabled={disabled || busy}
          onChange={async (e) => {
            const file = e.target.files?.[0];
            if (!file) return;
            setBusy(true);
            setError(undefined);
            try {
              if (file.size > 1048576)
                throw new Error("파일은 1 MiB 이하여야 합니다.");
              const bytes = new Uint8Array(await file.arrayBuffer());
              let binary = "";
              for (const b of bytes) binary += String.fromCharCode(b);
              const r = await api("/v1/operator/artifacts", "POST", {
                tenant_id: w.tenant,
                task_id: task,
                artifact: {
                  request_id: key(),
                  name: file.name,
                  media_type: file.type || "application/octet-stream",
                  data_base64: btoa(binary),
                },
              });
              await api(`/v1/operator/tasks/${task}`, "PATCH", {
                tenant_id: w.tenant,
                contract: {
                  ...contract,
                  input_artifacts: [
                    ...(contract.input_artifacts ?? []),
                    r.artifact.id,
                  ],
                },
                reason: "업무 입력 파일 추가",
              });
              w.refresh();
              w.notify("입력 파일을 등록했습니다.");
            } catch (err) {
              setError(err);
            } finally {
              setBusy(false);
              e.target.value = "";
            }
          }}
        />
      </label>
      <small>1 MiB 이하 · 코드 실행 입력은 UTF-8 텍스트</small>
      <ErrorBox error={error} />
    </div>
  );
}
