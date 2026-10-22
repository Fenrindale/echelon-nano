import { useState } from "react";
import { api, query } from "./api";
import { Badge, ErrorBox, Field, Modal, useAsync, type Json } from "./components";
import { useWorkspace } from "./context";

type Binding = { name: string; bundle_id: string; variable: string };
const blank = () => ({ transport: "npm", package: "", package_version: "", executable: "", args: [] as string[], url: "", allow_private: false, allow_http: false, environment: [] as Binding[] });
export function Mcp() {
  const w = useWorkspace();
  const servers = useAsync(() => api(`/v1/operator/mcp?${query({ tenant_id: w.tenant })}`), [w.tenant, w.revision]);
  const credentials = useAsync(() => api(`/v1/operator/credentials?${query({ tenant_id: w.tenant })}`), [w.tenant, w.revision]);
  const [editing, setEditing] = useState<Json | null>(null);
  const [name, setName] = useState("");
  const [spec, setSpec] = useState(blank());
  const [args, setArgs] = useState("");
  const [enabled, setEnabled] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<unknown>(null);
  const [detail, setDetail] = useState<Json | null>(null);
  function open(server?: Json) {
    setEditing(server ?? {}); setName(server?.name ?? "");
    setSpec(server?.spec ?? blank()); setArgs((server?.spec?.args ?? []).join("\n"));
    setEnabled(server?.enabled ?? true); setError(null);
  }
  async function install(server: Json) {
    await api(`/v1/operator/mcp/${server.id}/install`, "POST", { tenant_id: w.tenant, expected_version: server.version });
  }
  async function save(event: React.FormEvent) {
    event.preventDefault(); setBusy(true); setError(null);
    try {
      const configured = await api("/v1/operator/mcp", "POST", { tenant_id: w.tenant, id: editing?.id ?? null, expected_version: editing?.version ?? null, name, enabled, spec: { ...spec, args: args.split("\n").filter(Boolean) } });
      // Keep the saved identity after a failed install, so retry cannot create a duplicate.
      setEditing({ ...configured, name, spec, enabled });
      w.refresh();
      if (enabled) await install(configured);
      setEditing(null); w.refresh(); w.notify(enabled ? "MCP 설치와 도구 확인 완료" : "MCP 설정 저장 완료");
    } catch (e) { setError(e); w.refresh(); } finally { setBusy(false); }
  }
  function binding(index: number, change: Partial<Binding>) {
    setSpec({ ...spec, environment: spec.environment.map((b, i) => i === index ? { ...b, ...change } : b) });
  }
  return <>
    <div className="section-head"><div><h1>MCP</h1><p className="muted">공통 도구를 설치하면 Handler가 업무에 맞게 선택합니다.</p></div><button onClick={() => open()}>MCP 추가</button></div>
    <ErrorBox error={servers.error} />
    {!servers.data?.servers?.length && <div className="empty"><h3>설치된 MCP가 없습니다</h3><p>npm·Python 패키지 또는 원격 MCP 주소를 추가하세요.</p></div>}
    <div className="settings-grid">{servers.data?.servers?.map((s: Json) => <section className="panel" key={s.id}>
      <div className="section-head"><h2>{s.name}</h2><Badge value={s.enabled ? s.status : "paused"} /></div>
      <p>{s.spec.transport === "http" ? s.spec.url : `${s.spec.package} @ ${s.spec.package_version}`}</p>
      <p className="muted">버전 {s.version} · 도구 {s.tools.length}개 · {s.enabled ? s.status === "ready" ? "업무에서 사용 가능" : "연결 확인 필요" : "사용 중지"}</p>
      {s.error_code && <ErrorBox error={s.error_code} />}
      <div className="actions"><button className="quiet" onClick={() => open(s)}>설정</button><button className="quiet" onClick={() => setDetail(s)}>도구 보기</button></div>
    </section>)}</div>
    {editing && <Modal title={editing.id ? "MCP 설정" : "MCP 추가"} onClose={() => { if (!busy) setEditing(null); }} wide>
      <form onSubmit={save}>
        <Field label="이름"><input required value={name} onChange={e => setName(e.target.value)} /></Field>
        <Field label="연결 방식"><select value={spec.transport} onChange={e => setSpec({ ...blank(), transport: e.target.value, environment: spec.environment })}><option value="npm">npm 패키지 · stdio</option><option value="python">Python 패키지 · stdio</option><option value="http">원격 MCP · HTTP</option></select></Field>
        {spec.transport === "http" ? <>
          <Field label="MCP 주소"><input required type="url" value={spec.url} onChange={e => setSpec({ ...spec, url: e.target.value })} /></Field>
          <label><input type="checkbox" checked={spec.allow_private} onChange={e => setSpec({ ...spec, allow_private: e.target.checked })} /> 내부망 주소</label>
          <label><input type="checkbox" checked={spec.allow_http} onChange={e => setSpec({ ...spec, allow_http: e.target.checked })} /> 평문 HTTP 사용</label>
        </> : <>
          <Field label="패키지 이름"><input required value={spec.package} onChange={e => setSpec({ ...spec, package: e.target.value })} placeholder={spec.transport === "npm" ? "@organization/mcp-server" : "package-name"} /></Field>
          <Field label="설치 버전" hint="다음 업무에도 같은 버전을 사용합니다."><input required value={spec.package_version} onChange={e => setSpec({ ...spec, package_version: e.target.value })} placeholder="1.2.3" /></Field>
          <Field label={spec.transport === "npm" ? "실행 파일 이름" : "Python 모듈 이름"}><input required value={spec.executable} onChange={e => setSpec({ ...spec, executable: e.target.value })} /></Field>
          <Field label="실행 인자" hint="한 줄에 한 인자. 비밀번호·토큰은 아래 Vault 참조로 연결하세요."><textarea value={args} onChange={e => setArgs(e.target.value)} /></Field>
        </>}
        <h3>Vault 연결</h3><p className="muted">필요한 변수만 MCP에 전달합니다. 원격 Bearer 인증은 변수 이름을 MCP_AUTH_TOKEN으로 지정하세요.</p>
        {spec.environment.map((b, i) => <div className="panel" key={i}>
          <Field label="MCP 환경변수 이름"><input required value={b.name} onChange={e => binding(i, { name: e.target.value })} /></Field>
          <Field label="Vault 번들"><select required value={b.bundle_id} onChange={e => binding(i, { bundle_id: e.target.value, variable: "" })}><option value="">선택</option>{credentials.data?.bundles?.map((v: Json) => <option key={v.id} value={v.id}>{v.name}</option>)}</select></Field>
          <Field label="저장된 변수"><select required value={b.variable} onChange={e => binding(i, { variable: e.target.value })}><option value="">선택</option>{credentials.data?.bundles?.find((v: Json) => v.id === b.bundle_id)?.entry_names?.map((v: string) => <option key={v}>{v}</option>)}</select></Field>
          <button type="button" className="quiet" onClick={() => setSpec({ ...spec, environment: spec.environment.filter((_, n) => n !== i) })}>연결 제거</button>
        </div>)}
        <button type="button" className="quiet" onClick={() => setSpec({ ...spec, environment: [...spec.environment, { name: "", bundle_id: "", variable: "" }] })}>변수 연결 추가</button>
        <p><label><input type="checkbox" checked={enabled} onChange={e => setEnabled(e.target.checked)} /> 업무에서 사용</label></p>
        <ErrorBox error={error} /><button disabled={busy}>{busy ? "설치·연결 확인 중…" : enabled ? "설치하고 연결" : "설정 저장"}</button>
      </form>
    </Modal>}
    {detail && <Modal title={`${detail.name} 도구`} onClose={() => setDetail(null)} wide>{detail.tools.length ? detail.tools.map((t: Json) => <section key={t.name}><h3>{t.name}</h3><p>{t.description}</p><details><summary>입력 형식</summary><pre>{JSON.stringify(t.inputSchema, null, 2)}</pre></details></section>) : <p>연결 확인 후 도구 목록이 표시됩니다.</p>}</Modal>}
  </>;
}
