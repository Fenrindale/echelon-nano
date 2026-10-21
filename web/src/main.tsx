import { useEffect, useState, useRef, type FormEvent } from "react";
import { createRoot } from "react-dom/client";
import { api, setCsrf, query, key, formatNumber } from "./api";
import { Workspace } from "./context";
import {
  useAsync,
  Modal,
  Field,
  SubmitForm,
  ErrorBox,
  Empty,
  type Json,
} from "./components";
import { Tasks, TaskDetail, TaskForm } from "./tasks";
import { Central, KnowledgeDetail } from "./central";
import { Protocols, Dreaming, Vault, Settings } from "./governance";
import { applyAccent } from "./theme";
import "./style.css";

function App() {
  const [paired, setPaired] = useState<boolean | null>(null);
  const [error, setError] = useState<unknown>();
  const [tenants, setTenants] = useState<Json[]>([]);
  const [tenant, setTenant] = useState(
    () => localStorage.getItem("echelon.tenant") ?? "",
  );
  const [newTenant, setNewTenant] = useState(false);
  const [name, setName] = useState("");
  const [admin, setAdmin] = useState("");
  const [nonce, setNonce] = useState(0);
  useEffect(() => {
    let alive = true;
    async function connect() {
      try {
        if (location.hash.startsWith("#pair=")) {
          history.replaceState(null, "", location.pathname);
        }
        const session = await api("/v1/ui/session", "POST", {});
        setCsrf(session.csrf);
        if (alive) setPaired(true);
      } catch (e) {
        if (alive) {
          setPaired(false);
          setError(e);
        }
      }
    }
    connect();
    const lost = () => setNonce((v) => v + 1);
    window.addEventListener("echelon-auth-lost", lost);
    return () => {
      alive = false;
      window.removeEventListener("echelon-auth-lost", lost);
    };
  }, [nonce]);
  useEffect(() => {
    if (!paired) return;
    let alive = true;
    api("/v1/operator/tenants")
      .then((r) => {
        if (!alive) return;
        setTenants(r.tenants);
        if (!r.tenants.some((t: Json) => t.id === tenant)) {
          setTenant("");
          setAdmin("");
        }
      })
      .catch(setError);
    return () => {
      alive = false;
    };
  }, [paired, nonce]);
  useEffect(() => {
    if (!paired || !tenant) {
      setAdmin("");
      return;
    }
    let alive = true;
    setAdmin("");
    setError(undefined);
    localStorage.setItem("echelon.tenant", tenant);
    api(`/v1/operator/tenants/${tenant}/workspace`, "POST", {})
      .then((r) => {
        if (alive) setAdmin(r.administration_task_id);
      })
      .catch((e) => {
        if (alive) setError(e);
      });
    return () => {
      alive = false;
    };
  }, [paired, tenant]);
  if (paired === null)
    return (
      <div className="loading-screen">
        ECHELON NANO <span>연결 확인 중…</span>
      </div>
    );
  if (!paired)
    return (
      <>
        <ErrorBox error={error} />
        <div className="loading-screen">
          <span>운영 서버에 연결할 수 없습니다.</span>
          <button onClick={() => setNonce((v) => v + 1)}>다시 연결</button>
        </div>
      </>
    );
  const tenantName = tenants.find((t) => t.id === tenant)?.name ?? "";
  return (
    <>
      <Shell
        key={tenant || "none"}
        tenant={tenant}
        tenantName={tenantName}
        admin={admin}
        tenantPicker={
          <select
            aria-label="업무실"
            value={tenant}
            onChange={(e) => setTenant(e.target.value)}
          >
            <option value="">업무실 선택</option>
            {tenants.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        }
        newTenant={() => setNewTenant(true)}
      />
      {error && (
        <div className="toast">
          <ErrorBox error={error} />
        </div>
      )}
      {newTenant && (
        <Modal title="새 업무실" onClose={() => setNewTenant(false)}>
          <SubmitForm
            button="업무실 만들기"
            onSubmit={async () => {
              const r = await api("/v1/operator/tenants", "POST", { name });
              setTenants((v) => [...v, { id: r.id, name }]);
              setTenant(r.id);
              setName("");
              setNewTenant(false);
            }}
          >
            <Field
              label="이름"
              hint="중앙 지식과 Task는 업무실별로 분리됩니다."
            >
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="예: 에셜론 업무실"
                required
                maxLength={160}
              />
            </Field>
          </SubmitForm>
        </Modal>
      )}
    </>
  );
}
const nav = [
  ["tasks", "◫", "Tasks", "업무"],
  ["central", "⌘", "Central", "중앙 지식"],
  ["protocols", "◎", "Protocols", "프로토콜"],
  ["dreaming", "◇", "Dreaming", "감사·정리"],
  ["vault", "⌁", "Vault", "기밀 참조"],
  ["settings", "⚙", "Settings", "설정"],
];
function Shell({
  tenant,
  tenantName,
  admin,
  tenantPicker,
  newTenant,
}: {
  tenant: string;
  tenantName: string;
  admin: string;
  tenantPicker: React.ReactNode;
  newTenant: () => void;
}) {
  const [view, setView] = useState("tasks");
  const [task, setTask] = useState("");
  const [knowledge, setKnowledge] = useState<{
    id: string;
    version?: number;
  }>();
  const [revision, setRevision] = useState(0);
  const [providerRevision, setProviderRevision] = useState(0);
  const [connection, setConnection] = useState("");
  const [model, setModel] = useState("");
  const [toast, setToast] = useState("");
  const [create, setCreate] = useState(false);
  const [connected, setConnected] = useState(false);
  const [events, setEvents] = useState<Json[]>([]);
  const dirty = useRef(false);
  const initialized = useRef(false);
  const pref = useAsync(
    () =>
      tenant
        ? api(`/v1/operator/settings?${query({ tenant_id: tenant })}`)
        : Promise.resolve({}),
    [tenant],
  );
  const providerData = useAsync(
    () =>
      tenant
        ? api(`/v1/operator/providers?${query({ tenant_id: tenant })}`)
        : Promise.resolve({ connections: [] }),
    [tenant, providerRevision],
  );
  const catalog = useAsync(
    () =>
      tenant && connection
        ? api(
            `/v1/operator/provider-models?${query({ tenant_id: tenant, connection_id: connection })}`,
          )
        : Promise.resolve({ models: [] }),
    [tenant, connection],
  );
  const metrics = useAsync(
    () =>
      tenant
        ? api(`/v1/operator/metrics?${query({ tenant_id: tenant })}`)
        : Promise.resolve({ roles: [] }),
    [tenant, revision],
  );
  useEffect(() => {
    if (!pref.data) return;
    applyAccent(pref.data.accent ?? "#78cfbf");
    if (!initialized.current && providerData.data) {
      initialized.current = true;
      const choices = providerData.data.connections.filter(
        (v: Json) => v.enabled !== false && v.status !== "revoked",
      );
      const saved = pref.data.preferences ?? {};
      setConnection(
        choices.some((c: Json) => c.id === saved.connection_id)
          ? saved.connection_id
          : (choices[0]?.id ?? ""),
      );
      setModel(saved.model ?? "");
    }
  }, [pref.data, providerData.data]);
  useEffect(() => {
    const models = catalog.data?.models ?? [];
    if (models.length && !models.some((m: Json) => m.slug === model))
      setModel(models[0].slug);
  }, [catalog.data]);
  useEffect(() => {
    if (!tenant || !admin) return;
    const es = new EventSource(
      `/v1/operator/events?${query({ tenant_id: tenant })}`,
    );
    es.onopen = () => setConnected(true);
    es.onerror = () => setConnected(false);
    es.addEventListener("activity", (e) => {
      try {
        const v = JSON.parse((e as MessageEvent).data);
        setEvents((old) =>
          old.some((x) => x.seq === v.seq) ? old : [...old, v].slice(-500),
        );
        dirty.current = true;
      } catch {
        setConnected(false);
      }
    });
    const timer = setInterval(() => {
      if (dirty.current) {
        setRevision((v) => v + 1);
        dirty.current = false;
      }
    }, 1500);
    const fallback = setInterval(() => setRevision((v) => v + 1), 15000);
    return () => {
      es.close();
      clearInterval(timer);
      clearInterval(fallback);
    };
  }, [tenant, admin]);
  useEffect(() => {
    if (toast) {
      const timer = setTimeout(() => setToast(""), 4500);
      return () => clearTimeout(timer);
    }
  }, [toast]);
  const context = {
    tenant,
    tenantName,
    admin,
    revision,
    providers: providerData.data?.connections ?? [],
    models: catalog.data?.models ?? [],
    connection,
    model,
    setConnection,
    setModel,
    refreshProviders: () => setProviderRevision((v) => v + 1),
    refresh: () => setRevision((v) => v + 1),
    openKnowledge: (id: string, version?: number) =>
      setKnowledge({ id, version }),
    openTask: (id: string) => {
      setTask(id);
      setView("tasks");
    },
    notify: setToast,
  };
  const total =
    metrics.data?.roles?.reduce(
      (n: number, r: Json) => n + r.input_tokens + r.output_tokens,
      0,
    ) ?? 0;
  return (
    <Workspace.Provider value={context}>
      <div className="app-shell">
        <aside className="sidebar">
          <button
            className="brand"
            onClick={() => {
              setView("tasks");
              setTask("");
            }}
          >
            <span className="brand-mark small">E</span>
            <span>
              ECHELON
              <br />
              <small>NANO / OPERATIONS</small>
            </span>
          </button>
          <div className="workspace-picker">
            {tenantPicker}
            <button
              className="icon-button"
              aria-label="업무실 추가"
              onClick={newTenant}
            >
              ＋
            </button>
          </div>
          <nav aria-label="주 메뉴">
            {nav.map(([id, icon, en, ko]) => (
              <button
                key={id}
                aria-label={`${en} ${ko}`}
                aria-current={view === id ? "page" : undefined}
                className={view === id ? "selected" : ""}
                onClick={() => {
                  setView(id);
                  setKnowledge(undefined);
                }}
              >
                <span className="nav-icon">{icon}</span>
                <span>
                  {en}
                  <small>{ko}</small>
                </span>
                {view === id && <i />}
              </button>
            ))}
          </nav>
          <div className="sidebar-bottom">
            <div>
              <span className={`live-dot ${connected ? "live" : ""}`} />
              {connected ? "실시간 활동 연결됨" : "연결 대기"}
            </div>
            <small>
              LOCAL / PRIVATE WORKSPACE
              <br />
              Echelon Nano Beta
            </small>
          </div>
        </aside>
        <div className="main-column">
          <header className="topbar">
            <div className="mobile-workspace">
              {tenantPicker}
              <button
                className="icon-button"
                aria-label="업무실 추가"
                onClick={newTenant}
              >
                ＋
              </button>
            </div>
            <div className="breadcrumbs">
              {tenantName || "업무실을 선택해 주세요"} <span>/</span>{" "}
              {nav.find((n) => n[0] === view)?.[1]}{" "}
              {nav.find((n) => n[0] === view)?.[2]}
            </div>
            <div className="top-actions">
              <span className="usage">
                {formatNumber(total)} <small>관찰 tokens</small>
              </span>
              <button className="quiet" onClick={() => setView("settings")}>
                <span className={`live-dot ${connection ? "live" : ""}`} />
                {connection ? "모델 연결" : "계정 연결"}
              </button>
            </div>
          </header>
          <main className="content">
            {!tenant ? (
              <Empty title="업무실을 만들어 시작하세요">
                <button className="primary" onClick={newTenant}>
                  새 업무실
                </button>
              </Empty>
            ) : !admin ? (
              <Empty title="업무실을 준비하고 있습니다" />
            ) : (
              <>
                {view === "tasks" &&
                  (task ? (
                    <TaskDetail
                      key={task}
                      id={task}
                      onBack={() => setTask("")}
                    />
                  ) : (
                    <Tasks onNew={() => setCreate(true)} />
                  ))}
                {view === "central" && <Central />}
                {view === "protocols" && <Protocols />}
                {view === "dreaming" && <Dreaming />}
                {view === "vault" && <Vault />}
                {view === "settings" && <Settings />}
                <ErrorBox error={providerData.error || catalog.error} />
              </>
            )}
          </main>
        </div>
      </div>
      {toast && (
        <div className="toast" role="status">
          {toast}
        </div>
      )}
      {create && (
        <Modal title="새 Task" wide onClose={() => setCreate(false)}>
          <TaskForm
            onCreated={(id) => {
              setCreate(false);
              setTask(id);
            }}
          />
        </Modal>
      )}
      {knowledge && (
        <KnowledgeDetail
          key={`${knowledge.id}:${knowledge.version ?? "current"}`}
          id={knowledge.id}
          initialVersion={knowledge.version}
          onClose={() => setKnowledge(undefined)}
        />
      )}
    </Workspace.Provider>
  );
}
createRoot(document.getElementById("root")!).render(<App />);
