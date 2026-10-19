import { useEffect, useState } from "react";
import { api, domain, key, query, formatNumber, labels } from "./api";
import { useWorkspace } from "./context";
import {
  Badge,
  Empty,
  ErrorBox,
  Field,
  ID,
  JsonView,
  Modal,
  SubmitForm,
  Tags,
  useAsync,
  type Json,
} from "./components";
import { DocumentView, blankDocument } from "./central";
import { RunForm, TaskForm } from "./tasks";
import { applyAccent } from "./theme";
const split = (s: string) =>
  s
    .split("\n")
    .map((v) => v.trim())
    .filter(Boolean);
export function Protocols() {
  const w = useWorkspace();
  const [search, setSearch] = useState("");
  const [id, setId] = useState("");
  const [version, setVersion] = useState<number>();
  const [form, setForm] = useState(false);
  const [use, setUse] = useState(false);
  const [task, setTask] = useState("");
  const r = useAsync(
    () =>
      api(
        `/v1/operator/protocols?${query({ tenant_id: w.tenant, query: search })}`,
      ),
    [w.tenant, w.revision, search],
  );
  const detail = useAsync(
    () =>
      id
        ? api(
            `/v1/operator/protocols/${id}?${query({ tenant_id: w.tenant, version })}`,
          )
        : Promise.resolve(null),
    [id, version, w.revision],
  );
  const p = detail.data;
  return (
    <>
      <div className="page-title">
        <div>
          <p className="eyebrow">PROTOCOLS / REUSABLE OPERATIONS</p>
          <h1>반복을 작전으로.</h1>
          <p className="muted">
            검증한 절차와 고정 정보셋으로 다음 업무를 시작합니다.
          </p>
        </div>
        <button
          className="primary"
          onClick={() => {
            setId("");
            setForm(true);
          }}
        >
          ＋ 프로토콜 제안
        </button>
      </div>
      <div className="toolbar">
        <input
          aria-label="프로토콜 검색"
          placeholder="작전명·설명 검색"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>
      <ErrorBox error={r.error || detail.error} />
      <div className="protocol-grid">
        {r.data?.protocols?.map((x: Json) => (
          <button
            className={`protocol-card ${id === x.id ? "selected" : ""}`}
            key={x.id}
            onClick={() => {
              setId(x.id);
              setVersion(undefined);
            }}
          >
            <div className="row">
              <span className="protocol-symbol">◎</span>
              <Badge value={x.status} />
              <small>v{x.version}</small>
            </div>
            <h2>프로토콜:{x.codename}</h2>
            <p>{x.description}</p>
            <span className="text-link">정보셋 확인 ↗</span>
          </button>
        ))}
      </div>
      {!r.data?.protocols?.length && (
        <Empty title="등록된 프로토콜이 없습니다">
          Dreaming의 반복 업무 감지 또는 직접 제안으로 만들 수 있습니다.
        </Empty>
      )}
      {p && (
        <section className="panel protocol-detail">
          <div className="section-heading">
            <div>
              <p className="eyebrow">PROTOCOL:{p.document.codename}</p>
              <h2>{p.document.description}</h2>
            </div>
            <div className="button-group">
              <button onClick={() => setForm(true)}>개정 제안</button>
              <button
                className="primary"
                disabled={
                  p.current_status !== "active" ||
                  p.document.external_model_allowed === false
                }
                onClick={() => {
                  setTask("");
                  setUse(true);
                }}
              >
                이 프로토콜로 Task 시작
              </button>
            </div>
          </div>
          <div className="row">
            <Badge value={p.document.status} />
            <span>
              v{p.version} / 현재 v{p.current_version}
            </span>
            <button
              disabled={p.version <= 1}
              onClick={() => setVersion(p.version - 1)}
            >
              이전 버전
            </button>
            <button
              disabled={p.version >= p.current_version}
              onClick={() => setVersion(p.version + 1)}
            >
              다음 버전
            </button>
            <ID id={p.id} />
          </div>
          {p.review_required && (
            <div className="notice warning">
              정보셋에 새 개정 또는 거짓·실패·만료 주의 사항이 있습니다. 고정
              버전과 현재 상태를 함께 확인하세요.
              <JsonView value={p.warnings} />
            </div>
          )}
          {p.document.external_model_allowed === false && (
            <div className="notice warning">
              이 정보셋은 외부 모델 전송이 금지되어 실행할 수 없습니다.
            </div>
          )}
          <p>{p.document.applicability}</p>
          <div className="form-grid">
            <section>
              <h3>실행 절차</h3>
              <ol>
                {p.document.steps.map((s: string, i: number) => (
                  <li key={i}>{s}</li>
                ))}
              </ol>
              <h3>완료 조건</h3>
              <ul>
                {p.document.completion_conditions.map(
                  (s: string, i: number) => (
                    <li key={i}>{s}</li>
                  ),
                )}
              </ul>
            </section>
            <section>
              <h3>중단·재시도 조건</h3>
              <ul>
                {[
                  ...p.document.stop_conditions,
                  ...p.document.retry_conditions,
                ].map((s: string, i: number) => (
                  <li key={i}>{s}</li>
                ))}
              </ul>
              <h3>입력 변수</h3>
              {p.document.parameters.map((s: Json) => (
                <p key={s.name}>
                  <strong>{s.name}</strong>
                  {s.required ? " *" : ""} — {s.description}
                </p>
              ))}
            </section>
          </div>
          <h3>고정 정보셋</h3>
          <div className="reference-grid">
            {p.document.references.map((ref: Json) => (
              <Reference
                key={`${ref.item_id}:${ref.version}`}
                reference={ref}
              />
            ))}
          </div>
          <small className="muted">
            담당 {p.document.owner} · 캐시 지문 {p.cache_key.slice(0, 16)} ·
            근거 개정과 정책 변경 시 무효화
          </small>
        </section>
      )}
      {form && (
        <Modal title="프로토콜 제안·개정" wide onClose={() => setForm(false)}>
          <ProtocolForm
            protocol={id ? (p ?? undefined) : undefined}
            onDone={() => setForm(false)}
          />
        </Modal>
      )}
      {use && p && (
        <Modal
          title={task ? "프로토콜 실행" : "프로토콜 Task 만들기"}
          wide
          onClose={() => setUse(false)}
        >
          {task ? (
            <RunForm
              task={task}
              protocol={p}
              onStarted={() => {
                setUse(false);
                w.openTask(task);
              }}
            />
          ) : (
            <TaskForm
              initialGoal={p.document.description}
              initialContract={{
                completion_conditions: p.document.completion_conditions,
                allowed_tools: ["artifact.write"],
                constraints: p.document.stop_conditions,
              }}
              onCreated={setTask}
            />
          )}
        </Modal>
      )}
    </>
  );
}
function Reference({ reference: r }: { reference: Json }) {
  const w = useWorkspace();
  const d = useAsync(
    () =>
      domain(w.tenant, w.admin, "knowledge.read", {
        op: "get",
        item_id: r.item_id,
        version: r.version,
      }),
    [r.item_id, r.version],
  );
  const doc = d.data?.item?.document;
  return (
    <button
      className="reference-card"
      onClick={() => w.openKnowledge(r.item_id, r.version)}
    >
      <div className="row">
        <span className={`category ${doc?.category}`}>
          {doc?.category ?? "…"}
        </span>
        <span>
          v{r.version} · {r.required ? "필수" : "선택"}
        </span>
      </div>
      <strong>{doc?.title ?? r.item_id}</strong>
      <Tags tags={doc?.tags} />
      <ErrorBox error={d.error} />
    </button>
  );
}
function ProtocolForm({
  protocol,
  onDone,
}: {
  protocol?: Json;
  onDone: () => void;
}) {
  const w = useWorkspace();
  const d = protocol?.document;
  const [name, setName] = useState(d?.codename ?? "");
  const [description, setDescription] = useState(d?.description ?? "");
  const [applicability, setApplicability] = useState(d?.applicability ?? "");
  const [aliases, setAliases] = useState(d?.aliases?.join(", ") ?? "");
  const [steps, setSteps] = useState(d?.steps?.join("\n") ?? "");
  const [completion, setCompletion] = useState(
    d?.completion_conditions?.join("\n") ?? "",
  );
  const [stop, setStop] = useState(d?.stop_conditions?.join("\n") ?? "");
  const [retry, setRetry] = useState(d?.retry_conditions?.join("\n") ?? "");
  const [owner, setOwner] = useState(d?.owner ?? "운영자");
  const [status, setStatus] = useState(d?.status ?? "candidate");
  const [reason, setReason] = useState("");
  const [refs, setRefs] = useState<Json[]>(d?.references ?? []);
  const [params, setParams] = useState<Json[]>(d?.parameters ?? []);
  const inventory = useAsync(
    () =>
      domain(w.tenant, w.admin, "knowledge.read", { op: "list", limit: 100 }),
    [w.tenant, w.revision],
  );
  return (
    <SubmitForm
      button="Master 검토안 등록"
      onSubmit={async () => {
        await domain(w.tenant, w.admin, "knowledge.optimize", {
          op: "propose",
          request_id: key(),
          kind: "protocol",
          reason,
          payload: {
            protocol_id: protocol?.id ?? null,
            expected_version: protocol?.current_version ?? null,
            document: {
              codename: name,
              description,
              applicability,
              aliases: aliases
                .split(",")
                .map((s: string) => s.trim())
                .filter(Boolean),
              parameters: params,
              references: refs,
              steps: split(steps),
              completion_conditions: split(completion),
              stop_conditions: split(stop),
              retry_conditions: split(retry),
              owner,
              status,
              source_tasks: d?.source_tasks ?? [],
            },
          },
        });
        w.refresh();
        w.notify("Dreaming 검토 대기열에 프로토콜을 제안했습니다.");
        onDone();
      }}
    >
      <Field label="작전명">
        <input
          required
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="예: 알파"
          maxLength={80}
        />
      </Field>
      <Field label="프로토콜 설명">
        <textarea
          required
          value={description}
          onChange={(e) => setDescription(e.target.value)}
          maxLength={2000}
        />
      </Field>
      <Field label="적용 조건">
        <textarea
          value={applicability}
          onChange={(e) => setApplicability(e.target.value)}
        />
      </Field>
      <Field label="별칭 · 쉼표 구분">
        <input value={aliases} onChange={(e) => setAliases(e.target.value)} />
      </Field>
      <h3>정보셋</h3>
      <div className="check-list">
        {inventory.data?.items?.map((x: Json) => (
          <label className="check" key={x.id}>
            <input
              type="checkbox"
              checked={refs.some((r) => r.item_id === x.id)}
              onChange={(e) =>
                setRefs(
                  e.target.checked
                    ? [
                        ...refs,
                        { item_id: x.id, version: x.version, required: true },
                      ]
                    : refs.filter((r) => r.item_id !== x.id),
                )
              }
            />
            <span>
              {x.title} · v
              {refs.find((r) => r.item_id === x.id)?.version ?? x.version}{" "}
              <Tags tags={x.tags} />
            </span>
          </label>
        ))}
      </div>
      <Field label="실행 절차 · 한 줄에 하나">
        <textarea
          required
          value={steps}
          onChange={(e) => setSteps(e.target.value)}
          rows={5}
        />
      </Field>
      <Field label="완료 조건 · 한 줄에 하나">
        <textarea
          required
          value={completion}
          onChange={(e) => setCompletion(e.target.value)}
        />
      </Field>
      <div className="form-grid">
        <Field label="중단 조건">
          <textarea value={stop} onChange={(e) => setStop(e.target.value)} />
        </Field>
        <Field label="재시도 조건">
          <textarea value={retry} onChange={(e) => setRetry(e.target.value)} />
        </Field>
      </div>
      <h3>입력 변수</h3>
      {params.map((p, i) => (
        <div className="parameter-row" key={i}>
          <input
            aria-label="변수명"
            placeholder="변수명"
            required
            value={p.name}
            onChange={(e) =>
              setParams(
                params.map((x, j) =>
                  j === i ? { ...x, name: e.target.value } : x,
                ),
              )
            }
          />
          <input
            aria-label="변수 설명"
            placeholder="설명"
            required
            value={p.description}
            onChange={(e) =>
              setParams(
                params.map((x, j) =>
                  j === i ? { ...x, description: e.target.value } : x,
                ),
              )
            }
          />
          <label className="check">
            <input
              type="checkbox"
              checked={p.required}
              onChange={(e) =>
                setParams(
                  params.map((x, j) =>
                    j === i ? { ...x, required: e.target.checked } : x,
                  ),
                )
              }
            />
            필수
          </label>
          <button
            type="button"
            onClick={() => setParams(params.filter((_, j) => j !== i))}
          >
            ×
          </button>
        </div>
      ))}
      <button
        type="button"
        onClick={() =>
          setParams([...params, { name: "", description: "", required: true }])
        }
      >
        ＋ 변수
      </button>
      <div className="form-grid">
        <Field label="담당">
          <input
            required
            value={owner}
            onChange={(e) => setOwner(e.target.value)}
          />
        </Field>
        <Field label="제안 상태">
          <select value={status} onChange={(e) => setStatus(e.target.value)}>
            {["candidate", "testing", "active", "suspended", "retired"].map(
              (v) => (
                <option key={v} value={v}>
                  {labels[v]}
                </option>
              ),
            )}
          </select>
        </Field>
      </div>
      <Field label="제안·변경 이유">
        <textarea
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          required
        />
      </Field>
    </SubmitForm>
  );
}
export function Dreaming() {
  const w = useWorkspace();
  const [run, setRun] = useState(false);
  const [selected, setSelected] = useState<Json>();
  const [manual, setManual] = useState(false);
  const [error, setError] = useState<unknown>();
  const r = useAsync(
    () => api(`/v1/operator/maintenance?${query({ tenant_id: w.tenant })}`),
    [w.tenant, w.revision],
  );
  const inspect = useAsync(
    () => domain(w.tenant, w.admin, "knowledge.optimize", { op: "inspect" }),
    [w.tenant, w.revision],
  );
  const reviews = useAsync(
    () => api("/v1/operator/reviews?" + query({ tenant_id: w.tenant })),
    [w.tenant, w.revision],
  );
  return (
    <>
      <div className="page-title">
        <div>
          <p className="eyebrow">MASTER / DREAMING</p>
          <h1>경험을 정리하는 시간.</h1>
          <p className="muted">
            활동을 감사하고, 반복을 찾아 지식과 절차를 개선합니다.
          </p>
        </div>
        <div className="button-group">
          <button onClick={() => setManual(true)}>지식 정리 제안</button>
          <button className="primary" onClick={() => setRun(true)}>
            ◇ Master 감사 실행
          </button>
        </div>
      </div>
      <ErrorBox error={r.error || inspect.error || error} />
      <div className="metrics-strip">
        <div>
          <strong>
            {r.data?.proposals?.filter((p: Json) => p.status === "pending")
              .length ?? 0}
          </strong>
          <span>검토 대기</span>
        </div>
        <div>
          <strong>{inspect.data?.repetitions?.length ?? 0}</strong>
          <span>반복 업무 패턴</span>
        </div>
        <div>
          <strong>
            {inspect.data?.denials?.reduce(
              (n: number, d: Json) => n + d.count,
              0,
            ) ?? 0}
          </strong>
          <span>감사 대상 권한 거부</span>
        </div>
        <button
          onClick={async () => {
            try {
              const v = await domain(w.tenant, w.admin, "knowledge.optimize", {
                op: "deduplicate",
              });
              w.notify(
                `중복 연결 정리를 완료했습니다. ${v.linked?.length ?? 0}개 연결`,
              );
              w.refresh();
            } catch (e) {
              setError(e);
            }
          }}
        >
          동일 내용 중복 연결 정리
        </button>
      </div>
      {inspect.data?.repetitions?.length > 0 && (
        <section className="panel">
          <h2>반복 감지</h2>
          <p className="muted">
            동일 목적·실행 구조로 완료된 세 개 이상의 서로 다른 Task를 후보로
            표시합니다.
          </p>
          {inspect.data.repetitions.map((p: Json, i: number) => (
            <div className="repetition" key={i}>
              <strong>{p.purpose}</strong>
              <span>
                {p.completed_tasks}개 완료 Task · {p.output_format}
              </span>
              <div>
                {p.task_ids.map((id: string) => (
                  <button
                    className="quiet"
                    key={id}
                    onClick={() => w.openTask(id)}
                  >
                    <ID id={id} /> ↗
                  </button>
                ))}
              </div>
            </div>
          ))}
        </section>
      )}
      <section className="panel">
        <h2>정리·프로토콜 검토안</h2>
        <p className="muted">
          내용을 바꾸는 병합·분리·개정·프로토콜 활성화는 검토 후 적용합니다.
          과거 근거와 실패 이력은 유지됩니다.
        </p>
        {r.data?.proposals?.map((p: Json) => (
          <button
            className="knowledge-row"
            key={p.id}
            onClick={() => setSelected(p)}
          >
            <span className="proposal-icon">◇</span>
            <div className="grow">
              <h3>
                {p.kind === "protocol"
                  ? `프로토콜:${p.payload.document?.codename}`
                  : (
                      {
                        revise: "지식 개정",
                        merge: "지식 병합",
                        split: "지식 분리",
                        verification: "검증 요청",
                      } as Json
                    )[p.kind]}
              </h3>
              <p>{p.reason}</p>
            </div>
            <Badge value={p.status} />
            <span>검토 ↗</span>
          </button>
        ))}
        {!r.data?.proposals?.length && (
          <Empty title="아직 정리 제안이 없습니다" />
        )}
      </section>
      <section className="panel">
        <h2>별도 검증 요청</h2>
        <p className="muted">
          검증 작업은 운영자가 Task로 전환하고 허용 범위를 정한 뒤 Handler에게
          맡깁니다.
        </p>
        {reviews.data?.reviews?.map((v: Json) => (
          <div className="knowledge-row" key={v.id}>
            <div className="grow">
              <p>{v.reason}</p>
              {v.item_id && (
                <button
                  className="quiet"
                  onClick={() => w.openKnowledge(v.item_id)}
                >
                  대상 근거 ↗
                </button>
              )}
            </div>
            {v.task_id ? (
              <>
                <Badge value={v.status} />
                <button onClick={() => w.openTask(v.task_id)}>
                  검증 Task ↗
                </button>
              </>
            ) : (
              <button
                onClick={async () => {
                  try {
                    const t = await api(
                      `/v1/operator/reviews/${v.id}/task`,
                      "POST",
                      { tenant_id: w.tenant },
                    );
                    w.refresh();
                    w.openTask(t.task_id);
                  } catch (e) {
                    setError(e);
                  }
                }}
              >
                검증 Task 만들기
              </button>
            )}
          </div>
        ))}
        {!reviews.data?.reviews?.length && (
          <small className="muted">대기 중인 검증 요청이 없습니다.</small>
        )}
      </section>
      {run && (
        <Modal title="Master 감사" onClose={() => setRun(false)}>
          <p className="muted">
            감사는 중앙 지식과 활동 기록을 읽고 검토안을 만듭니다. Master는 업무
            도구를 실행하거나 Agent를 지시할 수 없습니다.
          </p>
          <RunForm
            task={w.admin}
            mode="dream"
            onStarted={() => {
              setRun(false);
              w.openTask(w.admin);
            }}
          />
        </Modal>
      )}
      {selected && (
        <ProposalReview
          proposal={selected}
          onClose={() => setSelected(undefined)}
        />
      )}{" "}
      {manual && (
        <Modal title="의미 변경 검토안" wide onClose={() => setManual(false)}>
          <MaintenanceForm onDone={() => setManual(false)} />
        </Modal>
      )}
    </>
  );
}
function MaintenanceForm({ onDone }: { onDone: () => void }) {
  const w = useWorkspace();
  const [kind, setKind] = useState("revise");
  const [selected, setSelected] = useState<string[]>([]);
  const [reason, setReason] = useState("");
  const [payload, setPayload] = useState("");
  const r = useAsync(
    () =>
      domain(w.tenant, w.admin, "knowledge.read", { op: "list", limit: 100 }),
    [w.tenant, w.revision],
  );
  return (
    <SubmitForm
      button="검토안 제출"
      onSubmit={async () => {
        const replacements = JSON.parse(payload);
        const bases = selected.map((id) => ({
          item_id: id,
          version: r.data.items.find((x: Json) => x.id === id).version,
        }));
        await domain(w.tenant, w.admin, "knowledge.optimize", {
          op: "propose",
          request_id: key(),
          kind,
          reason,
          payload: { bases, replacements },
        });
        w.refresh();
        onDone();
      }}
    >
      <Field label="정리 유형">
        <select value={kind} onChange={(e) => setKind(e.target.value)}>
          <option value="revise">개정 · 1 → 1</option>
          <option value="merge">병합 · 여러 정보 → 새 정보</option>
          <option value="split">분리 · 1 → 여러 정보</option>
        </select>
      </Field>
      <h3>기준 정보·버전</h3>
      <div className="check-list">
        {r.data?.items?.map((x: Json) => (
          <label key={x.id} className="check">
            <input
              type="checkbox"
              checked={selected.includes(x.id)}
              onChange={(e) =>
                setSelected(
                  e.target.checked
                    ? [...selected, x.id]
                    : selected.filter((id) => id !== x.id),
                )
              }
            />
            {x.title} · v{x.version}
          </label>
        ))}
      </div>
      <Field label="변경 이유">
        <textarea
          required
          value={reason}
          onChange={(e) => setReason(e.target.value)}
        />
      </Field>
      <Field
        label="새 정보 배열"
        hint='각 항목은 {"target_id": null, "document": {...}} 입니다. 개정은 target_id에 기준 정보 ID를 지정하세요.'
      >
        <textarea
          className="code-input"
          rows={16}
          value={payload}
          onChange={(e) => setPayload(e.target.value)}
          placeholder={JSON.stringify(
            [{ target_id: null, document: blankDocument }],
            null,
            2,
          )}
          required
        />
      </Field>
      <small className="muted">
        서버가 관계와 기준 버전, 범주·태그·실패 근거를 검증합니다.
      </small>
    </SubmitForm>
  );
}
function ProposalReview({
  proposal: p,
  onClose,
}: {
  proposal: Json;
  onClose: () => void;
}) {
  const w = useWorkspace();
  const [accept, setAccept] = useState(true);
  const [activate, setActivate] = useState(false);
  const [reason, setReason] = useState("");
  return (
    <Modal title="Master 검토안" wide onClose={onClose}>
      <div className="row">
        <Badge value={p.status} />
        <span>{p.kind}</span>
        <ID id={p.id} />
      </div>
      <blockquote>{p.reason}</blockquote>
      {p.kind === "protocol" ? (
        <>
          <h2>프로토콜:{p.payload.document.codename}</h2>
          <p>{p.payload.document.description}</p>
          <JsonView value={p.payload} />
        </>
      ) : p.payload.replacements ? (
        <>
          <h3>변경 전 · 고정 기준</h3>
          <div className="reference-grid">
            {p.payload.bases?.map((b: Json) => (
              <Reference key={b.item_id} reference={b} />
            ))}
          </div>
          <h3>변경 후</h3>
          {p.payload.replacements.map((r: Json, i: number) => (
            <div className="proposal-after" key={i}>
              <span>
                {r.target_id ? "기존 정보의 새 개정" : "새 정보 생성"}
              </span>
              <DocumentView document={r.document} />
            </div>
          ))}
        </>
      ) : (
        <JsonView value={p.payload} />
      )}{" "}
      {p.status === "pending" ? (
        <SubmitForm
          button="검토 결정"
          onSubmit={async () => {
            await api("/v1/operator/maintenance/decide", "POST", {
              tenant_id: w.tenant,
              proposal_id: p.id,
              accept,
              reason,
              activate_protocol: activate,
            });
            w.refresh();
            onClose();
          }}
        >
          <Field label="결정">
            <select
              value={accept ? "accept" : "reject"}
              onChange={(e) => setAccept(e.target.value === "accept")}
            >
              <option value="accept">적용 · 새 버전과 근거 보존</option>
              <option value="reject">반려 · 검토안 보존</option>
            </select>
          </Field>
          {p.kind === "protocol" && accept && p.payload.document.status === "active" && (
            <p className="muted">제안 상태가 활성입니다. 적용하면 해당 버전이 활성화됩니다.</p>
          )}
          {p.kind === "protocol" && accept && p.payload.document.status !== "active" && (
            <label className="check">
              <input
                type="checkbox"
                checked={activate}
                onChange={(e) => setActivate(e.target.checked)}
              />
              검토한 프로토콜을 활성화
            </label>
          )}
          <Field label="결정 이유">
            <textarea
              required
              value={reason}
              onChange={(e) => setReason(e.target.value)}
            />
          </Field>
        </SubmitForm>
      ) : (
        <>
          <h3>적용 결과·이전 버전</h3>
          <JsonView value={p.decision} />
          <p className="muted">
            지식 상세의 개정 이력에서 이전 내용을 새 개정으로 복원 제안할 수
            있습니다.
          </p>
        </>
      )}
    </Modal>
  );
}
export function Vault() {
  const w = useWorkspace();
  const [edit, setEdit] = useState<Json | null | undefined>();
  const [error, setError] = useState<unknown>();
  const r = useAsync(
    () => api(`/v1/operator/secrets?${query({ tenant_id: w.tenant })}`),
    [w.tenant, w.revision],
  );
  return (
    <>
      <div className="page-title">
        <div>
          <p className="eyebrow">VAULT / SECRET REFERENCES</p>
          <h1>기밀은 참조로만.</h1>
          <p className="muted">
            Task에는 사용 범위를, 모델에는 불투명한 참조만 전달합니다.
          </p>
        </div>
        <button className="primary" onClick={() => setEdit(null)}>
          ＋ 기밀 참조 등록
        </button>
      </div>
      <ErrorBox error={r.error || error} />
      <div className="panel">
        {r.data?.secrets?.map((s: Json) => (
          <div className="secret-row" key={s.id}>
            <span className="file-icon">⌁</span>
            <div className="grow">
              <h3>{s.name}</h3>
              <p>{s.purpose}</p>
              <ID id={s.id} />
              <small>
                {" "}
                v{s.version} · {s.enabled ? "사용 가능" : "사용 중지"}
              </small>
              <details>
                <summary>허용 주소 {s.allowed_urls.length}개</summary>
                {s.allowed_urls.map((u: string) => (
                  <p key={u}>
                    <code>{u}</code>
                  </p>
                ))}
              </details>
            </div>
            <button onClick={() => setEdit(s)}>회전·범위 수정</button>
            <button
              disabled={!s.enabled}
              onClick={async () => {
                try {
                  await api(`/v1/operator/secrets/${s.id}/disable`, "POST", {
                    tenant_id: w.tenant,
                  });
                  w.refresh();
                } catch (e) {
                  setError(e);
                }
              }}
            >
              사용 중지
            </button>
          </div>
        ))}
        {!r.data?.secrets?.length && (
          <Empty title="등록한 기밀 참조가 없습니다" />
        )}
      </div>
      {edit !== undefined && (
        <Modal
          title={edit ? "기밀 회전·범위 수정" : "새 기밀 참조"}
          onClose={() => setEdit(undefined)}
        >
          <SecretForm secret={edit} onDone={() => setEdit(undefined)} />
        </Modal>
      )}
    </>
  );
}
function SecretForm({
  secret: s,
  onDone,
}: {
  secret: Json | null;
  onDone: () => void;
}) {
  const w = useWorkspace();
  const [name, setName] = useState(s?.name ?? "");
  const [purpose, setPurpose] = useState(s?.purpose ?? "");
  const [urls, setUrls] = useState(s?.allowed_urls.join("\n") ?? "");
  const [header, setHeader] = useState("Authorization");
  const [value, setValue] = useState("");
  return (
    <SubmitForm
      button="Vault에 저장"
      onSubmit={async () => {
        try {
          await api("/v1/operator/secrets", "POST", {
            tenant_id: w.tenant,
            id: s?.id ?? null,
            expected_version: s?.version ?? null,
            name,
            purpose,
            allowed_urls: split(urls),
            header,
            value,
          });
          w.refresh();
          onDone();
        } finally {
          setValue("");
        }
      }}
    >
      <Field label="이름">
        <input
          required
          value={name}
          onChange={(e) => setName(e.target.value)}
          maxLength={160}
        />
      </Field>
      <Field label="용도">
        <input
          required
          value={purpose}
          onChange={(e) => setPurpose(e.target.value)}
          maxLength={1000}
        />
      </Field>
      <Field label="허용된 정확한 URL · 한 줄에 하나">
        <textarea
          required
          value={urls}
          onChange={(e) => setUrls(e.target.value)}
        />
      </Field>
      <Field label="인증 헤더">
        <select value={header} onChange={(e) => setHeader(e.target.value)}>
          <option>Authorization</option>
          <option>X-API-Key</option>
        </select>
      </Field>
      <Field
        label="새 기밀 값"
        hint="Authorization은 필요한 경우 Bearer 접두사까지 입력하세요. 기존 값은 조회하지 않습니다."
      >
        <input
          type="password"
          autoComplete="new-password"
          required
          value={value}
          onChange={(e) => setValue(e.target.value)}
        />
      </Field>
    </SubmitForm>
  );
}
export function Settings() {
  const w = useWorkspace();
  const [accent, setAccent] = useState("#78cfbf");
  const [auth, setAuth] = useState<Json>();
  const [keyForm, setKeyForm] = useState(false);
  const [error, setError] = useState<unknown>();
  const settings = useAsync(
    () => api(`/v1/operator/settings?${query({ tenant_id: w.tenant })}`),
    [w.tenant],
  );
  const health = useAsync(() => api("/readyz"), [w.tenant, w.revision]);
  const metrics = useAsync(
    () => api(`/v1/operator/metrics?${query({ tenant_id: w.tenant })}`),
    [w.tenant, w.revision],
  );
  useEffect(() => {
    if (settings.data) setAccent(settings.data.accent);
  }, [settings.data]);
  return (
    <>
      <div className="page-title">
        <div>
          <p className="eyebrow">SETTINGS / CONNECTIONS</p>
          <h1>실행 환경을 관리합니다.</h1>
          <p className="muted">계정, 사용 모델과 관찰된 사용량을 확인하세요.</p>
        </div>
        <Badge value={health.data?.worker ? "active" : "blocked"} />
      </div>
      <ErrorBox error={error || health.error} />
      <div className="settings-grid">
        <section className="panel">
          <h2>LLM 계정 연결</h2>
          <p className="muted">
            Sign in with ChatGPT 공식 인증을 사용합니다. API 키 연결은 별도로
            선택합니다.
          </p>
          {w.providers.map((p) => (
            <div className="provider-row" key={p.id}>
              <div>
                <strong>{p.name ?? "ChatGPT"}</strong>
                <small>
                  {p.kind ?? "chatgpt_oauth"} · {p.id.slice(0, 12)}
                </small>
              </div>
              <Badge
                value={
                  p.enabled === false ? "suspended" : (p.status ?? "active")
                }
              />
              {p.kind === "openai_api_key" ? (
                <button
                  onClick={async () => {
                    try {
                      await api(
                        `/v1/operator/providers/${p.id.slice(4)}/disable`,
                        "POST",
                        { tenant_id: w.tenant },
                      );
                      w.refreshProviders();
                    } catch (e) {
                      setError(e);
                    }
                  }}
                >
                  사용 중지
                </button>
              ) : (
                <button
                  onClick={async () => {
                    try {
                      await api(
                        `/v1/auth/connections/${p.id}/refresh`,
                        "POST",
                        { expected_generation: p.generation },
                      );
                      w.refreshProviders();
                      w.notify("계정 토큰을 갱신했습니다.");
                    } catch (e) {
                      setError(e);
                    }
                  }}
                >
                  토큰 갱신
                </button>
              )}
            </div>
          ))}
          <div className="button-group">
            <button
              className="primary"
              onClick={async () => {
                try {
                  setAuth(await api("/v1/auth/start", "POST", {}));
                } catch (e) {
                  setError(e);
                }
              }}
            >
              ChatGPT 로그인
            </button>
            <button onClick={() => setKeyForm(true)}>API 키 연결</button>
          </div>
          {auth && (
            <div className="notice">
              <p>
                아래 로그인 창에서 계정 선택과 플랜 사용 동의를 완료한 뒤 연결을
                확인하세요.
              </p>
              <a
                className="button primary"
                target="_blank"
                rel="noreferrer"
                href={auth.authorize_url}
              >
                ChatGPT 로그인 창 열기 ↗
              </a>
              <button
                onClick={async () => {
                  try {
                    const a = await api(`/v1/auth/attempts/${auth.attempt_id}`);
                    if (a.status === "connected") {
                      setAuth(undefined);
                      w.refreshProviders();
                      w.notify("ChatGPT 연결을 확인했습니다.");
                    } else w.notify(`로그인 상태: ${a.status}`);
                  } catch (e) {
                    setError(e);
                  }
                }}
              >
                연결 확인
              </button>
            </div>
          )}
        </section>
        <section className="panel">
          <h2>기본 모델·포인트 색상</h2>
          <SubmitForm
            button="설정 저장"
            onSubmit={async () => {
              await api("/v1/operator/settings", "PUT", {
                tenant_id: w.tenant,
                accent,
                preferences: { connection_id: w.connection, model: w.model },
              });
              applyAccent(accent);
              w.notify("설정을 저장했습니다.");
            }}
          >
            <Field label="계정">
              <select
                value={w.connection}
                onChange={(e) => w.setConnection(e.target.value)}
              >
                <option value="">선택</option>
                {w.providers
                  .filter((p) => p.enabled !== false)
                  .map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name ?? p.id}
                    </option>
                  ))}
              </select>
            </Field>
            <Field label="모델">
              <select
                value={w.model}
                onChange={(e) => w.setModel(e.target.value)}
              >
                <option value="">선택</option>
                {w.models.map((m) => (
                  <option value={m.slug} key={m.slug}>
                    {m.display_name ?? m.slug}
                  </option>
                ))}
              </select>
            </Field>
            <Field
              label="포인트 색상"
              hint="레이아웃·배경·상태 색상은 고정됩니다. 텍스트 대비가 부족하면 밝기를 보정합니다."
            >
              <div className="color-picker">
                <input
                  type="color"
                  aria-label="포인트 색상"
                  value={accent}
                  onChange={(e) => {
                    setAccent(e.target.value);
                    applyAccent(e.target.value);
                  }}
                />
                <code>{accent}</code>
                <span className="accent-preview">ECHELON / ACTIVE</span>
              </div>
            </Field>
          </SubmitForm>
        </section>
        <section className="panel span-2">
          <h2>관찰된 모델 사용량</h2>
          <p className="muted">
            이 업무실의 완료 응답에 포함된 토큰 합계입니다. 계정의 잔여
            한도·요금은 추정하지 않습니다.
          </p>
          <table>
            <thead>
              <tr>
                <th>역할</th>
                <th>호출</th>
                <th>입력 토큰</th>
                <th>출력 토큰</th>
              </tr>
            </thead>
            <tbody>
              {metrics.data?.roles?.map((r: Json) => (
                <tr key={r.role}>
                  <td>{r.role}</td>
                  <td>{formatNumber(r.calls)}</td>
                  <td>{formatNumber(r.input_tokens)}</td>
                  <td>{formatNumber(r.output_tokens)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
        <section className="panel span-2">
          <h2>로컬 환경</h2>
          <div className="row">
            <span className="live-dot live" />
            PostgreSQL{" "}
            <Badge value={health.data?.database ? "active" : "blocked"} /> ·
            Vault <Badge value={health.data?.vault ? "active" : "blocked"} /> ·
            Worker <Badge value={health.data?.worker ? "active" : "blocked"} />
          </div>
          <p className="muted">
            로컬 운영 화면은 암호나 인증 코드 없이 열립니다. 모델 사용을 위한
            ChatGPT 계정 연결은 별도로 관리합니다.
          </p>
          <details>
            <summary>서비스 상태</summary>
            <JsonView value={health.data} />
          </details>
        </section>
      </div>
      {keyForm && (
        <Modal title="명시적 API 키 계정" onClose={() => setKeyForm(false)}>
          <ApiKeyForm onDone={() => setKeyForm(false)} />
        </Modal>
      )}
    </>
  );
}
function ApiKeyForm({ onDone }: { onDone: () => void }) {
  const w = useWorkspace();
  const [name, setName] = useState("OpenAI API");
  const [value, setValue] = useState("");
  return (
    <SubmitForm
      button="API 계정 등록"
      onSubmit={async () => {
        try {
          await api("/v1/operator/providers", "POST", {
            tenant_id: w.tenant,
            name,
            api_key: value,
          });
          w.refreshProviders();
          onDone();
        } finally {
          setValue("");
        }
      }}
    >
      <p className="muted">
        Vault에 저장되며, 모델 목록과 실행에서 이 계정을 직접 선택해야
        사용됩니다.
      </p>
      <Field label="연결 이름">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          required
        />
      </Field>
      <Field label="API 키">
        <input
          type="password"
          autoComplete="new-password"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          required
        />
      </Field>
    </SubmitForm>
  );
}
