import { describe, it, expect } from "vitest";
import { readableAccent, contrast } from "./theme";
describe("single accent preserves legibility", () => {
  it("keeps every tested shade readable on the fixed surface", () => {
    for (const color of [
      "#000000",
      "#101519",
      "#223344",
      "#ff0000",
      "#00ff00",
      "#0000ff",
      "#ffffff",
      "#78cfbf",
    ])
      expect(contrast(readableAccent(color), "#11171b")).toBeGreaterThanOrEqual(
        4.5,
      );
  });
  it("preserves an already legible selection", () =>
    expect(readableAccent("#78cfbf")).toBe("#78cfbf"));
});
