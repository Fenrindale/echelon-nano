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
            업무 목표만 알려 주세요. Handler가 설정부터 실행까지 맡습니다.
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
  const [requestId, setRequestId] = useState(key);
  return (
    <SubmitForm button="Handler에게 맡기기" onSubmit={async () => {
      if (initialContract.model_access === "blocked") {
        throw new Error("외부 모델 전송이 금지된 원본 업무는 자동 실행으로 복제할 수 없습니다.");
      }
      const r = await api("/v1/operator/tasks/launch", "POST", {
        tenant_id: w.tenant,
        request_id: requestId,
        goal: goal.trim(),
      });
      w.refresh();
      onCreated(r.task_id);
    }}>
      <Field label="업무 목표">
        <textarea autoFocus required value={goal} maxLength={4000}
          onChange={(e) => { setGoal(e.target.value); setRequestId(key()); }}
          placeholder="완료하고 싶은 업무를 자유롭게 설명해 주세요." />
      </Field>
      <p className="muted">
        Handler가 목표에 맞춰 완료 조건, 산출물, 모델, 예산, 도구와 프로토콜을 정하고 바로 실행합니다.
      </p>
    </SubmitForm>
  );
}
export function RunForm({ task, onStarted, mode = "work" }: {
  task: string;
  onStarted: () => void;
  mode?: string;
}) {
  const w = useWorkspace();
  const [requestId] = useState(key);
  return (
    <SubmitForm button={mode === "dream" ? "Master 감사 시작" : "Handler에게 맡기기"}
      onSubmit={async () => {
        await api(`/v1/operator/tasks/${task}/launch`, "POST", {
          tenant_id: w.tenant, request_id: requestId, mode,
        });
        w.refresh();
        onStarted();
      }}>
      <p className="muted">저장된 업무 목표와 현재 계약으로 실행합니다.</p>
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
  const [note, setNote] = useState("");
  const [error, setError] = useState<unknown>();
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
                  onClick={async () => {try{await api(`/v1/operator/tasks/${id}/launch`,"POST",{tenant_id:w.tenant,request_id:key()});w.refresh();}catch(e){setError(e);}}}
                >
                  Handler 실행
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
                  <h2>{run.data?.handler_settings ? "Handler가 정한 업무 계약" : "업무 계약"}</h2>

                </div>
                <p className="prose muted">{t.goal}</p>
                {run.data?.handler_settings && <p className="muted">Handler 설정 · {run.data.handler_settings.model} · 최대 {run.data.handler_settings.max_calls}회 / {formatNumber(run.data.handler_settings.max_total_tokens)} 토큰 · {run.data.contract_finalized ? "계약 확정" : "근거 확인 중"}</p>}
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
