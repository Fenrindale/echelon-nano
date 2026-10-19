use clap::{Parser, Subcommand};
use echelon_core::{
    bootstrap,
    config::Config,
    policy::{self, Capability, Role, ToolRequest},
    server,
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(name = "echelon", version, about = "Echelon Nano control plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Serve,
    Bootstrap {
        #[arg(long, default_value = "/bootstrap")]
        directory: PathBuf,
    },
    RoleProbe {
        #[arg(long, value_enum)]
        role: Role,
        #[arg(long, default_value = "/run/secrets/role-token")]
        token_file: PathBuf,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter("echelon_core=info,echelon=info")
        .with_target(false)
        .init();
    let result = match Cli::parse().command {
        Command::Serve => server::run(Config::from_env()).await,
        Command::Bootstrap { directory } => bootstrap::run(&directory)
            .await
            .map(|status| println!("{status}")),
        Command::RoleProbe { role, token_file } => role_probe(role, token_file).await,
    };
    if let Err(error) = result {
        tracing::error!(event = "command_failed", reason = %error);
        std::process::exit(1);
    }
}

async fn role_probe(role: Role, token_file: PathBuf) -> anyhow::Result<()> {
    let token = tokio::fs::read_to_string(token_file).await?;
    let base = std::env::var("ECHELON_CORE_URL").unwrap_or_else(|_| "http://core:8080".into());
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(5))
        .build()?;
    let identity: Value = client
        .get(format!("{base}/v1/whoami"))
        .bearer_auth(token.trim())
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let actual: Role = serde_json::from_value(identity["role"].clone())?;
    anyhow::ensure!(actual == role, "role identity mismatch");
    let tenant = serde_json::from_value(identity["tenant_id"].clone())?;
    let task = serde_json::from_value(identity["task_id"].clone())?;
    let target = identity["targets"]
        .as_array()
        .and_then(|v| v.first())
        .and_then(Value::as_str)
        .map(String::from);
    let mut denied = 0;
    for tool in policy::ALL {
        let request = ToolRequest {
            tool,
            tenant_id: tenant,
            task_id: task,
            target: target.clone(),
            input: Value::Null,
        };
        let decision: Value = client
            .post(format!("{base}/v1/permissions/check"))
            .bearer_auth(token.trim())
            .json(&request)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        anyhow::ensure!(
            decision["allowed"] == policy::role_allows(role, tool),
            "permission matrix mismatch"
        );
        if !policy::role_allows(role, tool) {
            let result = client
                .post(format!("{base}/v1/tools/invoke"))
                .bearer_auth(token.trim())
                .header("X-Role", "agent")
                .json(&request)
                .send()
                .await?;
            anyhow::ensure!(
                result.status().as_u16() == 403,
                "forbidden dispatch was not blocked"
            );
            denied += 1;
        }
    }
    let allowed = policy::ALL
        .into_iter()
        .find(|action| policy::role_allows(role, *action))
        .unwrap_or(Capability::KnowledgeRead);
    let cross_scope = ToolRequest {
        tool: allowed,
        tenant_id: uuid::Uuid::new_v4(),
        task_id: task,
        target,
        input: Value::Null,
    };
    let response = client
        .post(format!("{base}/v1/tools/invoke"))
        .bearer_auth(token.trim())
        .json(&cross_scope)
        .send()
        .await?;
    anyhow::ensure!(
        response.status().as_u16() == 403,
        "cross-tenant dispatch accepted"
    );
    let egress_blocked = client
        .get("https://example.com")
        .timeout(Duration::from_secs(3))
        .send()
        .await
        .is_err();
    anyhow::ensure!(egress_blocked, "role runtime has direct internet access");
    println!(
        "{}",
        json!({"role":role,"matrix_checks":policy::ALL.len(),"forbidden_dispatch_checks":denied,
        "cross_tenant_denied":true,"egress_blocked":egress_blocked})
    );
    Ok(())
}
