use crate::error::{AppError, AppResult};
use reqwest::{Client, StatusCode};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use uuid::Uuid;

#[derive(Clone)]
pub struct VaultStore {
    client: Client,
    base: String,
    token: Arc<String>,
}

pub struct Versioned<T> {
    pub value: T,
    pub version: u64,
}

impl VaultStore {
    pub fn new(base: String, token: String) -> anyhow::Result<Self> {
        Ok(Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .build()?,
            base: base.trim_end_matches('/').into(),
            token: Arc::new(token),
        })
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> AppResult<Option<Versioned<T>>> {
        let response = self
            .client
            .get(format!("{}/v1/echelon/data/{}", self.base, path))
            .header("X-Vault-Token", self.token.as_str())
            .send()
            .await
            .map_err(|_| AppError::internal("vault_unavailable"))?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(AppError::internal("vault_read_failed"));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| AppError::internal("vault_invalid_response"))?;
        let value = serde_json::from_value(body["data"]["data"].clone())
            .map_err(|_| AppError::internal("vault_invalid_record"))?;
        let version = body["data"]["metadata"]["version"]
            .as_u64()
            .ok_or_else(|| AppError::internal("vault_version_missing"))?;
        Ok(Some(Versioned { value, version }))
    }

    pub async fn put<T: Serialize>(&self, path: &str, value: &T, expected: u64) -> AppResult<u64> {
        let response = self
            .client
            .post(format!("{}/v1/echelon/data/{}", self.base, path))
            .header("X-Vault-Token", self.token.as_str())
            .json(&json!({"options":{"cas":expected},"data":value}))
            .send()
            .await
            .map_err(|_| AppError::internal("vault_unavailable"))?;
        if !response.status().is_success() {
            return Err(AppError::internal("vault_write_failed"));
        }
        let body: Value = response
            .json()
            .await
            .map_err(|_| AppError::internal("vault_invalid_response"))?;
        body["data"]["version"]
            .as_u64()
            .ok_or_else(|| AppError::internal("vault_version_missing"))
    }

    pub async fn renew(&self) -> AppResult<()> {
        let response = self
            .client
            .post(format!("{}/v1/auth/token/renew-self", self.base))
            .header("X-Vault-Token", self.token.as_str())
            .json(&json!({}))
            .send()
            .await
            .map_err(|_| AppError::internal("vault_unavailable"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(AppError::internal("vault_token_renew_failed"))
        }
    }

    pub async fn healthy(&self) -> bool {
        self.client
            .get(format!("{}/v1/sys/health", self.base))
            .send()
            .await
            .is_ok_and(|r| r.status().is_success())
    }
}

pub async fn audit(
    pool: &PgPool,
    tenant: Option<Uuid>,
    task: Option<Uuid>,
    role: &str,
    action: &str,
    outcome: &str,
    details: Value,
) -> AppResult<()> {
    sqlx::query("INSERT INTO audit_records(tenant_id,task_id,actor_role,action,outcome,details) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(tenant).bind(task).bind(role).bind(action).bind(outcome).bind(details)
        .execute(pool).await.map_err(|_|AppError::internal("audit_write_failed"))?;
    Ok(())
}
