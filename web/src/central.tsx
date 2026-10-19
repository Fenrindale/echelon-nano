import { useState } from "react";
import { api, domain, key, labels } from "./api";
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
export const blankDocument: Json = {
  category: "data",
  title: "",
  storage_mode: "reference",
  content: "",
  tags: [],
  sources: [],
  relations: [],
  truth_state: "unverified",
  outcome: "not_applicable",
  freshness: "current",
  valid_until: null,
  applicability: "",
  assessment: null,
  external_model_allowed: true,
};
const options = (values: string[]) =>
  values.map((v) => (
    <option key={v} value={v}>
      {labels[v] ?? v}
    </option>
  ));
export function KnowledgeForm({
  initial = blankDocument,
  target,
  baseVersion,
  onDone,
}: {
  initial?: Json;
  target?: string;
  baseVersion?: number;
  onDone: () => void;
}) {
  const w = useWorkspace();
  const [doc, setDoc] = useState<Json>({ ...initial });
  const [reason, setReason] = useState("");
  const [relation, setRelation] = useState("");
  const [kind, setKind] = useState("references");
  const items = useAsync(
    () =>
      domain(w.tenant, w.admin, "knowledge.read", { op: "list", limit: 100 }),
    [w.tenant, w.revision],
  );
  const set = (k: string, v: unknown) => setDoc({ ...doc, [k]: v });
  const negative = doc.truth_state === "refuted" || doc.outcome === "failed";
  return (
    <SubmitForm
      button="Handler 지식 제출"
      onSubmit={async () => {
        await domain(w.tenant, w.admin, "knowledge.submit", {
          op: "submit",
          request_id: key(),
          target_id: target ?? null,
          base_version: baseVersion ?? null,
          document: doc,
          rationale: reason,
        });
        w.refresh();
        w.notify("Analyst 검토 대기열에 제출했습니다.");
        onDone();
      }}
    >
      <div className="notice">
        중앙 지식에 바로 저장하지 않습니다. 제출 내용을 Analyst가 채택하면 새
        항목 또는 새 개정으로 보관합니다.
      </div>
      <Field label="제목">
        <input
          required
          maxLength={240}
          value={doc.title}
          onChange={(e) => set("title", e.target.value)}
        />
      </Field>
      <div className="form-grid">
        <Field
          label="범주"
          hint="Data: 원천 / Info: 맥락화 / Intel: 판단에 쓸 근거"
        >
          <select
            value={doc.category}
            onChange={(e) => set("category", e.target.value)}
          >
            {options(["data", "info", "intel"])}
          </select>
        </Field>
        <Field label="적재 방식">
          <select
            value={doc.storage_mode}
            onChange={(e) => set("storage_mode", e.target.value)}
          >
            <option value="reference">출처 참조</option>
            <option value="excerpt">핵심 발췌</option>
            <option value="artifact">산출물 참조</option>
            <option value="hybrid">참조 + 핵심 발췌</option>
          </select>
        </Field>
      </div>
      <Field
        label="내용"
        hint="큰 문서는 위치와 필요한 절만, 짧은 정보는 재조회 비용을 고려해 발췌하세요."
      >
        <textarea
          rows={6}
          value={doc.content}
          onChange={(e) => set("content", e.target.value)}
          maxLength={16000}
        />
      </Field>
      <Field label="출처 URL 또는 artifact://파일 ID · 한 줄에 하나">
        <textarea
          defaultValue={doc.sources.map((s: Json) => s.uri).join("\n")}
          onChange={(e) =>
            set(
              "sources",
              e.target.value
                .split("\n")
                .filter(Boolean)
                .map(
                  (uri) =>
                    doc.sources.find((s: Json) => s.uri === uri) ?? {
                      uri,
                      locator: "",
                      note: "",
                    },
                ),
            )
          }
          placeholder="https://docs.example.com/guide"
        />
      </Field>
      {doc.sources.map((s: Json, i: number) => (
        <div className="source-edit" key={i}>
          <Field label="출처의 절·위치">
            <input
              value={s.locator}
              onChange={(e) =>
                set(
                  "sources",
                  doc.sources.map((v: Json, j: number) =>
                    j === i ? { ...v, locator: e.target.value } : v,
                  ),
                )
              }
            />
          </Field>
          <Field label="출처 설명">
            <input
              value={s.note}
              onChange={(e) =>
                set(
                  "sources",
                  doc.sources.map((v: Json, j: number) =>
                    j === i ? { ...v, note: e.target.value } : v,
                  ),
                )
              }
            />
          </Field>
        </div>
      ))}
      <Field label="키워드 태그 · 쉼표로 구분">
        <input
          defaultValue={doc.tags.join(", ")}
          onChange={(e) =>
            set(
              "tags",
              e.target.value
                .split(",")
                .map((s) => s.trim())
                .filter(Boolean),
            )
          }
        />
      </Field>
      <Field label="적용 범위·조건">
        <textarea
          value={doc.applicability}
          onChange={(e) => set("applicability", e.target.value)}
          maxLength={2000}
        />
      </Field>
      <div className="form-grid three">
        <Field label="진위">
          <select
            value={doc.truth_state}
            onChange={(e) => set("truth_state", e.target.value)}
          >
            {options(["unverified", "supported", "refuted", "conflicted"])}
          </select>
        </Field>
        <Field label="실행 결과">
          <select
            value={doc.outcome}
            onChange={(e) => set("outcome", e.target.value)}
          >
            {options([
              "not_applicable",
              "success",
              "failed",
              "partial",
              "blocked",
            ])}
          </select>
        </Field>
        <Field label="최신성">
          <select
            value={doc.freshness}
            onChange={(e) => set("freshness", e.target.value)}
          >
            {options(["current", "review_needed", "expired", "superseded"])}
          </select>
        </Field>
      </div>
      <Field
        label="유효기한 (선택)"
        hint="기한이 지나면 고정 본문은 보존하고 최신성 경고와 프로토콜 캐시를 갱신합니다."
      >
        <input
          type="datetime-local"
          value={
            doc.valid_until
              ? new Date(
                  doc.valid_until * 1000 -
                    new Date(doc.valid_until * 1000).getTimezoneOffset() *
                      60000,
                )
                  .toISOString()
                  .slice(0, 16)
              : ""
          }
          onChange={(e) =>
            set(
              "valid_until",
              e.target.value
                ? Math.floor(new Date(e.target.value).getTime() / 1000)
                : null,
            )
          }
        />
      </Field>
      {negative && (
        <div className="negative-form">
          <h3>거짓·실패 보존 근거</h3>
          {[
            ["conditions", "발생 조건"],
            ["attempted_action", "시도한 행동"],
            ["observed", "관찰 결과"],
            ["reason", "거짓·실패 판단 이유"],
            ["avoid", "다시 피해야 할 행동"],
            ["retry_when", "재시도 가능한 조건"],
          ].map(([id, label]) => (
            <Field key={id} label={label}>
              <textarea
                required={id !== "attempted_action"}
                value={doc.assessment?.[id] ?? ""}
                onChange={(e) =>
                  set("assessment", {
                    conditions: "",
                    attempted_action: "",
                    observed: "",
                    reason: "",
                    avoid: "",
                    retry_when: "",
                    ...doc.assessment,
                    [id]: e.target.value,
                  })
                }
              />
            </Field>
          ))}
        </div>
      )}
      <Field label="다른 정보와의 관계">
        <div className="relation-add">
          <select value={kind} onChange={(e) => setKind(e.target.value)}>
            {options([
              "references",
              "derived_from",
              "supports",
              "contradicts",
              "depends_on",
              "supersedes",
            ])}
          </select>
          <select
            value={relation}
            onChange={(e) => setRelation(e.target.value)}
          >
            <option value="">정보 선택</option>
            {items.data?.items
              ?.filter((x: Json) => x.id !== target)
              .map((x: Json) => (
                <option key={x.id} value={x.id}>
                  {x.title} · v{x.version}
                </option>
              ))}
          </select>
          <button
            type="button"
            disabled={!relation}
            onClick={() => {
              const item = items.data.items.find(
                (x: Json) => x.id === relation,
              );
              set("relations", [
                ...doc.relations,
                { kind, target_id: relation, target_version: item.version },
              ]);
              setRelation("");
            }}
          >
            추가
          </button>
        </div>
      </Field>
      <div className="tags">
        {doc.relations.map((r: Json, i: number) => (
          <button
            type="button"
            key={i}
            onClick={() =>
              set(
                "relations",
                doc.relations.filter((_: Json, j: number) => i !== j),
              )
            }
          >
            {r.kind} →{" "}
            {items.data?.items?.find((x: Json) => x.id === r.target_id)
              ?.title ?? r.target_id.slice(0, 8)}{" "}
            v{r.target_version} ×
          </button>
        ))}
      </div>
      <label className="check">
        <input
          type="checkbox"
          checked={doc.external_model_allowed === false}
          onChange={(e) => set("external_model_allowed", !e.target.checked)}
        />
        외부 모델 전송 금지
      </label>
      <Field label="제출 이유·재사용 가치">
        <textarea
          required
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          maxLength={2000}
        />
      </Field>
    </SubmitForm>
  );
}
export function Central() {
  const w = useWorkspace();
  const [tab, setTab] = useState("items");
  const [search, setSearch] = useState("");
  const [category, setCategory] = useState("");
  const [tags, setTags] = useState("");
  const [negative, setNegative] = useState(false);
  const [create, setCreate] = useState(false);
  const [after, setAfter] = useState<string>();
  const [review, setReview] = useState<string>();
  const [status, setStatus] = useState("pending");
  const r = useAsync(
    () =>
      domain(
        w.tenant,
        w.admin,
        "knowledge.read",
        tab === "items"
          ? {
              op: "list",
              category: category || null,
              query: search || null,
              tags: tags
                .split(",")
                .map((t) => t.trim())
                .filter(Boolean),
              negative_history: negative,
              after_id: after ?? null,
              limit: 50,
            }
          : {
              op: "submissions",
              status: status || null,
              after_id: after ?? null,
              limit: 50,
            },
      ),
    [
      w.tenant,
      w.revision,
      tab,
      search,
      category,
      tags,
      negative,
      after,
      status,
    ],
  );
  return (
    <>
      <div className="page-title">
        <div>
          <p className="eyebrow">CENTRAL / KNOWLEDGE</p>
          <h1>판단의 근거를 쌓습니다.</h1>
          <p className="muted">
            원천과 맥락, 판단을 연결하고 실패까지 보존합니다.
          </p>
        </div>
        <button className="primary" onClick={() => setCreate(true)}>
          ＋ 지식 제출
        </button>
      </div>
      <div className="tabs">
        {[
          ["items", "중앙 지식"],
          ["submissions", "Analyst 채택 대기열"],
        ].map(([id, label]) => (
          <button
            className={tab === id ? "selected" : ""}
            key={id}
            onClick={() => {
              setTab(id);
              setAfter(undefined);
            }}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="toolbar">
        {tab === "items" ? (
          <>
            <input
              aria-label="지식 검색"
              value={search}
              placeholder="제목·내용 검색"
              onChange={(e) => {
                setSearch(e.target.value);
                setAfter(undefined);
              }}
            />
            <select
              aria-label="범주"
              value={category}
              onChange={(e) => {
                setCategory(e.target.value);
                setAfter(undefined);
              }}
            >
              <option value="">모든 범주</option>
              {options(["data", "info", "intel"])}
            </select>
            <input
              aria-label="태그 필터"
              value={tags}
              onChange={(e) => {
                setTags(e.target.value);
                setAfter(undefined);
              }}
              placeholder="태그, 쉼표 구분"
            />
            <label className="check">
              <input
                type="checkbox"
                checked={negative}
                onChange={(e) => {
                  setNegative(e.target.checked);
                  setAfter(undefined);
                }}
              />
              거짓·실패 이력
            </label>
          </>
        ) : (
          <select
            value={status}
            aria-label="채택 상태"
            onChange={(e) => {
              setStatus(e.target.value);
              setAfter(undefined);
            }}
          >
            <option value="">모든 상태</option>
            {options(["pending", "accepted", "rejected"])}
          </select>
        )}
      </div>
      <ErrorBox error={r.error} />
      <div className="panel knowledge-table">
        {tab === "items" ? (
          r.data?.items?.length ? (
            r.data.items.map((x: Json) => (
              <button
                className="knowledge-row"
                key={x.id}
                onClick={() => w.openKnowledge(x.id)}
              >
                <span className={`category ${x.category}`}>{x.category}</span>
                <div className="grow">
                  <h3>{x.title}</h3>
                  <Tags tags={x.tags} />
                  <div className="row small">
                    <Badge value={x.truth_state} />
                    <Badge value={x.freshness} />
                    {x.has_negative_history && (
                      <span className="negative">거짓·실패 이력 보존</span>
                    )}
                    {x.external_model_allowed === false && (
                      <span className="negative">외부 모델 금지</span>
                    )}
                  </div>
                </div>
                <span className="version">v{x.version} ↗</span>
              </button>
            ))
          ) : (
            <Empty title="조건에 맞는 지식이 없습니다" />
          )
        ) : r.data?.submissions?.length ? (
          r.data.submissions.map((s: Json) => (
            <button
              className="knowledge-row"
              key={s.id}
              onClick={() => setReview(s.id)}
            >
              <span className={`category ${s.category}`}>{s.category}</span>
              <div className="grow">
                <h3>{s.title}</h3>
                <p>{s.rationale}</p>
                <Tags tags={s.tags} />
              </div>
              <Badge value={s.status} />
              <span>검토 ↗</span>
            </button>
          ))
        ) : (
          <Empty title="검토할 지식 제출이 없습니다" />
        )}
      </div>
      <div className="pagination">
        <button disabled={!after} onClick={() => setAfter(undefined)}>
          처음
        </button>
        <button
          disabled={(r.data?.items ?? r.data?.submissions ?? []).length < 50}
          onClick={() => setAfter(r.data.next_after_id)}
        >
          다음
        </button>
      </div>
      {create && (
        <Modal title="재사용 지식 제출" wide onClose={() => setCreate(false)}>
          <KnowledgeForm onDone={() => setCreate(false)} />
        </Modal>
      )}
      {review && (
        <SubmissionReview id={review} onClose={() => setReview(undefined)} />
      )}
    </>
  );
}
function SubmissionReview({
  id,
  onClose,
}: {
  id: string;
  onClose: () => void;
}) {
  const w = useWorkspace();
  const r = useAsync(
    () =>
      domain(w.tenant, w.admin, "knowledge.read", {
        op: "submission",
        submission_id: id,
      }),
    [id, w.revision],
  );
  const [decision, setDecision] = useState("accept");
  const [reason, setReason] = useState("");
  const [category, setCategory] = useState("");
  const [tags, setTags] = useState("");
  const s = r.data?.submission;
  return (
    <Modal title="Analyst 지식 검토" wide onClose={onClose}>
      <ErrorBox error={r.error} />
      {s && (
        <>
          <div className="row">
            <Badge value={s.status} />
            <button
              className="quiet"
              onClick={() => {
                onClose();
                w.openTask(s.source_task_id);
              }}
            >
              제출 Task ↗
            </button>
            {s.base_version && <span>기준 v{s.base_version}</span>}
          </div>
          <DocumentView document={s.document} />
          <blockquote>{s.rationale}</blockquote>
          {s.status === "pending" ? (
            <SubmitForm
              button="검토 확정"
              onSubmit={async () => {
                await domain(w.tenant, w.admin, "knowledge.publish", {
                  submission_id: id,
                  decision,
                  reason,
                  category: category || null,
                  tags: tags
                    ? tags
                        .split(",")
                        .map((v) => v.trim())
                        .filter(Boolean)
                    : null,
                });
                w.refresh();
                onClose();
              }}
            >
              <div className="form-grid">
                <Field label="결정">
                  <select
                    value={decision}
                    onChange={(e) => setDecision(e.target.value)}
                  >
                    <option value="accept">채택</option>
                    <option value="reject">반려 · 제출 이력 보존</option>
                  </select>
                </Field>
                <Field label="범주 조정">
                  <select
                    value={category}
                    onChange={(e) => setCategory(e.target.value)}
                  >
                    <option value="">제출 범주 유지</option>
                    {options(["data", "info", "intel"])}
                  </select>
                </Field>
              </div>
              <Field label="태그 조정 · 비우면 제출 태그 유지">
                <input value={tags} onChange={(e) => setTags(e.target.value)} />
              </Field>
              <Field label="판단 이유">
                <textarea
                  required
                  value={reason}
                  onChange={(e) => setReason(e.target.value)}
                  maxLength={2000}
                />
              </Field>
            </SubmitForm>
          ) : (
            <JsonView value={s.decision} />
          )}
        </>
      )}
    </Modal>
  );
}
export function DocumentView({ document: d }: { document: Json }) {
  const w = useWorkspace();
  return (
    <article className="document">
      <div className="row">
        <span className={`category ${d.category}`}>{d.category}</span>
        <Badge value={d.truth_state} />
        <Badge value={d.outcome} />
        <Badge
          value={
            d.valid_until &&
            d.valid_until * 1000 <= Date.now() &&
            d.freshness !== "superseded"
              ? "expired"
              : d.freshness
          }
        />
        <span className="muted">{d.storage_mode}</span>
      </div>
      <h2>{d.title}</h2>
      <Tags tags={d.tags} />
      {d.valid_until && (
        <small className="muted">
          유효기한 {new Date(d.valid_until * 1000).toLocaleString("ko-KR")}
        </small>
      )}
      {d.external_model_allowed === false && (
        <div className="notice warning">외부 모델 전송 금지</div>
      )}
      <p className="prose">{d.content}</p>
      {d.applicability && (
        <div className="applicability">
          <strong>적용 범위</strong>
          <p>{d.applicability}</p>
        </div>
      )}
      {d.sources?.length > 0 && (
        <section>
          <h3>출처</h3>
          {d.sources.map((s: Json, i: number) => (
            <div className="source" key={i}>
              {/^https?:\/\//.test(s.uri) ? (
                <a href={s.uri} target="_blank" rel="noreferrer">
                  {s.uri} ↗
                </a>
              ) : (
                <code>{s.uri}</code>
              )}
              <p>
                {s.locator} {s.note}
              </p>
            </div>
          ))}
        </section>
      )}
      {d.relations?.length > 0 && (
        <section>
          <h3>참조·종속 관계</h3>
          {d.relations.map((r: Json, i: number) => (
            <button
              className="relation"
              key={i}
              onClick={() => w.openKnowledge(r.target_id, r.target_version)}
            >
              {r.kind} → <ID id={r.target_id} /> v{r.target_version} ↗
            </button>
          ))}
        </section>
      )}
      {d.assessment && (
        <section className="negative-form">
          <h3>거짓·실패 분석</h3>
          <dl>
            {[
              ["conditions", "조건"],
              ["attempted_action", "시도"],
              ["observed", "관찰"],
              ["reason", "이유"],
              ["avoid", "피해야 할 행동"],
              ["retry_when", "재시도 조건"],
            ].map(([key, label]) => (
              <div key={key}>
                <dt>{label}</dt>
                <dd>{d.assessment[key]}</dd>
              </div>
            ))}
          </dl>
        </section>
      )}
    </article>
  );
}
export function KnowledgeDetail({
  id,
  initialVersion,
  onClose,
}: {
  id: string;
  initialVersion?: number;
  onClose: () => void;
}) {
  const w = useWorkspace();
  const [version, setVersion] = useState(initialVersion);
  const [edit, setEdit] = useState(false);
  const [classify, setClassify] = useState(false);
  const [category, setCategory] = useState("data");
  const [tags, setTags] = useState("");
  const [reason, setReason] = useState("");
  const [error, setError] = useState<unknown>();
  const r = useAsync(
    () =>
      domain(w.tenant, w.admin, "knowledge.read", {
        op: "get",
        item_id: id,
        version: version ?? null,
      }),
    [id, version, w.revision],
  );
  const history = useAsync(
    () =>
      domain(w.tenant, w.admin, "knowledge.read", {
        op: "history",
        item_id: id,
        limit: 100,
      }),
    [id, w.revision],
  );
  const item = r.data?.item;
  const latest = history.data?.revisions?.[0];
  return (
    <Modal title="고정 지식·개정 이력" wide onClose={onClose}>
      <ErrorBox error={r.error || history.error || error} />
      {item && (
        <>
          <div className="detail-controls">
            <ID id={id} />
            <select
              aria-label="개정 버전"
              value={version ?? "current"}
              onChange={(e) =>
                setVersion(
                  e.target.value === "current" ? undefined : +e.target.value,
                )
              }
            >
              <option value="current">최신 버전</option>
              {history.data?.revisions?.map((h: Json) => (
                <option key={h.version} value={h.version}>
                  v{h.version} · {h.actor_role} · {h.reason}
                </option>
              ))}
            </select>
            <span>표시 v{item.version}</span>
          </div>
          {r.data.negative_history?.length > 0 && (
            <div className="notice warning">
              거짓·실패 이력{" "}
              {r.data.negative_history
                .map((n: Json) => `v${n.version}`)
                .join(", ")}{" "}
              보존됨
            </div>
          )}
          {r.data.dependency_warnings?.length > 0 && (
            <details className="notice warning">
              <summary>종속 근거에 새 버전 또는 주의 상태가 있습니다</summary>
              <JsonView value={r.data.dependency_warnings} />
            </details>
          )}
          {r.data.external_model_allowed === false &&
            item.document.external_model_allowed !== false && (
              <div className="notice warning">
                유효 전송 정책: 외부 모델 금지 · 이전 본문과 개정은 보존됩니다.
              </div>
            )}
          <DocumentView document={item.document} />
          <div className="revision-footer">
            <small>
              {item.actor_role} · {item.reason}
            </small>
            <div className="button-group">
              <button onClick={() => setEdit(true)}>새 개정 제출</button>
              <button
                disabled={item.version !== latest?.version}
                onClick={() => {
                  setCategory(item.document.category);
                  setTags(item.document.tags.join(", "));
                  setClassify(true);
                }}
              >
                Analyst 분류·태그 조정
              </button>
              {item.version !== latest?.version && (
                <button
                  onClick={async () => {
                    try {
                      await domain(w.tenant, w.admin, "knowledge.optimize", {
                        op: "propose",
                        request_id: key(),
                        kind: "revise",
                        reason: `v${item.version} 내용으로 복원 제안; 기존 이력 보존`,
                        payload: {
                          bases: [{ item_id: id, version: latest.version }],
                          replacements: [
                            { target_id: id, document: item.document },
                          ],
                        },
                      });
                      w.refresh();
                      w.notify(
                        "이전 내용을 복원하는 개정안을 Dreaming에 제출했습니다.",
                      );
                    } catch (e) {
                      setError(e);
                    }
                  }}
                >
                  이 버전으로 복원 제안
                </button>
              )}
            </div>
          </div>
          {edit && (
            <div className="inline-form">
              <h3>새 개정 · 기준 v{item.version}</h3>
              <KnowledgeForm
                initial={item.document}
                target={id}
                baseVersion={item.version}
                onDone={() => {
                  setEdit(false);
                  setVersion(undefined);
                }}
              />
            </div>
          )}
          {classify && (
            <div className="inline-form">
              <SubmitForm
                button="분류 개정 보관"
                onSubmit={async () => {
                  await domain(w.tenant, w.admin, "knowledge.classify", {
                    item_id: id,
                    expected_version: item.version,
                    category,
                    tags: tags
                      .split(",")
                      .map((s) => s.trim())
                      .filter(Boolean),
                    reason,
                  });
                  setClassify(false);
                  w.refresh();
                }}
              >
                <Field label="범주">
                  <select
                    value={category}
                    onChange={(e) => setCategory(e.target.value)}
                  >
                    {options(["data", "info", "intel"])}
                  </select>
                </Field>
                <Field label="태그">
                  <input
                    value={tags}
                    onChange={(e) => setTags(e.target.value)}
                  />
                </Field>
                <Field label="변경 이유">
                  <textarea
                    value={reason}
                    onChange={(e) => setReason(e.target.value)}
                    required
                  />
                </Field>
              </SubmitForm>
            </div>
          )}
        </>
      )}
    </Modal>
  );
}
