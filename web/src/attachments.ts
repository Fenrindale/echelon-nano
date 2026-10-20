import { key } from "./api";

export const FILE_LIMIT = 1048576;
export const TOTAL_LIMIT = 4 * FILE_LIMIT;
export const FILE_COUNT = 16;
export type Attachment = { id: string; file: File };

export function validateFiles(files: Pick<File, "name" | "size">[]) {
  if (files.length > FILE_COUNT) throw new Error("첨부파일은 최대 16개까지 선택할 수 있습니다.");
  if (files.some((f) => f.size > FILE_LIMIT)) throw new Error("파일당 1 MiB 이하여야 합니다.");
  if (files.reduce((sum, f) => sum + f.size, 0) > TOTAL_LIMIT) {
    throw new Error("첨부파일의 합계는 4 MiB 이하여야 합니다.");
  }
  if (new Set(files.map((f) => f.name)).size !== files.length) {
    throw new Error("같은 이름의 파일이 있습니다. 파일명을 다르게 지정해 주세요.");
  }
}
export function selectAttachments(current: Attachment[], added: File[]) {
  const next = [...current, ...added.map((file) => ({ id: key(), file }))];
  validateFiles(next.map((a) => a.file));
  return next;
}
export async function encodeAttachment({ id, file }: Attachment) {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  for (let i = 0; i < bytes.length; i += 32768) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 32768));
  }
  return {
    request_id: id,
    name: file.name,
    media_type: file.type || "application/octet-stream",
    data_base64: btoa(binary),
  };
}
