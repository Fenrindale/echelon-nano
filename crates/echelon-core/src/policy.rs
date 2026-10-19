use crate::{
    error::{AppError, AppResult},
    unix_time,
};
use clap::ValueEnum;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Analyst,
    Handler,
    Agent,
    Master,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub enum Capability {
    #[serde(rename = "knowledge.read")]
    KnowledgeRead,
    #[serde(rename = "knowledge.classify")]
    KnowledgeClassify,
    #[serde(rename = "knowledge.publish")]
    KnowledgePublish,
    #[serde(rename = "context.request")]
    ContextRequest,
    #[serde(rename = "context.curate")]
    ContextCurate,
    #[serde(rename = "context.read")]
    ContextRead,
    #[serde(rename = "task.plan")]
    TaskPlan,
    #[serde(rename = "knowledge.submit")]
    KnowledgeSubmit,
    #[serde(rename = "agent.delegate")]
    AgentDelegate,
    #[serde(rename = "task.observe")]
    TaskObserve,
    #[serde(rename = "external.fetch")]
    ExternalFetch,
    #[serde(rename = "shell.exec")]
    ShellExec,
    #[serde(rename = "knowledge.optimize")]
    KnowledgeOptimize,
    #[serde(rename = "protocol.manage")]
    ProtocolManage,
    #[serde(rename = "audit.read")]
    AuditRead,
    #[serde(rename = "audit.rewrite")]
    AuditRewrite,
    #[serde(rename = "vault.raw_read")]
    VaultRawRead,
}

pub const ALL: [Capability; 17] = [
    Capability::KnowledgeRead,
    Capability::KnowledgeClassify,
    Capability::KnowledgePublish,
    Capability::ContextRequest,
    Capability::ContextCurate,
    Capability::ContextRead,
    Capability::TaskPlan,
    Capability::KnowledgeSubmit,
    Capability::AgentDelegate,
    Capability::TaskObserve,
    Capability::ExternalFetch,
    Capability::ShellExec,
    Capability::KnowledgeOptimize,
    Capability::ProtocolManage,
    Capability::AuditRead,
    Capability::AuditRewrite,
    Capability::VaultRawRead,
];

