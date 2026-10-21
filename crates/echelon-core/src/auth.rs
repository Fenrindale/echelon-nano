use crate::{
    bootstrap::{random_secret, secret_hash},
    error::{AppError, AppResult},
    store::{VaultStore, audit},
    unix_time,
};
use axum::http::StatusCode;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::StreamExt;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use std::{collections::HashMap, sync::Arc, time::Duration};
use subtle::ConstantTimeEq;
use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const JWKS: &str = "https://auth.openai.com/.well-known/jwks.json";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

#[derive(Serialize, Deserialize)]
pub struct CredentialRecord {
    pub issuer: String,
    pub subject: String,
    pub client_id: String,
    pub host_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: String,
    pub scopes: Vec<String>,
    pub expires_at: u64,
    pub generation: u64,
}

impl CredentialRecord {
    pub fn plan_enabled(&self) -> bool {
        self.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct")
    }
}

#[derive(Deserialize)]
pub struct TokenReply {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    token_type: String,
    expires_in: u64,
    scope: Option<String>,
}

#[derive(Deserialize)]
pub struct IdClaims {
    pub sub: String,
    pub iss: String,
    pub nonce: Option<String>,
}

pub fn verify_id_token(
    jwks: &JwkSet,
    token: &str,
    client_id: &str,
    nonce: Option<&str>,
    subject: Option<&str>,
) -> AppResult<IdClaims> {
    let header =
        jsonwebtoken::decode_header(token).map_err(|_| AppError::bad("invalid_id_token"))?;
    if header.alg != Algorithm::RS256 {
        return Err(AppError::bad("invalid_id_algorithm"));
    }
    let kid = header.kid.ok_or_else(|| AppError::bad("id_key_missing"))?;
    let jwk = jwks
        .find(&kid)
        .ok_or_else(|| AppError::bad("id_key_unknown"))?;
    let key = DecodingKey::from_jwk(jwk).map_err(|_| AppError::bad("id_key_invalid"))?;
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[client_id]);
    validation.set_required_spec_claims(&["iss", "sub", "aud", "exp"]);
    validation.leeway = 15;
    let claims = jsonwebtoken::decode::<IdClaims>(token, &key, &validation)
        .map_err(|_| AppError::bad("id_validation_failed"))?
        .claims;
    if claims.iss != ISSUER || claims.sub.is_empty() {
        return Err(AppError::bad("id_validation_failed"));
    }
    if let Some(expected) = nonce {
        let actual = claims
            .nonce
            .as_deref()
            .ok_or_else(|| AppError::bad("id_nonce_missing"))?;
        if !bool::from(actual.as_bytes().ct_eq(expected.as_bytes())) {
            return Err(AppError::bad("id_nonce_mismatch"));
        }
    }
    if subject.is_some_and(|expected| claims.sub != expected) {
        return Err(AppError::bad("account_mismatch"));
    }
    Ok(claims)
}

pub fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn merge_refresh(
    old: CredentialRecord,
    next: TokenReply,
    now: u64,
) -> AppResult<CredentialRecord> {
    if !next.token_type.eq_ignore_ascii_case("bearer")
        || next.access_token.is_empty()
        || next.expires_in == 0
    {
        return Err(AppError::bad("invalid_token_reply"));
    }
    Ok(CredentialRecord {
        access_token: next.access_token,
        refresh_token: next.refresh_token.or(old.refresh_token),
        id_token: next.id_token.unwrap_or(old.id_token),
        scopes: next
            .scope
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or(old.scopes),
        expires_at: now
            .checked_add(next.expires_in)
            .ok_or_else(|| AppError::bad("invalid_token_expiry"))?,
        generation: old.generation + 1,
        ..old
    })
}

struct Pending {
    id: Uuid,
    state: String,
    nonce: String,
    verifier: String,
    client_id: Option<String>,
    expected_subject: Option<String>,
    expires: u64,
}

