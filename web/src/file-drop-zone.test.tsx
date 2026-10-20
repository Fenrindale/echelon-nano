// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { FileDropZone } from "./file-drop-zone";
import { TaskForm } from "./tasks";

vi.mock("./context", () => ({ useWorkspace: () => ({ tenant: "owned-ui-only", refresh: vi.fn() }) }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});
function zone() { return host.querySelector('[role="group"]')!; }
async function drag(target: Element, type: string, files: File[], options: { directory?: boolean; text?: boolean } = {}) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  const transfer = {
    types: options.text ? ["text/plain"] : ["Files"], files, dropEffect: "none",
    items: options.directory ? [{ kind: "file", webkitGetAsEntry: () => ({ isDirectory: true }) }] : [],
  };
  Object.defineProperty(event, "dataTransfer", { value: transfer });
  await act(() => { target.dispatchEvent(event); });
  return { event, transfer };
}
async function pick(files: File[]) {
  const input = host.querySelector('input[type="file"]')!;
  Object.defineProperty(input, "files", { value: files, configurable: true });
  await act(() => { input.dispatchEvent(new Event("change", { bubbles: true })); });
}

it("combines dropped and selected files in the real Task form and supports removal", async () => {
  await act(() => root.render(<TaskForm onCreated={vi.fn()} />));
  await drag(zone(), "drop", [new File(["상태,건수\n완료,7"], "집계.csv"), new File(["참고"], "메모.txt")]);
  await pick([new File([new Uint8Array([0, 255, 128])], "원본.bin")]);
  expect(Array.from(host.querySelectorAll(".file-row strong"), (e) => e.textContent)).toEqual(["집계.csv", "메모.txt", "원본.bin"]);
  await act(() => { (host.querySelector('[aria-label="메모.txt 첨부 취소"]') as HTMLButtonElement).click(); });
  expect(Array.from(host.querySelectorAll(".file-row strong"), (e) => e.textContent)).toEqual(["집계.csv", "원본.bin"]);
});

it("rejects invalid drops atomically without discarding previously selected files", async () => {
  await act(() => root.render(<TaskForm onCreated={vi.fn()} />));
  await pick([new File(["keep"], "keep.txt")]);
  await drag(zone(), "drop", [new File(["other"], "valid.txt"), new File(["duplicate"], "keep.txt")]);
  expect(host.textContent).toContain("같은 이름");
  expect(host.querySelectorAll(".file-row")).toHaveLength(1);
  await drag(zone(), "drop", [new File([new Uint8Array(1048577)], "large.bin")]);
  expect(host.textContent).toContain("파일당 1 MiB");
  expect(host.querySelectorAll(".file-row")).toHaveLength(1);
  await drag(zone(), "drop", Array.from({ length: 16 }, (_, i) => new File(["x"], `${i}.txt`)));
  expect(host.textContent).toContain("최대 16개");
  await drag(zone(), "drop", Array.from({ length: 4 }, (_, i) => new File([new Uint8Array(1048576)], `${i}.bin`)));
  expect(host.textContent).toContain("합계는 4 MiB");
  expect(host.querySelectorAll(".file-row")).toHaveLength(1);
});

it("keeps the highlight over nested elements and resets it after a drop or exit", async () => {
  const onFiles = vi.fn();
  await act(() => root.render(<FileDropZone onFiles={onFiles} onError={vi.fn()} hint="limit" />));
  const file = new File(["exact"], "input.txt");
  await drag(zone(), "dragenter", []);
  await drag(host.querySelector("button")!, "dragenter", []);
  await drag(host.querySelector("button")!, "dragleave", []);
  expect(zone().classList.contains("dragging")).toBe(true);
  expect(host.textContent).toContain("여기에 놓아");
  const over = await drag(zone(), "dragover", []);
  expect(over.transfer.dropEffect).toBe("copy");
  await drag(zone(), "drop", [file]);
  expect(onFiles).toHaveBeenCalledWith([file]);
  expect(zone().classList.contains("dragging")).toBe(false);
  await drag(zone(), "dragenter", []);
  await drag(zone(), "dragleave", []);
  expect(zone().classList.contains("dragging")).toBe(false);
});

it("blocks directory and multiple-file drops on the existing single-file uploader", async () => {
  const onFiles = vi.fn(), onError = vi.fn();
  await act(() => root.render(<FileDropZone onFiles={onFiles} onError={onError} hint="limit" />));
  await drag(zone(), "drop", [], { directory: true });
  expect(onError.mock.lastCall?.[0].message).toContain("폴더");
  await drag(zone(), "drop", [new File([], "a.txt"), new File([], "b.txt")]);
  expect(onError.mock.lastCall?.[0].message).toContain("하나씩");
  expect(onFiles).not.toHaveBeenCalled();
});

it("disables file acceptance while sending and leaves text drag-and-drop alone", async () => {
  const onFiles = vi.fn();
  await act(() => root.render(<FileDropZone disabled onFiles={onFiles} onError={vi.fn()} hint="limit" />));
  await drag(zone(), "dragenter", []);
  const over = await drag(zone(), "dragover", []);
  await drag(zone(), "drop", [new File([], "a.txt")]);
  expect(over.transfer.dropEffect).toBe("none");
  expect(zone().classList.contains("dragging")).toBe(false);
  expect(onFiles).not.toHaveBeenCalled();
  const text = await drag(zone(), "drop", [], { text: true });
  expect(text.event.defaultPrevented).toBe(false);
  expect((host.querySelector("button") as HTMLButtonElement).disabled).toBe(true);
});

it("prevents file navigation outside the target only while an uploader is mounted", async () => {
  const onFiles = vi.fn();
  await act(() => root.render(<FileDropZone onFiles={onFiles} onError={vi.fn()} hint="limit" />));
  const outside = await drag(document.body, "drop", [new File([], "a.txt")]);
  expect(outside.event.defaultPrevented).toBe(true);
  expect(onFiles).not.toHaveBeenCalled();
  await act(() => root.render(null));
  expect((await drag(document.body, "drop", [new File([], "a.txt")])).event.defaultPrevented).toBe(false);
});