pub fn role_allows(role: Role, action: Capability) -> bool {
    use Capability::*;
    match role {
        Role::Analyst => matches!(
            action,
            KnowledgeRead | KnowledgeClassify | KnowledgePublish | ContextCurate
        ),
        Role::Handler => matches!(
            action,
            ContextRequest | ContextRead | TaskPlan | KnowledgeSubmit | AgentDelegate
        ),
        Role::Agent => matches!(action, TaskObserve | ExternalFetch | ShellExec),
        Role::Master => matches!(
            action,
            KnowledgeRead | KnowledgeOptimize | ProtocolManage | AuditRead
        ),
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Claims {
    pub iss: String,
    pub aud: String,
    pub sub: String,
    pub role: Role,
    pub tenant_id: Uuid,
    pub task_id: Uuid,
    pub work_order_id: Option<Uuid>,
    pub targets: Vec<String>,
    pub exp: u64,
    pub iat: u64,
    pub jti: Uuid,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolRequest {
    pub tool: Capability,
    pub tenant_id: Uuid,
    pub task_id: Uuid,
    pub target: Option<String>,
    #[serde(default)]
    pub input: serde_json::Value,
}

pub fn authorize(claims: &Claims, req: &ToolRequest) -> AppResult<()> {
    if !role_allows(claims.role, req.tool) {
        return Err(AppError::forbidden("role_denied"));
    }
    if claims.tenant_id != req.tenant_id || claims.task_id != req.task_id {
        return Err(AppError::forbidden("scope_denied"));
    }
    if matches!(
        req.tool,
        Capability::ExternalFetch | Capability::ShellExec | Capability::TaskObserve
    ) {
        if claims.work_order_id.is_none() {
            return Err(AppError::forbidden("work_order_required"));
        }
        if matches!(req.tool, Capability::ExternalFetch | Capability::ShellExec)
            && !req
                .target
                .as_ref()
                .is_some_and(|target| claims.targets.contains(target))
        {
            return Err(AppError::forbidden("target_denied"));
        }
    }
    Ok(())
}

pub fn sign(key: &[u8], claims: &Claims) -> AppResult<String> {
    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        claims,
        &EncodingKey::from_secret(key),
    )
    .map_err(|_| AppError::internal("role_token_sign_failed"))
}

pub fn verify(key: &[u8], token: &str) -> AppResult<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&["echelon-core"]);
    validation.set_issuer(&["echelon-nano"]);
    validation.leeway = 0;
    let claims = jsonwebtoken::decode::<Claims>(token, &DecodingKey::from_secret(key), &validation)
        .map_err(|_| AppError::unauthorized())?
        .claims;
    if claims.iat > unix_time() + 30 || claims.exp <= claims.iat || claims.exp - claims.iat > 3600 {
        return Err(AppError::unauthorized());
    }
    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn claims(role: Role) -> Claims {
        Claims {
            iss: "echelon-nano".into(),
            aud: "echelon-core".into(),
            sub: "role-test".into(),
            role,
            tenant_id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            work_order_id: Some(Uuid::new_v4()),
            targets: vec!["registered-source".into()],
            exp: unix_time() + 300,
            iat: unix_time(),
            jti: Uuid::new_v4(),
        }
    }
    #[test]
    fn least_privilege_matrix_and_forbidden_primitives() {
        for role in [Role::Analyst, Role::Handler, Role::Agent, Role::Master] {
            assert!(!role_allows(role, Capability::AuditRewrite));
            assert!(!role_allows(role, Capability::VaultRawRead));
        }
        assert!(!role_allows(Role::Analyst, Capability::ExternalFetch));
        assert!(!role_allows(Role::Analyst, Capability::AgentDelegate));
        assert!(!role_allows(Role::Handler, Capability::KnowledgePublish));
        assert!(!role_allows(Role::Master, Capability::ShellExec));
        assert!(!role_allows(Role::Master, Capability::AgentDelegate));
        for role in [Role::Analyst, Role::Handler, Role::Agent, Role::Master] {
            assert_eq!(
                role_allows(role, Capability::ContextRequest),
                role == Role::Handler
            );
            assert_eq!(
                role_allows(role, Capability::ContextRead),
                role == Role::Handler
            );
            assert_eq!(
                role_allows(role, Capability::ContextCurate),
                role == Role::Analyst
            );
        }
    }
    #[test]
    fn scope_work_order_and_target_are_independent_gates() {
        let mut actor = claims(Role::Agent);
        let mut req = ToolRequest {
            tool: Capability::ExternalFetch,
            tenant_id: actor.tenant_id,
            task_id: actor.task_id,
            target: Some("registered-source".into()),
            input: serde_json::Value::Null,
        };
        assert!(authorize(&actor, &req).is_ok());
        req.tenant_id = Uuid::new_v4();
        assert_eq!(authorize(&actor, &req).unwrap_err().code, "scope_denied");
        req.tenant_id = actor.tenant_id;
        req.target = Some("other-source".into());
        assert_eq!(authorize(&actor, &req).unwrap_err().code, "target_denied");
        req.target = Some("registered-source".into());
        actor.work_order_id = None;
        assert_eq!(
            authorize(&actor, &req).unwrap_err().code,
            "work_order_required"
        );
    }
    #[test]
    fn signed_identity_cannot_be_changed_by_caller() {
        let actor = claims(Role::Analyst);
        let token = sign(b"test-key-not-a-runtime-secret", &actor).unwrap();
        assert_eq!(
            verify(b"test-key-not-a-runtime-secret", &token)
                .unwrap()
                .role,
            Role::Analyst
        );
        assert!(verify(b"different-test-key", &token).is_err());
        let mut expired = actor.clone();
        expired.iat = unix_time() - 1000;
        expired.exp = unix_time() - 1;
        assert!(
            verify(
                b"test-key-not-a-runtime-secret",
                &sign(b"test-key-not-a-runtime-secret", &expired).unwrap()
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<ToolRequest>(serde_json::json!({
            "tool":"external.fetch","tenant_id":actor.tenant_id,"task_id":actor.task_id,
            "target":"registered-source","role":"agent"}))
            .is_err()
        );
    }
}