#[derive(Clone, Serialize)]
pub struct AttemptStatus {
    pub status: String,
    pub connection_id: Option<String>,
    pub error: Option<String>,
    #[serde(skip)]
    retained_client: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartLogin {
    pub connection_id: Option<String>,
    pub retry_attempt_id: Option<Uuid>,
}

#[derive(Serialize)]
pub struct LoginStart {
    pub attempt_id: Uuid,
    pub authorize_url: String,
    pub expires_in: u64,
}

#[derive(Deserialize)]
pub struct Callback {
    pub state: String,
    pub code: Option<String>,
    pub client_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct OAuthService {
    client: Client,
    inference_client: Client,
    vault: VaultStore,
    pool: PgPool,
    host_id: String,
    redirect_uri: String,
    pending: Arc<Mutex<HashMap<String, Pending>>>,
    statuses: Arc<Mutex<HashMap<Uuid, AttemptStatus>>>,
    jwks: Arc<RwLock<Option<(u64, JwkSet)>>>,
}

impl OAuthService {
    pub fn new(
        vault: VaultStore,
        pool: PgPool,
        host_id: String,
        redirect_uri: String,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(30))
                .timeout(Duration::from_secs(120))
                .build()?,
            inference_client: crate::persistence::inference_client()?,
            vault,
            pool,
            host_id,
            redirect_uri,
            pending: Arc::new(Mutex::new(HashMap::new())),
            statuses: Arc::new(Mutex::new(HashMap::new())),
            jwks: Arc::new(RwLock::new(None)),
        })
    }

    pub async fn start(&self, req: StartLogin) -> AppResult<LoginStart> {
        if req.connection_id.is_some() && req.retry_attempt_id.is_some() {
            return Err(AppError::bad("choose_one_registration"));
        }
        let (client_id, subject) = if let Some(id) = req.connection_id {
            let record = self.read_record(&id).await?.value;
            (Some(record.client_id), Some(record.subject))
        } else if let Some(id) = req.retry_attempt_id {
            let statuses = self.statuses.lock().await;
            let client = statuses
                .get(&id)
                .and_then(|s| s.retained_client.clone())
                .ok_or_else(|| AppError::bad("registration_unavailable"))?;
            (Some(client), None)
        } else {
            (None, None)
        };
        let id = Uuid::new_v4();
        let state = random_secret();
        let nonce = random_secret();
        let verifier = random_secret();
        let mut url =
            url::Url::parse(AUTHORIZE).map_err(|_| AppError::internal("authorize_url_invalid"))?;
        {
            let mut q = url.query_pairs_mut();
            q.append_pair(
                "client_id",
                client_id.as_deref().unwrap_or("dynamic_agent_client"),
            )
            .append_pair("ext_agent_host_id", &self.host_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &self.redirect_uri)
            .append_pair("scope", SCOPES)
            .append_pair("resource", RESOURCE)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &pkce_challenge(&verifier));
            if client_id.is_none() {
                q.append_pair("agent_name_hint", "Echelon Nano");
            }
        }
        let mut pending = self.pending.lock().await;
        pending.retain(|_, p| p.expires > unix_time());
        if pending.len() >= 20 {
            return Err(AppError::new(
                StatusCode::TOO_MANY_REQUESTS,
                "too_many_attempts",
            ));
        }
        pending.insert(
            secret_hash(&state),
            Pending {
                id,
                state,
                nonce,
                verifier,
                client_id,
                expected_subject: subject,
                expires: unix_time() + 600,
            },
        );
        let mut statuses = self.statuses.lock().await;
        if statuses.len() > 100 {
            statuses.clear();
        }
        statuses.insert(
            id,
            AttemptStatus {
                status: "pending".into(),
                connection_id: None,
                error: None,
                retained_client: None,
            },
        );
        Ok(LoginStart {
            attempt_id: id,
            authorize_url: url.into(),
            expires_in: 600,
        })
    }

    pub async fn attempt(&self, id: Uuid) -> AppResult<AttemptStatus> {
        self.statuses
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "attempt_unknown"))
    }

    async fn fetch_jwks(&self, force: bool) -> AppResult<JwkSet> {
        if !force
            && let Some((expires, keys)) = &*self.jwks.read().await
            && *expires > unix_time()
        {
            return Ok(keys.clone());
        }
        let response = self
            .client
            .get(JWKS)
            .timeout(Duration::from_secs(20))
            .send()
            .await
            .map_err(|_| AppError::internal("jwks_unavailable"))?;
        if !response.status().is_success() {
            return Err(AppError::internal("jwks_unavailable"));
        }
        let keys: JwkSet = response
            .json()
            .await
            .map_err(|_| AppError::internal("jwks_invalid"))?;
        *self.jwks.write().await = Some((unix_time() + 3600, keys.clone()));
        Ok(keys)
    }

    async fn validate_id(
        &self,
        token: &str,
        client: &str,
        nonce: Option<&str>,
        subject: Option<&str>,
    ) -> AppResult<IdClaims> {
        let keys = self.fetch_jwks(false).await?;
        match verify_id_token(&keys, token, client, nonce, subject) {
            Err(e) if e.code == "id_key_unknown" => {
                verify_id_token(&self.fetch_jwks(true).await?, token, client, nonce, subject)
            }
            result => result,
        }
    }

    async fn exchange(&self, form: &[(&str, &str)]) -> AppResult<TokenReply> {
        let response = self
            .client
            .post(TOKEN)
            .timeout(Duration::from_secs(30))
            .form(form)
            .send()
            .await
            .map_err(|_| AppError::internal("token_endpoint_unavailable"))?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                400 => AppError::bad("oauth_grant_rejected"),
                401 | 403 => AppError::forbidden("oauth_access_denied"),
                429 => AppError::new(StatusCode::TOO_MANY_REQUESTS, "provider_limit"),
                _ => AppError::internal("token_endpoint_failed"),
            });
        }
        response
            .json()
            .await
            .map_err(|_| AppError::bad("invalid_token_reply"))
    }

    pub async fn callback(&self, callback: Callback) -> AppResult<String> {
        let attempt = self
            .pending
            .lock()
            .await
            .remove(&secret_hash(&callback.state))
            .ok_or_else(|| AppError::bad("state_unknown_or_consumed"))?;
        if !bool::from(attempt.state.as_bytes().ct_eq(callback.state.as_bytes()))
            || attempt.expires <= unix_time()
        {
            return Err(AppError::bad("state_invalid_or_expired"));
        }
        let id = attempt.id;
        let result = self.finish_callback(attempt, callback).await;
        let mut statuses = self.statuses.lock().await;
        if let Some(status) = statuses.get_mut(&id) {
            match &result {
                Ok(connection) => {
                    status.status = "connected".into();
                    status.connection_id = Some(connection.clone());
                }
                Err(error) => {
                    status.status = "failed".into();
                    status.error = Some(error.code.into());
                }
            }
        }
        result
    }

    async fn finish_callback(&self, attempt: Pending, callback: Callback) -> AppResult<String> {
        if callback.error.is_some() {
            return Err(AppError::forbidden("oauth_consent_denied"));
        }
        let client = match (&attempt.client_id, callback.client_id) {
            (Some(expected), Some(actual)) if expected != &actual => {
                return Err(AppError::bad("client_mismatch"));
            }
            (Some(expected), _) => expected.clone(),
            (None, Some(issued))
                if issued != "dynamic_agent_client"
                    && !issued.is_empty()
                    && issued.len() <= 256 =>
            {
                issued
            }
            _ => return Err(AppError::bad("issued_client_missing")),
        };
        if let Some(status) = self.statuses.lock().await.get_mut(&attempt.id) {
            status.retained_client = Some(client.clone());
        }
        let code = callback
            .code
            .ok_or_else(|| AppError::bad("authorization_code_missing"))?;
        let tokens = self
            .exchange(&[
                ("grant_type", "authorization_code"),
                ("client_id", &client),
                ("code", &code),
                ("code_verifier", &attempt.verifier),
                ("redirect_uri", &self.redirect_uri),
                ("resource", RESOURCE),
            ])
            .await?;
        if !tokens.token_type.eq_ignore_ascii_case("bearer")
            || tokens.expires_in == 0
            || tokens.access_token.is_empty()
        {
            return Err(AppError::bad("invalid_token_reply"));
        }
        let id_token = tokens
            .id_token
            .ok_or_else(|| AppError::bad("id_token_missing"))?;
        let identity = self
            .validate_id(
                &id_token,
                &client,
                Some(&attempt.nonce),
                attempt.expected_subject.as_deref(),
            )
            .await?;
        let connection = secret_hash(&format!("{}|{}|{}", identity.iss, identity.sub, client));
        let path = format!("oauth/{connection}");
        let mut registration_lock = self
            .pool
            .begin()
            .await
            .map_err(|_| AppError::internal("database_unavailable"))?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(&connection)
            .execute(&mut *registration_lock)
            .await
            .map_err(|_| AppError::internal("registration_lock_failed"))?;
        let old = self.vault.get::<CredentialRecord>(&path).await?;
        let generation = old.as_ref().map_or(0, |r| r.value.generation + 1);
        let record = CredentialRecord {
            issuer: identity.iss,
            subject: identity.sub,
            client_id: client,
            host_id: self.host_id.clone(),
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            id_token,
            scopes: tokens
                .scope
                .unwrap_or_default()
                .split_whitespace()
                .map(String::from)
                .collect(),
            expires_at: unix_time()
                .checked_add(tokens.expires_in)
                .ok_or_else(|| AppError::bad("invalid_token_expiry"))?,
            generation,
        };
        let version = self
            .vault
            .put(&path, &record, old.map_or(0, |r| r.version))
            .await?;
        self.save_metadata(&connection, &record, version).await?;
        audit(
            &self.pool,
            None,
            None,
            "operator",
            "oauth.login",
            "connected",
            json!({"connection_id":connection,"plan_enabled":record.plan_enabled()}),
        )
        .await?;
        registration_lock
            .commit()
            .await
            .map_err(|_| AppError::internal("registration_commit_failed"))?;
        Ok(connection)
    }

    async fn read_record(&self, id: &str) -> AppResult<crate::store::Versioned<CredentialRecord>> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(AppError::bad("invalid_connection_id"));
        }
        self.vault
            .get(&format!("oauth/{id}"))
            .await?
            .ok_or_else(|| AppError::new(StatusCode::NOT_FOUND, "connection_unknown"))
    }

    async fn save_metadata(
        &self,
        id: &str,
        record: &CredentialRecord,
        version: u64,
    ) -> AppResult<()> {
        sqlx::query("INSERT INTO oauth_connections(id,credential_version,generation,expires_at,plan_enabled) VALUES($1,$2,$3,$4,$5) ON CONFLICT(id) DO UPDATE SET credential_version=$2,generation=$3,expires_at=$4,plan_enabled=$5,status='connected',updated_at=now()")
            .bind(id).bind(version as i64).bind(record.generation as i64)
            .bind(record.expires_at as i64).bind(record.plan_enabled()).execute(&self.pool).await
            .map_err(|_|AppError::internal("connection_metadata_failed"))?;
        Ok(())
    }

    pub async fn connections(&self) -> AppResult<Value> {
        let rows=sqlx::query("SELECT id,generation,expires_at,plan_enabled,status FROM oauth_connections ORDER BY created_at")
            .fetch_all(&self.pool).await.map_err(|_|AppError::internal("database_unavailable"))?;
        Ok(json!({"connections":rows.into_iter().map(|row|json!({
            "id":row.get::<String,_>("id"),"generation":row.get::<i64,_>("generation"),
            "expires_at":row.get::<i64,_>("expires_at"),"plan_enabled":row.get::<bool,_>("plan_enabled"),
            "status":row.get::<String,_>("status")
        })).collect::<Vec<_>>()}))
    }

    pub async fn refresh(&self, id: &str, expected: Option<u64>, force: bool) -> AppResult<Value> {
        let record = self.fresh_record(id, expected, force).await?;
        Ok(
            json!({"generation":record.generation,"expires_at":record.expires_at,"plan_enabled":record.plan_enabled()}),
        )
    }

    async fn fresh_record(
        &self,
        id: &str,
        expected: Option<u64>,
        force: bool,
    ) -> AppResult<CredentialRecord> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| AppError::internal("database_unavailable"))?;
        sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| AppError::internal("refresh_lock_failed"))?;
        let stored = self.read_record(id).await?;
        let old = stored.value;
        if expected.is_some_and(|generation| old.generation != generation)
            || (!force && old.expires_at > unix_time() + 60)
        {
            tx.commit()
                .await
                .map_err(|_| AppError::internal("refresh_lock_failed"))?;
            return Ok(old);
        }
        let refresh = old
            .refresh_token
            .as_deref()
            .ok_or_else(|| AppError::bad("refresh_token_missing"))?;
        let reply = self
            .exchange(&[
                ("grant_type", "refresh_token"),
                ("client_id", &old.client_id),
                ("refresh_token", refresh),
                ("resource", RESOURCE),
            ])
            .await?;
        if let Some(id_token) = &reply.id_token {
            self.validate_id(id_token, &old.client_id, None, Some(&old.subject))
                .await?;
        }
        let new = merge_refresh(old, reply, unix_time())?;
        let version = self
            .vault
            .put(&format!("oauth/{id}"), &new, stored.version)
            .await?;
        sqlx::query("UPDATE oauth_connections SET credential_version=$2,generation=$3,expires_at=$4,plan_enabled=$5,updated_at=now() WHERE id=$1")
            .bind(id).bind(version as i64).bind(new.generation as i64)
            .bind(new.expires_at as i64).bind(new.plan_enabled()).execute(&mut *tx).await
            .map_err(|_|AppError::internal("connection_metadata_failed"))?;
        sqlx::query("INSERT INTO audit_records(actor_role,action,outcome,details) VALUES('operator','oauth.refresh','completed',$1)")
            .bind(json!({"connection_id":id,"generation":new.generation})).execute(&mut *tx).await
            .map_err(|_|AppError::internal("audit_write_failed"))?;
        tx.commit()
            .await
            .map_err(|_| AppError::internal("refresh_commit_failed"))?;
        Ok(new)
    }

    pub async fn models(&self, id: &str) -> AppResult<Value> {
        let record = self.fresh_record(id, None, false).await?;
        if !record.plan_enabled() {
            return Err(AppError::forbidden("plan_usage_not_granted"));
        }
        let response = self
            .client
            .get(format!("{RESOURCE}/models"))
            .bearer_auth(&record.access_token)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| AppError::internal("provider_unavailable"))?;
        if !response.status().is_success() {
            return Err(AppError::bad("model_catalog_rejected"));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| AppError::bad("model_catalog_invalid"))?;
        let models = body["models"]
            .as_array()
            .ok_or_else(|| AppError::bad("model_catalog_invalid"))?;
        Ok(
            json!({"models":models.iter().filter(|m|m["visibility"]=="list").map(|m|json!({
            "slug":m["slug"],"display_name":m["display_name"]})).collect::<Vec<_>>()}),
        )
    }

    pub async fn probe(&self, id: &str, model: &str) -> AppResult<Value> {
        if model.is_empty() || model.len() > 160 {
            return Err(AppError::bad("invalid_model"));
        }
        let record = self.fresh_record(id, None, false).await?;
        if !record.plan_enabled() {
            return Err(AppError::forbidden("plan_usage_not_granted"));
        }
        let response = self
            .client
            .post(format!("{RESOURCE}/responses"))
            .bearer_auth(&record.access_token)
            .json(&json!({"model":model,"store":false,"stream":true,
                "input":[{"role":"user","content":"Say exactly: ECHELON_OK"}]}))
            .send()
            .await
            .map_err(|_| AppError::internal("provider_unavailable"))?;
        if !response.status().is_success() {
            return Err(AppError::bad("inference_rejected"));
        }
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unavailable")
            .to_owned();
        let mut stream = response.bytes_stream();
        let mut parser = SseProbe::default();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| AppError::bad("inference_stream_interrupted"))?;
            parser.feed(&chunk)?;
        }
        let (text, usage) = parser.finish()?;
        let matched = text.trim() == "ECHELON_OK";
        audit(&self.pool,None,None,"operator","oauth.inference",if matched{"completed"}else{"output_mismatch"},
            json!({"connection_id":id,"model":model,"request_id":request_id,"generation":record.generation,"usage":usage})).await?;
        Ok(
            json!({"completed":true,"matched":matched,"model":model,"generation":record.generation,
            "request_id":request_id,"usage":usage}),
        )
    }
}

