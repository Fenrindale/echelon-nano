import { expect, it } from "vitest";
import { encodeAttachment, FILE_LIMIT, validateFiles } from "./attachments";

it("preserves arbitrary bytes and Korean filenames without text conversion", async () => {
  const bytes = new Uint8Array([0, 255, 128, 65, 13, 10]);
  const file = new File([bytes], "첨부 자료.bin", { type: "application/octet-stream" });
  const encoded = await encodeAttachment({ id: "owned-request", file });
  expect(encoded.name).toBe("첨부 자료.bin");
  expect(Array.from(atob(encoded.data_base64), (c) => c.charCodeAt(0))).toEqual(Array.from(bytes));
});

it("checks individual, aggregate, count and filename limits before upload", () => {
  expect(() => validateFiles([{ name: "large", size: FILE_LIMIT + 1 }])).toThrow("파일당");
  expect(() => validateFiles(Array.from({ length: 5 }, (_, i) => ({ name: `${i}`, size: FILE_LIMIT })))).toThrow("합계");
  expect(() => validateFiles(Array.from({ length: 17 }, (_, i) => ({ name: `${i}`, size: 1 })))).toThrow("최대");
  expect(() => validateFiles([{ name: "same", size: 1 }, { name: "same", size: 2 }])).toThrow("같은 이름");
  expect(() => validateFiles([{ name: "empty", size: 0 }])).not.toThrow();
});
