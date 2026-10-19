import {
  useEffect,
  useState,
  useId,
  useRef,
  isValidElement,
  cloneElement,
  type ReactElement,
  type ReactNode,
  type FormEvent,
} from "react";
import { labels, short } from "./api";
export type Json = Record<string, any>;
export function Badge({ value }: { value?: string }) {
  return (
    <span
      className={`badge ${["failed", "refuted", "cancelled", "rejected"].includes(value ?? "") ? "danger" : ["blocked", "paused", "interrupted", "exhausted", "review_needed", "expired", "pending", "candidate"].includes(value ?? "") ? "warning" : ["completed", "active", "supported", "accepted", "applied", "success"].includes(value ?? "") ? "success" : ""}`}
    >
      {labels[value ?? ""] ?? value ?? "—"}
    </span>
  );
}
export function Role({ role }: { role: string }) {
  return <span className="role">{role}</span>;
}
export function Empty({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="empty">
      <span className="empty-icon">◇</span>
      <h3>{title}</h3>
      <div className="empty-description">{children}</div>
    </div>
  );
}
export function ErrorBox({ error }: { error: unknown }) {
  return error ? (
    <div className="error" role="alert">
      {error instanceof Error ? error.message : String(error)}
    </div>
  ) : null;
}
export function Modal({
  title,
  children,
  onClose,
  wide = false,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  wide?: boolean;
}) {
  const element = useRef<HTMLElement>(null);
  const close = useRef(onClose);
  close.current = onClose;
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const overflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    const first = element.current?.querySelector<HTMLElement>(
      "input:not([type=hidden]),textarea,select,button",
    );
    first?.focus();
    const f = (e: KeyboardEvent) => {
      const dialogs = document.querySelectorAll("[role=dialog]");
      if (dialogs[dialogs.length - 1] !== element.current) return;
      if (e.key === "Escape") {
        e.stopPropagation();
        close.current();
      }
      if (e.key === "Tab") {
        const focusable = Array.from(
          element.current?.querySelectorAll<HTMLElement>(
            'button:not(:disabled),a[href],input:not(:disabled),textarea:not(:disabled),select:not(:disabled),[tabindex="0"]',
          ) ?? [],
        ).filter((x) => x.getClientRects().length);
        const first = focusable[0],
          last = focusable[focusable.length - 1];
        if (e.shiftKey && document.activeElement === first) {
          e.preventDefault();
          last?.focus();
        } else if (!e.shiftKey && document.activeElement === last) {
          e.preventDefault();
          first?.focus();
        }
      }
    };
    window.addEventListener("keydown", f);
    return () => {
      window.removeEventListener("keydown", f);
      document.body.style.overflow = overflow;
      if (previous?.isConnected) previous.focus();
    };
  }, []);
  return (
    <div className="overlay" onClick={onClose}>
      <section
        ref={element}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        className={`modal ${wide ? "wide" : ""}`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-head">
          <h2>{title}</h2>
          <button className="icon-button" aria-label="닫기" onClick={onClose}>
            ×
          </button>
        </div>
        {children}
      </section>
    </div>
  );
}
export function Field({
  label,
  children,
  hint,
}: {
  label: string;
  children: ReactNode;
  hint?: string;
}) {
  const id = useId();
  const simple =
    isValidElement(children) &&
    typeof children.type === "string" &&
    ["input", "textarea", "select"].includes(children.type);
  return (
    <div className="field">
      {simple ? <label htmlFor={id}>{label}</label> : <span>{label}</span>}
      {simple
        ? cloneElement(children as ReactElement<any>, {
            id,
            "aria-describedby": hint ? `${id}-hint` : undefined,
          })
        : children}
      {hint && <small id={`${id}-hint`}>{hint}</small>}
    </div>
  );
}
export function JsonView({ value }: { value: unknown }) {
  return <pre className="json">{JSON.stringify(value, null, 2)}</pre>;
}
export function Tags({ tags }: { tags?: string[] }) {
  return (
    <div className="tags">
      {tags?.map((tag) => (
        <span
          key={tag}
          className={tag === "거짓" || tag === "실패" ? "tag caution" : "tag"}
        >
          {tag}
        </span>
      ))}
    </div>
  );
}
export function useAsync<T>(load: () => Promise<T>, deps: unknown[]) {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<unknown>();
  const [revision, setRevision] = useState(0);
  useEffect(() => {
    let live = true;
    setError(undefined);
    load()
      .then((v) => {
        if (live) setData(v);
      })
      .catch((e) => {
        if (live) setError(e);
      });
    return () => {
      live = false;
    };
  }, [...deps, revision]);
  return { data, error, reload: () => setRevision((v) => v + 1) };
}
export function SubmitButton({
  busy,
  children,
}: {
  busy: boolean;
  children: ReactNode;
}) {
  return (
    <button className="primary" type="submit" disabled={busy}>
      {busy ? "처리 중…" : children}
    </button>
  );
}
export function ID({ id }: { id?: string }) {
  return <code title={id}>{short(id)}</code>;
}
export function SubmitForm({
  children,
  onSubmit,
  button = "저장",
}: {
  children: ReactNode;
  onSubmit: () => Promise<unknown>;
  button?: string;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>();
  async function submit(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      await onSubmit();
    } catch (e) {
      setError(e);
    } finally {
      setBusy(false);
    }
  }
  return (
    <form onSubmit={submit}>
      {children}
      <ErrorBox error={error} />
      <div className="form-actions">
        <SubmitButton busy={busy}>{button}</SubmitButton>
      </div>
    </form>
  );
}
