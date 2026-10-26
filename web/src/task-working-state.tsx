import { Badge, ID, type Json } from "./components";

export function TaskWorkingState({ state }: { state?: Json }) {
  if (!state?.goals?.length && !Object.keys(state?.steps ?? {}).length) return null;
  const steps: Json[] = Object.values(state?.steps ?? {});
  const active = steps.filter((s) => ["pending", "running"].includes(s.status));
  const settled = steps.filter((s) => !["pending", "running"].includes(s.status)).slice(-8);
  return <section className="panel" aria-label="업무 진행과 판단">
    <div className="section-heading"><h2>업무 진행과 판단</h2><span className="muted">Task 작업 기억</span></div>
    <ul>{(state?.goals ?? []).map((goal: Json) => <li key={goal.id}>
      <strong>{goal.evidence?.length ? "근거 확인" : "확인 예정"}</strong> · {goal.condition}
      {goal.reason && <p className="muted">{goal.reason}</p>}
    </li>)}</ul>
    {[...active, ...settled].map((step) => <article key={step.id} className="notice">
      <Badge value={step.status} /> <span>{step.objective}</span>
      {step.work_order_id && <ID id={step.work_order_id} />}
      {step.result?.summary && <p className="prose muted">{step.result.summary}</p>}
    </article>)}
    {(state?.notes ?? []).slice(-6).map((note: Json) => <p key={note.call_id} className="prose muted">{note.text}</p>)}
    <p className="muted">이 업무의 시도와 판단입니다. 재사용 지식은 별도로 심사합니다.</p>
  </section>;
}
