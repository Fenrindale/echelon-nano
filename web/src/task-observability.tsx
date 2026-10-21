import { useState, type PointerEvent } from "react";
import { api, query, date, actions, formatNumber, short } from "./api";
import { Badge, Empty, ErrorBox, JsonView, Role, useAsync, type Json } from "./components";
import { useWorkspace } from "./context";

export const nodeKinds: Record<string, string> = {
  goal: "목표", run: "업무 실행", context_request: "정보 요청", context_pack: "Context Pack",
  work_order: "Work Order", observation: "관찰", result: "결과", artifact: "파일",
  ingestion: "본문 분석", source_unit: "본문 구간", submission: "지식 제출",
  decision: "채택·지역 보존·반려", knowledge: "Data / Info / Intel", review: "재심사",
  maintenance: "Master 감사", milestone: "결정·상태 변화", protocol: "프로토콜",
};
const relations: Record<string, string> = {
  starts: "시작", requests: "요청", delivers: "전달", pins: "버전 고정", delegates: "지시",
  uses: "사용", grants: "입력 허용", produces: "생성", observes: "관찰", attaches: "첨부", analyzes: "분석",
  reads: "읽음", submits: "제출", decides: "심사", adopts: "반영", reviews: "재심사",
  audits: "감사", changes: "변경", references: "참조", derived_from: "도출 근거",
  supports: "지지", contradicts: "반증", depends_on: "의존", supersedes: "대체",
};
const stages: Record<string, number> = {
  goal: 0, run: 1, ingestion: 1, review: 1, context_request: 2, artifact: 2,
  context_pack: 3, source_unit: 3, work_order: 4, observation: 5, result: 5,
  submission: 6, decision: 7, knowledge: 8, maintenance: 8, milestone: 5, protocol: 2,
};
const sources: Record<string, string> = { execution: "업무 실행", ingestion: "본문 분석", retention: "지식 재심사" };
export function sessionKey(s: Json) { return `${s.source}:${s.source_id}:${s.role}`; }
export function nodeSession(n: Json): Json | undefined {
  const source = n.detail?.session_source;
  const source_id = source === "execution" ? n.detail.run_id : n.detail?.job_id;
  if (source && source_id) return { source, source_id, role: n.role, ordinal: n.detail.ordinal };
  const parent = (n.parents ?? []).find((p: Json) => p.id.startsWith("run:") || p.id.startsWith("ingestion:"));
  return parent ? { source: parent.id.startsWith("run:") ? "execution" : "ingestion", source_id: parent.id.split(":")[1], role: n.kind === "work_order" ? "agent" : n.role } : undefined;
}
export function graphLayout(nodes: Json[]): Json[] {
  const slots = Array(9).fill(0);
  return nodes.map((n) => {
    const stage = n.kind === "artifact" && n.detail?.work_order_id ? 5 : stages[n.kind] ?? 5;
    return { ...n, x: 28 + stage * 260, y: 48 + slots[stage]++ * 118 };
  });
}
function NodeTags({ node }: { node: Json }) {
  const d = node.detail ?? {};
  return <div className="row"><span className="node-kind">{nodeKinds[node.kind] ?? node.kind}</span>
    <Role role={node.role} /><Badge value={node.status} />
    {d.category && <span>{String(d.category).toUpperCase()}</span>}
    {d.version && <code>v{d.version}</code>}
    {d.retention_scope && <Badge value={d.retention_scope} />}
    {d.outcome && d.outcome !== "not_applicable" && <Badge value={d.outcome} />}
  </div>;
}
function NodeDetail({ node, nodes, onFocus, onSession }: {
  node: Json; nodes: Json[]; onFocus: (id: string) => void; onSession: (s: Json) => void;
}) {
  const w = useWorkspace();
  const lane = nodeSession(node);
  return <section className="node-detail" aria-label="선택 항목 상세">
    <NodeTags node={node} /><h3>{actions[node.title] ?? node.title}</h3>
    <small>{date(node.created_at)} · <code>{node.id}</code></small>
    <p className="prose">{node.summary}</p>
    <div className="row">
      <button onClick={() => onFocus(node.id)}>주변 관계 펼치기</button>
      {node.detail?.item_id && node.detail?.version && <button onClick={() => w.openKnowledge(node.detail.item_id, node.detail.version)}>고정 개정 v{node.detail.version} 열기</button>}
      {lane && <button onClick={() => onSession(lane)}>Sessions에서 실행 보기</button>}
    </div>
    {(node.parents ?? []).length > 0 && <div className="relation-list"><strong>연결된 근거·상위 항목</strong>
      {node.parents.map((p: Json) => <button key={`${p.id}:${p.relation}`} onClick={() => onFocus(p.id)}>
        {relations[p.relation] ?? p.relation} · {nodes.find((x) => x.id === p.id)?.title ?? p.id}
      </button>)}
    </div>}
    <details><summary>서버 기록·근거</summary><JsonView value={node.detail} /></details>
  </section>;
}
function Graph({ nodes, edges, selected, onSelect }: { nodes: Json[]; edges: Json[]; selected?: string; onSelect: (n: Json) => void }) {
  const [zoom, setZoom] = useState(0.8);
  const [drag, setDrag] = useState<{ x: number; y: number; left: number; top: number }>();
  const positioned = graphLayout(nodes);
  const lookup = new Map(positioned.map((n) => [n.id, n]));
  const width = 2390, height = Math.max(520, ...positioned.map((n) => n.y + 130));
  function down(e: PointerEvent<HTMLDivElement>) {
    if (e.button !== 0 || (e.target as Element).closest('[role="button"]')) return;
    setDrag({ x: e.clientX, y: e.clientY, left: e.currentTarget.scrollLeft, top: e.currentTarget.scrollTop });
    e.currentTarget.setPointerCapture(e.pointerId);
  }
  return <>
    <div className="row graph-tools"><strong>업무·근거 관계도</strong>
      <button aria-label="관계도 축소" onClick={() => setZoom(Math.max(0.4, zoom - 0.15))}>−</button>
      <span>{Math.round(zoom * 100)}%</span><button aria-label="관계도 확대" onClick={() => setZoom(Math.min(1.5, zoom + 0.15))}>＋</button>
      <button onClick={() => setZoom(0.8)}>크기 복원</button><small>빈 공간을 끌거나 스크롤하여 이동 · 노드를 선택해 근거 확인</small>
    </div>
    <div className={`graph-viewport ${drag ? "dragging" : ""}`} onPointerDown={down}
      onPointerMove={(e) => { if (drag) { e.currentTarget.scrollLeft = drag.left - (e.clientX - drag.x); e.currentTarget.scrollTop = drag.top - (e.clientY - drag.y); } }}
      onPointerUp={() => setDrag(undefined)} onPointerCancel={() => setDrag(undefined)}>
      <svg width={width * zoom} height={height * zoom} viewBox={`0 0 ${width} ${height}`} role="group" aria-label="목표와 지식 근거의 연결">
        <defs><marker id="path-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" /></marker></defs>
        {["목표", "실행", "요청·입력", "선별·분석", "지시", "관찰·결과", "제출", "심사", "지식·감사"].map((s, i) => <text key={s} x={28 + i * 260} y={24} className="graph-stage">{s}</text>)}
        {edges.map((e) => {
          const a = lookup.get(e.source), b = lookup.get(e.target);
          if (!a || !b) return null;
          const x1 = a.x + 222, y1 = a.y + 47, x2 = b.x, y2 = b.y + 47;
          return <path key={`${e.source}:${e.relation}:${e.target}`} className={`graph-edge ${selected === a.id || selected === b.id ? "highlight" : ""}`}
            d={`M${x1},${y1} C${x1 + 65},${y1} ${x2 - 65},${y2} ${x2},${y2}`} markerEnd="url(#path-arrow)">
            <title>{relations[e.relation] ?? e.relation}: {a.title} → {b.title}</title>
          </path>;
        })}
        {positioned.map((n) => <g key={n.id} role="button" tabIndex={0} aria-label={`${nodeKinds[n.kind]}: ${n.title}`} className={`graph-node ${selected === n.id ? "selected" : ""} ${["failed", "refuted", "rejected", "blocked"].includes(n.status) || n.detail?.outcome === "failed" ? "negative" : ""}`}
          transform={`translate(${n.x},${n.y})`} onClick={() => onSelect(n)} onKeyDown={(e) => { if (["Enter", " "].includes(e.key)) { e.preventDefault(); onSelect(n); } }}>
          <title>{n.title}</title><rect width={222} height={96} rx={9} />
          <text x={12} y={21} className="graph-kind">{nodeKinds[n.kind]} {n.detail?.version ? `· v${n.detail.version}` : ""}</text>
          <text x={12} y={46}>{String(actions[n.title] ?? n.title).slice(0, 20)}</text>
          <text x={12} y={66}>{String(actions[n.title] ?? n.title).slice(20, 40)}</text>
          <text x={12} y={85} className="graph-state">{n.role} · {n.status}</text>
        </g>)}
      </svg>
    </div>
  </>;
}
function BusinessView({ task, mode, focus, setFocus, onPath, onSession }: {
  task: string; mode: string; focus?: string; setFocus: (v?: string) => void; onPath: () => void; onSession: (s: Json) => void;
}) {
  const w = useWorkspace();
  const [kind, setKind] = useState("");
  const [search, setSearch] = useState("");
  const [q, setQ] = useState("");
  const [offset, setOffset] = useState(0);
  const [selected, setSelected] = useState<Json>();
  const data = useAsync(() => api(`/v1/operator/tasks/${task}/${mode}?${query({ tenant_id: w.tenant, kind: focus ? "" : kind, q: focus ? "" : q, offset: focus ? 0 : offset, limit: 40, focus })}`), [task, w.tenant, w.revision, mode, kind, q, offset, focus]);
  const page = data.data;
  function browse(id?: string) { setFocus(id); setOffset(0); setSelected(undefined); onPath(); }
  const selectedNow = page?.nodes?.find((n: Json) => n.id === selected?.id) ?? selected;
  return <section className="panel business-view">
    <h2>{mode === "path" ? "Path · 업무와 근거의 연결" : "Broadcast · 업무 변화와 결과"}</h2>
    <p className="muted">{mode === "path" ? "서버에 기록된 목표·지시·관찰·심사와 고정 버전의 관계입니다. 선택 항목의 주변 관계를 펼칠 수 있습니다." : "업무 기록을 생성 시각순으로 보여줍니다. 상태와 보관 범위는 현재 값이며, 호출·오류·재시도 상세는 Sessions에서 확인합니다."}</p>
    <div className="observability-filters">
      <label>종류<select value={kind} onChange={(e) => { setKind(e.target.value); setFocus(undefined); setOffset(0); setSelected(undefined); }}><option value="">모든 업무 기록</option>{Object.entries(nodeKinds).map(([k, v]) => <option key={k} value={k}>{v}</option>)}</select></label>
      <form onSubmit={(e) => { e.preventDefault(); setQ(search); setFocus(undefined); setOffset(0); setSelected(undefined); }}><label>내용 검색<input value={search} maxLength={100} onChange={(e) => setSearch(e.target.value)} /></label><button>검색</button></form>
      {focus && <button onClick={() => browse()}>주변 보기 해제</button>}
    </div>
    <ErrorBox error={data.error} />
    {page && <>
      <div className="row pagination"><span>{focus ? "선택 항목" : `전체 ${formatNumber(page.total)}건 중 ${page.items.length ? offset + 1 : 0}–${offset + page.items.length}건`}</span>
        {!focus && <><button disabled={!offset} onClick={() => setOffset(Math.max(0, offset - 40))}>이전</button><button disabled={!page.has_more} onClick={() => setOffset(offset + 40)}>다음</button></>}
        <button onClick={() => { setOffset(0); setFocus(undefined); data.reload(); }}>최신 기록</button>
      </div>
      {mode === "path" && page.nodes.length > 0 && <><p className="muted">현재 페이지와 인접 항목 {page.nodes.length}개 · 전체 회사 그래프를 자동으로 펼치지 않습니다.{page.neighbours_truncated ? " 인접 항목이 많아 200개까지 표시합니다. 항목을 선택해 범위를 좁히세요." : ""}</p>
        <Graph nodes={page.nodes} edges={page.edges} selected={selectedNow?.id} onSelect={setSelected} />
      </>}
      <div className={mode === "broadcast" ? "business-timeline" : "path-records"} aria-label={mode === "broadcast" ? "업무 방송 기록" : "관계도 항목 목록"}>
        {page.items.map((n: Json) => <article className={`business-record ${selectedNow?.id === n.id ? "selected" : ""}`} key={n.id}>
          <small>{date(n.created_at)}</small><NodeTags node={n} />
          <button className="record-title" onClick={() => setSelected(selectedNow?.id === n.id ? undefined : n)}>{actions[n.title] ?? n.title}</button>
          <p className="prose">{String(n.summary ?? "").slice(0, 600)}</p>
          <div className="row"><small>{n.parents.length}개 근거·상위 연결</small><button onClick={() => browse(n.id)}>Path에서 관계 보기</button></div>
          {mode === "broadcast" && selectedNow?.id === n.id && <NodeDetail node={selectedNow} nodes={page.nodes} onFocus={browse} onSession={onSession} />}
        </article>)}
      </div>
      {mode === "path" && selectedNow && <NodeDetail node={selectedNow} nodes={page.nodes} onFocus={browse} onSession={onSession} />}
      {!page.items.length && <Empty title="해당 업무 기록이 없습니다" />}
    </>}
    {!page && !data.error && <p className="muted">업무 기록을 불러오고 있습니다…</p>}
  </section>;
}
function SessionLane({ task, lane }: { task: string; lane: Json }) {
  const w = useWorkspace();
  const [before, setBefore] = useState<number>();
  const [ordinal, setOrdinal] = useState<number | undefined>(lane.ordinal);
  const [number, setNumber] = useState(lane.ordinal != null ? String(lane.ordinal + 1) : "");
  const data = useAsync(() => api(`/v1/operator/tasks/${task}/session?${query({ tenant_id: w.tenant, source: lane.source, source_id: lane.source_id, role: lane.role, limit: 40, before, ordinal })}`), [task, w.tenant, w.revision, lane.source, lane.source_id, lane.role, before, ordinal]);
  const page = data.data;
  return <div className="session-transcript" aria-label="선택 세션 실행 기록">
    <div className="section-heading"><h3><Role role={lane.role} /> {sources[lane.source]} · {short(lane.source_id)}</h3><Badge value={page?.status ?? lane.status} /></div>
    <p className="muted">저장된 공개 지시·도구 입력·출력·판단 기록입니다. 원본 이력은 유지하며 긴 값은 표시 범위를 제한합니다.</p>
    <div className="row pagination"><button onClick={() => { setBefore(undefined); data.reload(); }}>최신 실행 기록</button><button disabled={!page?.has_more} onClick={() => setBefore(page.next_before)}>이전 실행 기록</button>
      {before && <small>과거 기록 조회 중 · 커서 {before}</small>}
    </div>
    {["ingestion", "retention"].includes(lane.source) && <form className="row unit-picker" onSubmit={(e) => { e.preventDefault(); setOrdinal(number ? Number(number) - 1 : undefined); setBefore(undefined); }}>
      <label>{lane.source === "ingestion" ? "본문 구간 번호" : "심사 항목 번호"}<input type="number" min={1} max={1000001} value={number} onChange={(e) => setNumber(e.target.value)} placeholder="전체" /></label><button>구간 조회</button>
      {ordinal != null && <button type="button" onClick={() => { setNumber(""); setOrdinal(undefined); setBefore(undefined); }}>전체 구간</button>}
    </form>}
    <ErrorBox error={data.error} />
    {page?.unit && <section className="unit-output"><h4>{ordinal! + 1}번의 저장된 역할 산출물</h4><p>해당 구간에서 실제 저장된 결과입니다. 완료된 모델 요청을 다시 실행하지 않습니다.</p><JsonView value={page.unit} /></section>}
    {page?.events?.map((e: Json) => <article className={`session-event ${e.kind.includes("failed") || e.kind.includes("denied") || e.kind.includes("stopped") ? "negative" : ""}`} key={e.seq}>
      <div className="row"><strong>{actions[e.details?.tool] ?? actions[e.kind] ?? e.kind}</strong>{e.ordinal != null && <button onClick={() => { setOrdinal(e.ordinal); setNumber(String(e.ordinal + 1)); setBefore(undefined); }}>구간 {e.ordinal + 1}</button>}<small>{date(e.created_at)} · #{e.seq}</small></div>
      {e.details?.phase && <p className="muted">단계 · {e.details.phase}</p>}
      {e.details?.usage && <p>{formatNumber(e.details.usage.total_tokens)} tokens · 입력 {formatNumber(e.details.usage.input_tokens)} / 출력 {formatNumber(e.details.usage.output_tokens)}</p>}
      {e.request && <details><summary>도구 지시·입력 · {e.request.tool}</summary><JsonView value={e.request.input} /></details>}
      {e.outcome && <details><summary>실제 도구 출력·영수증</summary><JsonView value={e.outcome} /></details>}
      <details><summary>이벤트 상세</summary><JsonView value={e.details} /></details>
    </article>)}
    {page && !page.events.length && <Empty title="이 범위의 실행 이벤트가 없습니다" />}
    {!page && !data.error && <p>실행 기록을 불러오고 있습니다…</p>}
  </div>;
}
function Sessions({ task, initial }: { task: string; initial?: Json }) {
  const w = useWorkspace();
  const [offset, setOffset] = useState(0);
  const [role, setRole] = useState("");
  const [selected, setSelected] = useState(initial);
  const data = useAsync(() => api(`/v1/operator/tasks/${task}/sessions?${query({ tenant_id: w.tenant, offset, limit: 40 })}`), [task, w.tenant, w.revision, offset]);
  const page = data.data;
  const active = selected ?? page?.sessions?.[0];
  return <section className="panel sessions-view"><h2>Sessions · 역할별 실행</h2>
    <p className="muted">업무 실행·본문 분석·지식 재심사의 역할별 기록을 함께 조회합니다. 세션 선택은 업무 실행이나 권한을 바꾸지 않습니다.</p>
    <ErrorBox error={data.error} />
    <div className="sessions-layout"><aside className="session-list" aria-label="역할별 세션 목록">
      <label>역할<select value={role} onChange={(e) => setRole(e.target.value)}><option value="">모든 역할</option>{["handler", "analyst", "agent", "master"].map((r) => <option key={r} value={r}>{r}</option>)}</select></label>
      {(page?.sessions ?? []).filter((s: Json) => !role || s.role === role).map((s: Json) => <button className={`session-item ${active && sessionKey(active) === sessionKey(s) ? "selected" : ""}`} key={sessionKey(s)} onClick={() => setSelected(s)}>
        <span><Role role={s.role} /> {sources[s.source]}</span><Badge value={s.status} />
        <code>{short(s.source_id)}</code><small>실행 전체 단계 · {s.phase || "—"}</small>
        <small>{formatNumber(s.calls)} calls · {formatNumber(s.tokens)} tokens</small>
        <small>{date(s.last_activity)}</small>{s.error_code && <span className="error-code">{s.error_code}</span>}
      </button>)}
      {page && <div className="row pagination"><small>{page.total}개 역할 세션</small><button disabled={!offset} onClick={() => setOffset(Math.max(0, offset - 40))}>이전</button><button disabled={!page.has_more} onClick={() => setOffset(offset + 40)}>다음</button></div>}
    </aside>{active ? <SessionLane key={sessionKey(active)} task={task} lane={active} /> : <Empty title="아직 실행 세션이 없습니다" />}</div>
  </section>;
}
export function TaskObservability({ task, tab, onTab }: { task: string; tab: string; onTab: (v: string) => void }) {
  const [focus, setFocus] = useState<string>();
  const [lane, setLane] = useState<Json>();
  function showSession(s: Json) { setLane(s); onTab("sessions"); }
  if (tab === "sessions") return <Sessions task={task} initial={lane} />;
  return <BusinessView task={task} mode={tab} focus={focus} setFocus={setFocus} onPath={() => onTab("path")} onSession={showSession} />;
}
