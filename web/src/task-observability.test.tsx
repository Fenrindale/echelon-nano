// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TaskObservability, graphLayout } from "./task-observability";
import { api } from "./api";
vi.mock("./api", async () => ({ ...await vi.importActual<typeof import("./api")>("./api"), api: vi.fn() }));
const openKnowledge = vi.fn();
vi.mock("./context", () => ({ useWorkspace: () => ({ tenant: "owned", revision: 0, openKnowledge }) }));
let host: HTMLDivElement, root: Root;
const node = { id: "knowledge:owned:3", kind: "knowledge", role: "analyst", title: "실패를 피하는 재사용 규칙", summary: "검증 실패 시 이전 결과를 보존한다", status: "refuted", created_at: "2026-10-21T00:00:00Z", parents: [], detail: { item_id: "owned", version: 3, outcome: "failed", retention_scope: "central" } };
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  vi.mocked(api).mockReset(); openKnowledge.mockReset();
});
afterEach(async () => { await act(() => root.unmount()); host.remove(); vi.unstubAllGlobals(); });
async function click(text: string) { const buttons = Array.from(host.querySelectorAll("button")).filter((b) => b.textContent === text); expect(buttons).toHaveLength(1); await act(() => buttons[0].click()); }
it("shows business records separately from execution events and opens the pinned version", async () => {
  vi.mocked(api).mockResolvedValue({ items: [node], nodes: [node], edges: [], total: 1, has_more: false });
  await act(() => root.render(<TaskObservability task="task" tab="broadcast" onTab={vi.fn()} />));
  expect(host.textContent).toContain("Broadcast · 업무 변화와 결과");
  expect(host.textContent).not.toContain("model.started");
  await click(node.title); await click("고정 개정 v3 열기");
  expect(openKnowledge).toHaveBeenCalledWith("owned", 3);
});
it("paginates business records rather than hiding everything beyond the newest window", async () => {
  vi.mocked(api).mockResolvedValue({ items: [node], nodes: [node], edges: [], total: 90, has_more: true });
  await act(() => root.render(<TaskObservability task="task" tab="broadcast" onTab={vi.fn()} />));
  await click("다음");
  expect(vi.mocked(api).mock.calls.at(-1)?.[0]).toContain("offset=40");
});
it("shows the ongoing ingestion lane alongside a completed ordinary run and pages immutable events", async () => {
  const lanes = [{ source: "ingestion", source_id: "body-job", role: "agent", status: "running", calls: 500, tokens: 1234, last_activity: "2026-10-21" }, { source: "execution", source_id: "old-run", role: "handler", status: "completed", calls: 4, tokens: 100, last_activity: "2026-10-20" }];
  vi.mocked(api).mockImplementation(async (path) => path.includes("/sessions?") ? { sessions: lanes, total: 2 } : { status: "running", events: [{ seq: 999, kind: "model.started", created_at: "2026-10-21", details: { phase: "ingestion.extract" }, ordinal: 4 }], next_before: 999, has_more: true, unit: null });
  await act(() => root.render(<TaskObservability task="task" tab="sessions" onTab={vi.fn()} />));
  expect(host.textContent).toContain("본문 분석"); expect(host.textContent).toContain("업무 실행");
  expect(vi.mocked(api).mock.calls.some(([p]) => p.includes("source=ingestion") && p.includes("source_id=body-job"))).toBe(true);
  await click("이전 실행 기록"); expect(vi.mocked(api).mock.calls.at(-1)?.[0]).toContain("before=999");
  await click("구간 5"); expect(vi.mocked(api).mock.calls.at(-1)?.[0]).toContain("ordinal=4");
});
it("exposes receipt input/output and visible errors without starting a new execution", async () => {
  vi.mocked(api).mockImplementation(async (p) => p.includes("/sessions?") ? { sessions: [{ source: "execution", source_id: "run", role: "agent", status: "failed", calls: 1, tokens: 10 }], total: 1 } : { status: "failed", events: [{ seq: 1, kind: "tool.failed", details: { tool: "artifact.write" }, request: { tool: "artifact.write", input: { name: "report.md" } }, outcome: { ok: false, error: { code: "owned_failure" } } }] });
  await act(() => root.render(<TaskObservability task="task" tab="sessions" onTab={vi.fn()} />));
  expect(host.textContent).toContain("실제 도구 출력·영수증"); expect(host.textContent).toContain("owned_failure");
  expect(vi.mocked(api).mock.calls.every(([, method]) => method === undefined)).toBe(true);
});
it("lays out branching knowledge and execution nodes with stable selectable identities", () => {
  const result = graphLayout([{ id: "g", kind: "goal" }, { id: "a", kind: "work_order" }, { id: "b", kind: "work_order" }, node, {id:"output",kind:"artifact",detail:{work_order_id:"a"}}]);
  expect(result[0].x).toBeLessThan(result[1].x); expect(result[1].x).toBe(result[2].x); expect(result[1].y).not.toBe(result[2].y); expect(result[3].id).toBe(node.id);
  expect(result[4].x).toBeGreaterThan(result[1].x);
});
