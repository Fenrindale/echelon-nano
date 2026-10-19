//! Isolated file tools. No provider/DB/Vault credentials or host/Docker mounts enter this service.
use crate::{
    bootstrap::secret_hash,
    error::{AppError, AppResult},
    workspace::{nonempty, safe_name},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use subtle::ConstantTimeEq;
use tokio::sync::Mutex;
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
}
fn timeout() -> u64 {
    20
}
struct Worker {
    key: String,
    jobs: Mutex<BTreeMap<Uuid, (String, Value)>>,
}
pub async fn serve() -> anyhow::Result<()> {
    #[cfg(not(unix))]
    anyhow::bail!("The isolated worker requires Linux containers");
    let key = tokio::fs::read_to_string("/root/.broker/token").await?;
    anyhow::ensure!(key.trim().len() >= 32, "worker credential missing");
    let state = Arc::new(Worker {
        key: secret_hash(key.trim()),
        jobs: Mutex::new(BTreeMap::new()),
    });
    let router = Router::new()
        .route(
            "/healthz",
            get(|| async { Json(json!({"status":"ready"})) }),
        )
        .route("/jobs", post(run))
        .layer(DefaultBodyLimit::max(524288))
        .with_state(state);
    axum::serve(tokio::net::TcpListener::bind("0.0.0.0:9090").await?, router).await?;
    Ok(())
}
async fn run(
    State(s): State<Arc<Worker>>,
    h: HeaderMap,
    Json(job): Json<Job>,
) -> AppResult<Json<Value>> {
    let token = h
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(AppError::unauthorized)?;
    if !bool::from(secret_hash(token).as_bytes().ct_eq(s.key.as_bytes())) {
        return Err(AppError::unauthorized());
    }
    if !nonempty(&job.script, 24000)
        || job.files.len() > 16
        || job.outputs.len() > 16
        || job
            .files
            .iter()
            .any(|f| !safe_name(&f.name) || f.content.len() > 131072)
        || job.outputs.iter().any(|n| !safe_name(n))
        || !(1..=30).contains(&job.timeout_seconds)
    {
        return Err(AppError::bad("invalid_worker_job"));
    }
    // Own execution independently of the HTTP response. A disconnected core
    // must not drop the supervisor, release the UID lock or lose its receipt.
    // Dropping this JoinHandle detaches the task; an explicit retry waits for
    // the same serialized job and receives its cached outcome.
    tokio::spawn(async move { run_cached(s, job).await })
        .await
        .map_err(|_| AppError::internal("worker_supervisor_failed"))?
        .map(Json)
}
async fn run_cached(s: Arc<Worker>, job: Job) -> AppResult<Value> {
    // One job owns the unprivileged UID at a time, including descendant cleanup.
    let mut cache = s.jobs.lock().await;
    let hash = crate::workspace::digest(
        &serde_json::to_vec(&job).map_err(|_| AppError::bad("invalid_worker_job"))?,
    );
    if let Some((old, result)) = cache.get(&job.id) {
        if old != &hash {
            return Err(AppError::new(
                StatusCode::CONFLICT,
                "worker_idempotency_conflict",
            ));
        }
        let mut result = result.clone();
        result["replayed"] = json!(true);
        return Ok(result);
    }
    tracing::info!(event="worker_job_started",job_id=%job.id);
    let mut result = execute(&job).await?;
    result["replayed"] = json!(false);
    tracing::info!(event="worker_job_completed",job_id=%job.id);
    if cache.len() >= 128 {
        cache.clear();
    }
    cache.insert(job.id, (hash, result.clone()));
    Ok(result)
}
#[cfg(unix)]
fn chown(path: &Path) -> std::io::Result<()> {
    std::os::unix::fs::chown(path, Some(10002), Some(10002))
}
#[cfg(not(unix))]
fn chown(_: &Path) -> std::io::Result<()> {
    Err(std::io::Error::other("Linux required"))
}
async fn kill_descendants() {
    let _=tokio::process::Command::new("/usr/bin/python3").args(["-c", "import os,glob,signal\nfor p in glob.glob('/proc/[0-9]*'):\n try:\n  if os.stat(p).st_uid==10002: os.kill(int(p.rsplit('/',1)[1]),signal.SIGKILL)\n except (ProcessLookupError,FileNotFoundError): pass"]).env_clear().status().await;
}
fn io_error(error: std::io::Error) -> AppError {
    tracing::warn!(event="worker_io_failed",kind=?error.kind(),os_code=error.raw_os_error());
    AppError::internal("worker_io_failed")
}
async fn execute(job: &Job) -> AppResult<Value> {
    let root = PathBuf::from("/jobs").join(job.id.to_string());
    tokio::fs::create_dir(&root).await.map_err(io_error)?;
    let result = execute_in(job, &root).await;
    kill_descendants().await;
    // root is built only from a parsed UUID under the dedicated tmpfs, never a caller path.
    let _ = tokio::fs::remove_dir_all(&root).await;
    result
}
async fn execute_in(job: &Job, root: &Path) -> AppResult<Value> {
    use std::io::{Read, Seek};
    for file in &job.files {
        let path = root.join(&file.name);
        let parent = path
            .parent()
            .ok_or_else(|| AppError::bad("invalid_worker_path"))?;
        tokio::fs::create_dir_all(parent).await.map_err(io_error)?;
        tokio::fs::write(&path, &file.content)
            .await
            .map_err(io_error)?;
        chown(&path).map_err(io_error)?;
        for ancestor in parent.ancestors().take_while(|p| p.starts_with(root)) {
            chown(ancestor).map_err(io_error)?;
        }
    }
    chown(root).map_err(io_error)?;
    // Keep open handles: a child replacing a pathname cannot redirect privileged reads.
    let stdout = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(root.join(".stdout"))
        .map_err(io_error)?;
    let stderr = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(root.join(".stderr"))
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
        command.uid(10002).gid(10002);
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(io_error)?;
    let status = tokio::time::timeout(
        std::time::Duration::from_secs(job.timeout_seconds),
        child.wait(),
    )
    .await;
    kill_descendants().await;
    let (exit, timed_out) = match status {
        Ok(Ok(s)) => (s.code(), false),
        Ok(Err(e)) => return Err(io_error(e)),
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            (None, true)
        }
    };
    let read_handle = |mut file: std::fs::File| -> AppResult<String> {
        file.rewind().map_err(io_error)?;
        let mut bytes = Vec::new();
        file.take(65536).read_to_end(&mut bytes).map_err(io_error)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    };
    let mut outputs = vec![];
    let canonical = std::fs::canonicalize(root).map_err(io_error)?;
    let mut total = 0;
    for name in &job.outputs {
        let path = root.join(name);
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 131072 {
            return Err(AppError::bad("worker_output_invalid"));
        }
        let resolved = std::fs::canonicalize(&path).map_err(io_error)?;
        if !resolved.starts_with(&canonical) {
            return Err(AppError::forbidden("worker_output_scope_denied"));
        }
        let bytes = std::fs::read(&resolved).map_err(io_error)?;
        total += bytes.len();
        if total > 524288 {
            return Err(AppError::bad("worker_output_budget_exceeded"));
        }
        outputs.push(json!({"name":name,"content":String::from_utf8(bytes).map_err(|_|AppError::bad("worker_output_not_utf8"))?}));
    }
    Ok(
        json!({"job_id":job.id,"exit_code":exit,"timed_out":timed_out,"stdout":read_handle(stdout)?,"stderr":read_handle(stderr)?,"outputs":outputs}),
    )
}
