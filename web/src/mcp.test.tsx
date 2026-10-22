// @vitest-environment jsdom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { Mcp } from "./mcp";
const { request } = vi.hoisted(() => ({ request: vi.fn() }));
vi.mock("./api", () => ({ api: request, query: () => "tenant_id=owned", labels: {}, short: (s: string) => s }));
vi.mock("./context", () => ({ useWorkspace: () => ({ tenant: "owned", revision: 0, refresh: vi.fn(), notify: vi.fn() }) }));
vi.mock("./components", async () => {
  const actual = await vi.importActual<typeof import("./components")>("./components");
  return { ...actual, useAsync: () => ({ data: { servers: [], bundles: [] } }) };
});
let root: ReturnType<typeof createRoot> | undefined;
afterEach(async () => { if (root) await act(() => root!.unmount()); document.body.innerHTML = ""; vi.clearAllMocks(); });
it("reuses a saved MCP identity when installation fails, preserving a reviewable editable configuration", async () => {
  (globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;
  const saves: any[] = [];
  request.mockImplementation(async (path: string, _method: string, body: any) => {
    if (path === "/v1/operator/mcp") { saves.push(body); return { id: "saved-server", version: saves.length }; }
    throw new Error("Owned installation failure");
  });
  const host = document.createElement("div"); document.body.append(host); root = createRoot(host);
  await act(() => root!.render(<Mcp />));
  await act(() => (host.querySelector("button") as HTMLButtonElement).click());
  const form = document.querySelector("form")!;
  await act(() => { form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })); });
  expect(document.body.textContent).toContain("Owned installation failure");
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  await act(() => { form.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })); });
  expect(saves[0].id).toBeNull();
  expect(saves[1].id).toBe("saved-server");
  expect(saves[1].expected_version).toBe(1);
  expect(document.querySelector('input[type="password"]')).toBeNull();
});
