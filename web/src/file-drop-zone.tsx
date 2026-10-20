import { useEffect, useRef, useState, type ReactNode } from "react";
import { validateFiles } from "./attachments";

function isFileDrag(transfer: DataTransfer | null) {
  return !!transfer && (Array.from(transfer.types).includes("Files") || transfer.files.length > 0);
}

export function FileDropZone({
  disabled = false,
  multiple = false,
  title,
  button = "＋ 파일 선택",
  hint,
  children,
  onFiles,
  onError,
}: {
  disabled?: boolean;
  multiple?: boolean;
  title?: ReactNode;
  button?: string;
  hint: string;
  children?: ReactNode;
  onFiles: (files: File[]) => void | Promise<void>;
  onError: (error: unknown) => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const depth = useRef(0);
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    // Dropping a file beside the target must not navigate away from an unsaved Task.
    const preventFileNavigation = (event: DragEvent) => {
      if (isFileDrag(event.dataTransfer)) event.preventDefault();
    };
    document.addEventListener("dragover", preventFileNavigation);
    document.addEventListener("drop", preventFileNavigation);
    return () => {
      document.removeEventListener("dragover", preventFileNavigation);
      document.removeEventListener("drop", preventFileNavigation);
    };
  }, []);

  useEffect(() => {
    if (disabled) {
      depth.current = 0;
      setDragging(false);
    }
  }, [disabled]);

  async function accept(files: File[], directory = false) {
    if (disabled) return;
    try {
      if (directory) throw new Error("폴더는 첨부할 수 없습니다. 폴더 안의 파일을 선택해 주세요.");
      if (!files.length) return;
      if (!multiple && files.length > 1) throw new Error("파일은 한 번에 하나씩 첨부해 주세요.");
      validateFiles(files);
      onError(undefined);
      await onFiles(files);
    } catch (error) {
      onError(error);
    }
  }

  return (
    <div
      className={`file-drop-zone${dragging && !disabled ? " dragging" : ""}${disabled ? " disabled" : ""}`}
      role="group"
      aria-label="첨부파일 업로드"
      aria-disabled={disabled}
      onDragEnter={(event) => {
        if (!isFileDrag(event.dataTransfer)) return;
        event.preventDefault();
        if (!disabled && ++depth.current === 1) setDragging(true);
      }}
      onDragOver={(event) => {
        if (!isFileDrag(event.dataTransfer)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = disabled ? "none" : "copy";
      }}
      onDragLeave={(event) => {
        if (!isFileDrag(event.dataTransfer)) return;
        event.preventDefault();
        depth.current = Math.max(0, depth.current - 1);
        if (!depth.current) setDragging(false);
      }}
      onDrop={(event) => {
        if (!isFileDrag(event.dataTransfer)) return;
        event.preventDefault();
        depth.current = 0;
        setDragging(false);
        const directory = Array.from(event.dataTransfer.items).some(
          (item) => item.kind === "file" && item.webkitGetAsEntry?.()?.isDirectory,
        );
        void accept(Array.from(event.dataTransfer.files), directory);
      }}
    >
      <div className="row drop-zone-header">
        {title && <strong>{title}</strong>}
        <button type="button" disabled={disabled} onClick={() => input.current?.click()}>
          {button}
        </button>
        <input ref={input} type="file" multiple={multiple} hidden aria-label="첨부파일 선택" disabled={disabled}
          onChange={(event) => {
            const files = Array.from(event.currentTarget.files ?? []);
            event.currentTarget.value = "";
            void accept(files);
          }} />
      </div>
      <p className="drop-zone-prompt" aria-live="polite">
        {dragging && !disabled ? "여기에 놓아 첨부하세요" : "파일을 여기에 끌어 놓거나 파일을 선택하세요."}
      </p>
      {children}
      <small className="muted">{hint}</small>
    </div>
  );
}
