use crate::{config::Config, store::VaultStore};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{path::Path, time::Duration};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
pub struct RuntimeKeys {
    pub role_signing_key: String,
    pub operator_hash: String,
}

pub fn random_secret() -> String {
    let mut bytes = [0u8; 48];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn secret_hash(input: &str) -> String {
    hex::encode(Sha256::digest(input.as_bytes()))
}

pub async fn private_write(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true).mode(0o600);
        use std::io::Write;
        let mut file = options.open(path)?;
        file.write_all(data)?;
        file.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        tokio::fs::write(path, data).await?;
    }
    Ok(())
}

pub async fn run(directory: &Path) -> anyhow::Result<Value> {
    let config = Config::from_env();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let base = config.vault_url;
    let recovery_path = directory.join("vault-recovery.json");
    let health = client.get(format!("{base}/v1/sys/health")).send().await?;
    let health: Value = health.json().await?;
    let newly_initialized = !health["initialized"].as_bool().unwrap_or(false);
    let recovery: Value = if newly_initialized {
        let reply = client
            .post(format!("{base}/v1/sys/init"))
            .json(&json!({"secret_shares":1,"secret_threshold":1}))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        private_write(&recovery_path, &serde_json::to_vec(&reply)?).await?;
        reply
    } else {
        serde_json::from_slice(
            &tokio::fs::read(&recovery_path)
                .await
                .context("protected recovery material is required for bootstrap")?,
        )?
    };
    use anyhow::Context;
    if health["sealed"].as_bool().unwrap_or(true) {
        let key = recovery["keys_base64"][0]
            .as_str()
            .context("recovery key unavailable")?;
        client
            .post(format!("{base}/v1/sys/unseal"))
            .json(&json!({"key":key}))
            .send()
            .await?
            .error_for_status()?;
    }
    let root = recovery["root_token"]
        .as_str()
        .context("recovery token unavailable")?;
    let mounts: Value = client
        .get(format!("{base}/v1/sys/mounts"))
        .header("X-Vault-Token", root)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if mounts.get("echelon/").is_none() {
        client
            .post(format!("{base}/v1/sys/mounts/echelon"))
            .header("X-Vault-Token", root)
            .json(&json!({"type":"kv","options":{"version":"2"}}))
            .send()
            .await?
            .error_for_status()?;
    }
    let policy = r#"path "echelon/data/oauth/*" { capabilities = ["create","read","update"] }
path "echelon/data/runtime/identity" { capabilities = ["read"] }
path "echelon/data/runtime/keys" { capabilities = ["read"] }
path "auth/token/renew-self" { capabilities = ["update"] }
path "auth/token/lookup-self" { capabilities = ["read"] }"#;
    client
        .put(format!("{base}/v1/sys/policies/acl/echelon-core"))
        .header("X-Vault-Token", root)
        .json(&json!({"policy":policy}))
        .send()
        .await?
        .error_for_status()?;
    let root_store = VaultStore::new(base.clone(), root.into())?;
    if root_store.get::<Value>("runtime/identity").await?.is_none() {
        root_store
            .put(
                "runtime/identity",
                &json!({"host_id":format!("urn:uuid:{}",Uuid::new_v4())}),
                0,
            )
            .await?;
    }
    let keys_path = "runtime/keys";
    let existing = root_store.get::<RuntimeKeys>(keys_path).await?;
    let operator = tokio::fs::read_to_string(directory.join("operator-input")).await?;
    let new_keys = RuntimeKeys {
        role_signing_key: existing
            .as_ref()
            .map(|x| x.value.role_signing_key.clone())
            .unwrap_or_else(random_secret),
        operator_hash: secret_hash(operator.trim()),
    };
    root_store
        .put(keys_path, &new_keys, existing.map_or(0, |x| x.version))
        .await?;
    let token:Value=client.post(format!("{base}/v1/auth/token/create")).header("X-Vault-Token",root)
        .json(&json!({"policies":["echelon-core"],"no_default_policy":true,"period":"1h","renewable":true}))
        .send().await?.error_for_status()?.json().await?;
    let app_token = token["auth"]["client_token"]
        .as_str()
        .context("application credential unavailable")?;
    private_write(&directory.join("vault-core-token"), app_token.as_bytes()).await?;
    Ok(json!({"status":"ready","initialized":newly_initialized}))
}
