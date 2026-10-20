use sqlx::postgres::PgConnectOptions;
use std::{env, path::PathBuf};

#[derive(Clone)]
pub struct Config {
    pub listen: String,
    pub vault_url: String,
    pub vault_token_file: PathBuf,
    pub callback_uri: String,
    pub ui_origins: String,
    pub postgres_host: String,
    pub postgres_password_file: PathBuf,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            listen: env::var("ECHELON_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            vault_url: env::var("VAULT_ADDR").unwrap_or_else(|_| "http://vault:8200".into()),
            vault_token_file: env::var("VAULT_TOKEN_FILE")
                .unwrap_or_else(|_| "/run/secrets/vault-core-token".into())
                .into(),
            callback_uri: env::var("ECHELON_CALLBACK_URI")
                .unwrap_or_else(|_| "http://127.0.0.1:18431/auth/callback".into()),
            ui_origins: env::var("ECHELON_UI_ORIGINS").unwrap_or_default(),
            postgres_host: env::var("POSTGRES_HOST").unwrap_or_else(|_| "postgres".into()),
            postgres_password_file: env::var("POSTGRES_PASSWORD_FILE")
                .unwrap_or_else(|_| "/run/secrets/postgres-password".into())
                .into(),
        }
    }

    pub async fn database_options(&self) -> anyhow::Result<PgConnectOptions> {
        let password = tokio::fs::read_to_string(&self.postgres_password_file).await?;
        Ok(PgConnectOptions::new()
            .host(&self.postgres_host)
            .port(5432)
            .username("echelon")
            .database("echelon")
            .password(password.trim()))
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        crate::ui_auth::origins(&self.ui_origins)?;
        let uri = url::Url::parse(&self.callback_uri)?;
        anyhow::ensure!(
            uri.scheme() == "http"
                && uri.host_str() == Some("127.0.0.1")
                && uri.port().is_some()
                && uri.path() == "/auth/callback"
                && uri.query().is_none()
                && uri.fragment().is_none()
                && uri.username().is_empty()
                && uri.password().is_none(),
            "invalid loopback callback"
        );
        Ok(())
    }
}