pub struct ModelReply {
    pub output: Vec<Value>,
    pub usage: Value,
    pub generation: u64,
}

impl OAuthService {
    /// Only the server-owned execution broker supplies instructions and tool definitions.
    /// No arbitrary credential, endpoint, or model fallback is accepted here.
    pub async fn infer(
        &self,
        id: &str,
        model: &str,
        instructions: &str,
        input: &[Value],
        tools: Value,
    ) -> AppResult<ModelReply> {
        if model.is_empty()
            || model.len() > 160
            || serde_json::to_vec(input)
                .map_err(|_| AppError::bad("invalid_inference_input"))?
                .len()
                > 262144
        {
            return Err(AppError::bad("inference_input_budget_exceeded"));
        }
        let record = self.fresh_record(id, None, false).await?;
        if !record.plan_enabled() {
            return Err(AppError::forbidden("plan_usage_not_granted"));
        }
        // Model generation has connect/idle/total deadlines independent of Task lifetime.
        let response = self
            .inference_client
            .post(format!("{RESOURCE}/responses"))
            .bearer_auth(&record.access_token)
            .json(
                &json!({"model":model,"instructions":instructions,"input":input,
                "tools":tools,"tool_choice":"required","parallel_tool_calls":false,"store":false,"stream":true,
                "include":["reasoning.encrypted_content"]}),
            )
            .send()
            .await
            .map_err(crate::persistence::transport_error)?;
        if !response.status().is_success() {
            return Err(crate::persistence::rejected(response).await);
        }
        let mut stream = response.bytes_stream();
        let mut parser = SseProbe::default();
        while let Some(chunk) = stream.next().await {
            parser.feed(&chunk.map_err(crate::persistence::transport_error)?)?;
        }
        let (output, usage) = parser.finish_output()?;
        Ok(ModelReply {
            output,
            usage,
            generation: record.generation,
        })
    }
}

