import { afterEach, expect, it, vi } from "vitest";
import { key } from "./api";

afterEach(() => vi.unstubAllGlobals());

it("creates distinct valid effect IDs when an HTTP LAN origin lacks randomUUID", () => {
  const getRandomValues = crypto.getRandomValues.bind(crypto);
  vi.stubGlobal("crypto", { getRandomValues });
  const ids = Array.from({ length: 64 }, () => key());
  expect(new Set(ids).size).toBe(ids.length);
  for (const id of ids)
    expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
});
