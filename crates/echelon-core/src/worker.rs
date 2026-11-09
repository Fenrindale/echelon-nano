//! Isolated file tools. No provider/DB/Vault credentials or host/Docker mounts enter this service.
use crate::{
    bootstrap::secret_hash,
    error::{AppError, AppResult},
    workspace::{nonempty, safe_name},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path as RpcPath, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};
use subtle::ConstantTimeEq;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InputFile {
    pub name: String,
    pub content: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: Uuid,
    pub script: String,
    #[serde(default)]
    pub files: Vec<InputFile>,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default = "timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub workspace_id: Option<Uuid>,
}
fn timeout() -> u64 {
    20
}
struct Worker {
    key: String,
    jobs: Mutex<BTreeMap<Uuid, Arc<Entry>>>,
    execution: Mutex<()>,
    identities: Mutex<BTreeMap<Uuid, u32>>,
    next_uid: AtomicU32,
}
struct Entry {
    uid: u32,
    hash: String,
    result: Mutex<Option<Value>>,
    done: Notify,
    cancel: Notify,
    cancelled: AtomicBool,
}
pub async fn serve() -> anyhow::Result<()> {
    #[cfg(not(unix))]
    anyhow::bail!("The isolated worker requires Linux containers");
    let key = tokio::fs::read_to_string("/root/.broker/token").await?;
    anyhow::ensure!(key.trim().len() >= 32, "worker credential missing");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions("/jobs", std::fs::Permissions::from_mode(0o711))?;
    }
    let state = Arc::new(Worker {
        key: secret_hash(key.trim()),
        jobs: Mutex::new(BTreeMap::new()),
        execution: Mutex::new(()),
        identities: Mutex::new(BTreeMap::new()),
        next_uid: AtomicU32::new(10002),
    });
    let router = Router::new()
        .route(
            "/healthz",
            get(|| async { Json(json!({"status":"ready"})) }),
        )
        .route("/jobs", post(run))
        .route("/jobs/start", post(start))
        .route("/jobs/{id}", get(poll))
        .route("/jobs/{id}/cancel", post(cancel))
        .route("/jobs/{id}/release", post(release))
        .route("/workspaces/{id}/close", post(close_workspace))
        // This RPC is private and authenticated. Source sizes are governed by
        // container resources, not a second, hidden transfer-size quota.
        .layer(DefaultBodyLimit::disable())
        .with_state(state);
    axum::serve(tokio::net::TcpListener::bind("0.0.0.0:9090").await?, router).await?;
    Ok(())
}
async fn run(
    State(s): State<Arc<Worker>>,
    h: HeaderMap,
    Json(job): Json<Job>,
) -> AppResult<Json<Value>> {
    authenticated(&s, &h)?;
    let (entry, replayed) = start_job(s, job.clone()).await?;
    loop {
        let ready = entry.done.notified();
        if let Some(mut result) = entry.result.lock().await.clone() {
            result["replayed"] = json!(replayed);
            return Ok(Json(result));
        }
        ready.await;
    }
}
fn authenticated(s: &Worker, h: &HeaderMap) -> AppResult<()> {
    let token = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(AppError::unauthorized)?;
    if !bool::from(secret_hash(token).as_bytes().ct_eq(s.key.as_bytes())) {
        return Err(AppError::unauthorized());
    }
    Ok(())
}
pub(crate) fn validate(job: &Job) -> AppResult<()> {
    if !nonempty(&job.script, 24000) {
        return Err(AppError::bad("worker_script_invalid"));
    }
    if !(1..=3600).contains(&job.timeout_seconds) {
        return Err(AppError::bad("worker_timeout_invalid"));
    }
    if job.files.iter().any(|f| !safe_name(&f.name)) {
        return Err(AppError::bad("worker_input_path_invalid"));
    }
    let names: std::collections::BTreeSet<_> = job.files.iter().map(|f| &f.name).collect();
    if names.len() != job.files.len() {
        return Err(AppError::bad("worker_duplicate_input_path"));
    }
    if job.outputs.len() > 16 {
        return Err(AppError::bad("worker_output_count_exceeded"));
    }
    if job.outputs.iter().any(|n| !safe_name(n)) {
        return Err(AppError::bad("worker_output_path_invalid"));
    }
    Ok(())
}
async fn start(
    State(s): State<Arc<Worker>>,
    h: HeaderMap,
    Json(job): Json<Job>,
) -> AppResult<Json<Value>> {
    authenticated(&s, &h)?;
    let id = job.id;
    let (entry, replayed) = start_job(s, job).await?;
    Ok(Json(snapshot(id, &entry, replayed).await))
}
async fn poll(
    State(s): State<Arc<Worker>>,
    h: HeaderMap,
    RpcPath(id): RpcPath<Uuid>,
) -> AppResult<Json<Value>> {
    authenticated(&s, &h)?;
    let entry = s
        .jobs
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| AppError::bad("worker_job_not_found"))?;
    Ok(Json(snapshot(id, &entry, true).await))
}
async fn cancel(
    State(s): State<Arc<Worker>>,
    h: HeaderMap,
    RpcPath(id): RpcPath<Uuid>,
) -> AppResult<Json<Value>> {
    authenticated(&s, &h)?;
    let entry = s
        .jobs
        .lock()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| AppError::bad("worker_job_not_found"))?;
    if entry.result.lock().await.is_none() {
        entry.cancelled.store(true, Ordering::SeqCst);
        entry.cancel.notify_one();
    }
    Ok(Json(snapshot(id, &entry, true).await))
}
async fn snapshot(id: Uuid, entry: &Entry, replayed: bool) -> Value {
    let mut result=entry.result.lock().await.clone().unwrap_or_else(||json!({"job_id":id,"status":if entry.cancelled.load(Ordering::SeqCst){"cancelling"}else{"running"}}));
    result["replayed"] = json!(replayed);
    result
}
async fn release(
    State(s): State<Arc<Worker>>,
    h: HeaderMap,
    RpcPath(id): RpcPath<Uuid>,
) -> AppResult<Json<Value>> {
    authenticated(&s, &h)?;
    let mut jobs = s.jobs.lock().await;
    if let Some(entry) = jobs.get(&id)
        && entry.result.lock().await.is_none()
    {
        return Err(AppError::bad("worker_job_still_running"));
    }
    jobs.remove(&id);
    Ok(Json(json!({"released":true})))
}
async fn close_workspace(
    State(s): State<Arc<Worker>>,
    h: HeaderMap,
    RpcPath(id): RpcPath<Uuid>,
) -> AppResult<Json<Value>> {
    authenticated(&s, &h)?;
    let _lease = s.execution.lock().await;
    let path = PathBuf::from("/jobs").join(id.to_string());
    // The authenticated core calls only after immutable Work Order completion.
    if path.exists() {
        tokio::fs::remove_dir_all(path).await.map_err(io_error)?;
    }
    s.identities.lock().await.remove(&id);
    Ok(Json(json!({"closed":true})))
}
async fn start_job(s: Arc<Worker>, job: Job) -> AppResult<(Arc<Entry>, bool)> {
    validate(&job)?;
    let mut cache = s.jobs.lock().await;
    let hash = crate::workspace::digest(
        &serde_json::to_vec(&job).map_err(|_| AppError::bad("invalid_worker_job"))?,
    );
    if let Some(entry) = cache.get(&job.id) {
        if entry.hash != hash {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "worker_idempotency_conflict",
            ));
        }
        return Ok((entry.clone(), true));
    }
    if cache.len() >= 256 {
        return Err(AppError::bad("worker_capacity_reached"));
    }
    let mut identities = s.identities.lock().await;
    let uid = *identities
        .entry(job.workspace_id.unwrap_or(job.id))
        .or_insert_with(|| s.next_uid.fetch_add(1, Ordering::SeqCst));
    drop(identities);
    let entry = Arc::new(Entry {
        uid,
        hash,
        result: Mutex::new(None),
        done: Notify::new(),
        cancel: Notify::new(),
        cancelled: AtomicBool::new(false),
    });
    cache.insert(job.id, entry.clone());
    drop(cache);
    let owned = entry.clone();
    tokio::spawn(async move {
        // One execution owns the sandbox UID through all descendant cleanup.
        let _lease = s.execution.lock().await;
        tracing::info!(event="worker_job_started",job_id=%job.id);
        let result = if owned.cancelled.load(Ordering::SeqCst) {
            Ok(json!({"job_id":job.id,"status":"cancelled","outputs":[]}))
        } else {
            execute(&job, &owned).await
        };
        let value = result.unwrap_or_else(
            |e| json!({"job_id":job.id,"status":"failed","error_code":e.code,"outputs":[]}),
        );
        *owned.result.lock().await = Some(value);
        owned.done.notify_waiters();
        tracing::info!(event="worker_job_completed",job_id=%job.id);
    });
    Ok((entry, false))
}
#[cfg(unix)]
fn chown(path: &Path, uid: u32) -> std::io::Result<()> {
    std::os::unix::fs::chown(path, Some(uid), Some(uid))
}
#[cfg(not(unix))]
fn chown(_: &Path, _: u32) -> std::io::Result<()> {
    Err(std::io::Error::other("Linux required"))
}
async fn kill_descendants(uid: u32) {
    let _=tokio::process::Command::new("/usr/bin/python3").args(["-c", "import os,glob,signal,sys\nfor p in glob.glob('/proc/[0-9]*'):\n try:\n  if os.stat(p).st_uid==int(sys.argv[1]): os.kill(int(p.rsplit('/',1)[1]),signal.SIGKILL)\n except (ProcessLookupError,FileNotFoundError): pass",&uid.to_string()]).env_clear().status().await;
}
fn io_error(error: std::io::Error) -> AppError {
    tracing::warn!(event="worker_io_failed",kind=?error.kind(),os_code=error.raw_os_error());
    if error.raw_os_error() == Some(28) {
        return AppError::bad("worker_storage_exhausted");
    }
    AppError::internal("worker_io_failed")
}
async fn execute(job: &Job, entry: &Entry) -> AppResult<Value> {
    let root = PathBuf::from("/jobs").join(job.workspace_id.unwrap_or(job.id).to_string());
    tokio::fs::create_dir_all(&root).await.map_err(io_error)?;
    let result = execute_in(job, &root, entry).await;
    kill_descendants(entry.uid).await;
    // root is built only from a parsed UUID under the dedicated tmpfs, never a caller path.
    if job.workspace_id.is_none() {
        let _ = tokio::fs::remove_dir_all(&root).await;
    }
    result
}
async fn execute_in(job: &Job, root: &Path, entry: &Entry) -> AppResult<Value> {
    use std::io::{Read, Seek};
    // CHOWN is available to this supervisor; FOWNER deliberately is not. Set
    // directory mode as its owner before handing it to the sandbox identity.
    chown(root, 0).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700)).map_err(io_error)?;
    }
    chown(root, entry.uid).map_err(io_error)?;
    for file in &job.files {
        let path = root.join(&file.name);
        // Never follow a child-created symlink while hydrating the next command.
        if std::fs::symlink_metadata(&path).is_ok() {
            continue;
        }
        let parent = path
            .parent()
            .ok_or_else(|| AppError::bad("invalid_worker_path"))?;
        let mut cursor = root.to_path_buf();
        for part in parent
            .strip_prefix(root)
            .map_err(|_| AppError::bad("invalid_worker_path"))?
            .components()
        {
            cursor.push(part);
            match std::fs::symlink_metadata(&cursor) {
                Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
                Ok(_) => return Err(AppError::bad("invalid_worker_path")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::create_dir(&cursor).map_err(io_error)?
                }
                Err(e) => return Err(io_error(e)),
            }
        }
        tokio::fs::write(&path, &file.content)
            .await
            .map_err(io_error)?;
        chown(&path, entry.uid).map_err(io_error)?;
        for ancestor in parent.ancestors().take_while(|p| p.starts_with(root)) {
            chown(ancestor, entry.uid).map_err(io_error)?;
        }
    }
    // Keep open handles: a child replacing a pathname cannot redirect privileged reads.
    let stdout = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(root.join(format!(".stdout-{}", job.id)))
        .map_err(io_error)?;
    let stderr = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(root.join(format!(".stderr-{}", job.id)))
        .map_err(io_error)?;
    let wrapper = include_str!("worker_guard.py");
    let mut command = tokio::process::Command::new("/usr/bin/python3");
    command
        .args(["-I", "-c", wrapper, &job.script])
        .current_dir(root)
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(stdout.try_clone().map_err(io_error)?)
        .stderr(stderr.try_clone().map_err(io_error)?)
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        command.uid(entry.uid).gid(entry.uid);
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(io_error)?;
    let status = tokio::select! {s=tokio::time::timeout(
        std::time::Duration::from_secs(job.timeout_seconds),
        child.wait(),
    )=>Some(s), _=entry.cancel.notified()=>None};
    kill_descendants(entry.uid).await;
    let (exit, timed_out) = match status {
        Some(Ok(Ok(s))) => (s.code(), false),
        Some(Ok(Err(e))) => return Err(io_error(e)),
        Some(Err(_)) | None => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            (None, !entry.cancelled.load(Ordering::SeqCst))
        }
    };
    let read_handle = |mut file: std::fs::File| -> AppResult<String> {
        file.rewind().map_err(io_error)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(io_error)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    let mut outputs = vec![];
    let canonical = std::fs::canonicalize(root).map_err(io_error)?;
    for name in &job.outputs {
        let path = root.join(name);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(AppError::bad("worker_output_invalid"));
        }
        let resolved = std::fs::canonicalize(&path).map_err(io_error)?;
        if !resolved.starts_with(&canonical) {
            return Err(AppError::forbidden("worker_output_scope_denied"));
        }
        let bytes = std::fs::read(&resolved).map_err(io_error)?;
        outputs.push(json!({"name":name,"content":String::from_utf8(bytes).map_err(|_|AppError::bad("worker_output_not_utf8"))?}));
    }
    Ok(
        json!({"job_id":job.id,"status":if entry.cancelled.load(Ordering::SeqCst){"cancelled"}else{"completed"},"exit_code":exit,"timed_out":timed_out,"stdout":read_handle(stdout)?,"stderr":read_handle(stderr)?,"outputs":outputs,"execution":{"script":job.script.chars().take(1024).collect::<String>(),"script_sha256":crate::workspace::digest(job.script.as_bytes()),"script_truncated":job.script.chars().count()>1024,"outputs":job.outputs,"timeout_seconds":job.timeout_seconds,"persistent_workspace":job.workspace_id.is_some()}}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_paths_and_execution_contract_without_a_file_size_quota() {
        let mut job = Job {
            id: Uuid::new_v4(),
            script: "true".into(),
            files: vec![InputFile {
                name: "large.json".into(),
                content: "x".repeat(2_000_000),
            }],
            outputs: vec!["large-result.json".into()],
            timeout_seconds: 30,
            workspace_id: None,
        };
        validate(&job).unwrap();
        job.files.push(job.files[0].clone());
        assert_eq!(
            validate(&job).unwrap_err().code,
            "worker_duplicate_input_path"
        );
        job.files.pop();
        job.files[0].name = "../foreign".into();
        assert_eq!(
            validate(&job).unwrap_err().code,
            "worker_input_path_invalid"
        );
        job.files.clear();
        job.timeout_seconds = 0;
        assert_eq!(validate(&job).unwrap_err().code, "worker_timeout_invalid");
    }
}