#[derive(Default)]
pub struct SseProbe {
    pending: Vec<u8>,
    data: Vec<String>,
    text: String,
    completed: bool,
    failed: bool,
    total: usize,
    usage: Value,
    output: Vec<Value>,
    items: std::collections::BTreeMap<u64, Value>,
}

impl SseProbe {
    pub fn feed(&mut self, chunk: &[u8]) -> AppResult<()> {
        self.total += chunk.len();
        if self.total > 2 * 1024 * 1024 {
            return Err(AppError::bad("inference_stream_too_large"));
        }
        self.pending.extend_from_slice(chunk);
        while let Some(index) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=index).collect();
            let line = std::str::from_utf8(&line)
                .map_err(|_| AppError::bad("inference_stream_invalid"))?
                .trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                self.event()?;
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data.push(data.trim_start().into());
            }
        }
        Ok(())
    }

    fn event(&mut self) -> AppResult<()> {
        if self.data.is_empty() {
            return Ok(());
        }
        let data = self.data.join("\n");
        self.data.clear();
        if data == "[DONE]" {
            return Ok(());
        }
        let value: Value =
            serde_json::from_str(&data).map_err(|_| AppError::bad("inference_stream_invalid"))?;
        match value["type"].as_str() {
            Some("response.output_item.done") => {
                let index = value["output_index"]
                    .as_u64()
                    .filter(|n| *n < 128)
                    .ok_or_else(|| AppError::bad("inference_stream_invalid"))?;
                if !value["item"].is_object() {
                    return Err(AppError::bad("inference_stream_invalid"));
                }
                self.items.insert(index, value["item"].clone());
            }
            Some("response.output_text.delta") => self
                .text
                .push_str(value["delta"].as_str().unwrap_or_default()),
            Some("response.completed") => {
                if self.completed {
                    return Err(AppError::bad("inference_duplicate_completion"));
                }
                if value["response"]["status"] != "completed" {
                    self.failed = true;
                }
                self.completed = true;
                self.usage = value["response"]["usage"].clone();
                self.output = value["response"]["output"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if self.text.is_empty()
                    && let Some(output) = value["response"]["output"].as_array()
                {
                    for item in output {
                        if let Some(content) = item["content"].as_array() {
                            for part in content {
                                if part["type"] == "output_text" {
                                    self.text
                                        .push_str(part["text"].as_str().unwrap_or_default());
                                }
                            }
                        }
                    }
                }
            }
            Some("response.failed" | "response.incomplete" | "error") => self.failed = true,
            _ => {}
        }
        Ok(())
    }

    fn terminal(&mut self) -> AppResult<()> {
        if !self.pending.is_empty() {
            let line = std::str::from_utf8(&self.pending)
                .map_err(|_| AppError::bad("inference_stream_invalid"))?;
            if let Some(data) = line.strip_prefix("data:") {
                self.data.push(data.trim().into());
            }
        }
        self.event()?;
        if self.failed {
            return Err(AppError::bad("inference_failed"));
        }
        if !self.completed {
            return Err(AppError::bad("inference_missing_completion"));
        }
        // Some transports omit output items from the terminal envelope. Completed item
        // events carry the full function arguments and encrypted reasoning in index order.
        if self.output.is_empty() {
            self.output = self.items.values().cloned().collect();
        }
        Ok(())
    }
    pub fn finish(mut self) -> AppResult<(String, Value)> {
        self.terminal()?;
        Ok((self.text, self.usage))
    }
    pub fn finish_output(mut self) -> AppResult<(Vec<Value>, Value)> {
        self.terminal()?;
        if self.output.is_empty() {
            return Err(AppError::bad("inference_output_missing"));
        }
        Ok((self.output, self.usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn function_calls_are_released_only_after_terminal_success() {
        let call = json!({"type":"function_call","name":"invoke","namespace":"echelon","call_id":"synthetic","arguments":"{}"});
        let frame = |status: &str| {
            format!(
                "data: {}\n\n",
                json!({"type":format!("response.{status}"),"response":{"status":status,"output":[call.clone()],"usage":{"total_tokens":10}}})
            )
        };
        let mut failed = SseProbe::default();
        failed.feed(frame("incomplete").as_bytes()).unwrap();
        assert!(failed.finish_output().is_err());
        let mut success = SseProbe::default();
        for chunk in frame("completed").as_bytes().chunks(7) {
            success.feed(chunk).unwrap();
        }
        let (output, usage) = success.finish_output().unwrap();
        assert_eq!(output, vec![call]);
        assert_eq!(usage["total_tokens"], 10);
        let item = format!(
            "data: {}\n\n",
            json!({"type":"response.output_item.done","output_index":0,"item":output[0]})
        );
        let mut pending = SseProbe::default();
        pending.feed(item.as_bytes()).unwrap();
        assert!(pending.finish_output().is_err());
        let mut streamed = SseProbe::default();
        streamed.feed(item.as_bytes()).unwrap();
        streamed.feed(b"data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"total_tokens\":10}}}\n\n").unwrap();
        assert_eq!(streamed.finish_output().unwrap().0, output);
    }
    #[test]
    fn pkce_matches_rfc7636_vector() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }
    #[test]
    fn refresh_rotation_is_atomic_record_replacement() {
        let old = CredentialRecord {
            issuer: ISSUER.into(),
            subject: "synthetic-sub".into(),
            client_id: "synthetic-client".into(),
            host_id: "urn:uuid:synthetic".into(),
            access_token: "old-synthetic-access".into(),
            refresh_token: Some("old-synthetic-refresh".into()),
            id_token: "synthetic-id".into(),
            scopes: vec!["chatgpt.tokens.use.direct".into()],
            expires_at: 1,
            generation: 7,
        };
        let reply = TokenReply {
            access_token: "new-synthetic-access".into(),
            refresh_token: Some("new-synthetic-refresh".into()),
            id_token: None,
            token_type: "Bearer".into(),
            expires_in: 3600,
            scope: None,
        };
        let new = merge_refresh(old, reply, 100).unwrap();
        assert_eq!(new.refresh_token.as_deref(), Some("new-synthetic-refresh"));
        assert_eq!(new.expires_at, 3700);
        assert_eq!(new.generation, 8);
        assert!(new.plan_enabled());
    }
    #[test]
    fn stream_requires_terminal_success_even_after_text() {
        let mut p = SseProbe::default();
        p.feed(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"ECHELON_OK\"}\n\n")
            .unwrap();
        assert_eq!(p.finish().unwrap_err().code, "inference_missing_completion");
        let mut p = SseProbe::default();
        p.feed(b"data: {\"type\":\"response.failed\"}\n\ndata: [DONE]\n\n")
            .unwrap();
        assert_eq!(p.finish().unwrap_err().code, "inference_failed");
    }
    #[test]
    fn stream_handles_fragmented_utf8_and_crlf() {
        let payload = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"확인\"}\r\n\r\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":2}}}\r\n\r\n";
        let mut p = SseProbe::default();
        for chunk in payload.as_bytes().chunks(1) {
            p.feed(chunk).unwrap();
        }
        let (text, usage) = p.finish().unwrap();
        assert_eq!(text, "확인");
        assert_eq!(usage["input_tokens"], 2);
    }
}
