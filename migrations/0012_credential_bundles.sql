ALTER TABLE artifacts ADD CONSTRAINT artifact_scope_identity UNIQUE(tenant_id,task_id,id);
CREATE TABLE credential_bundles (
 id UUID PRIMARY KEY,
 tenant_id UUID NOT NULL REFERENCES tenants(id),
 source_task_id UUID NOT NULL,
 source_artifact_id UUID NOT NULL,
 source_sha256 TEXT NOT NULL,
 name TEXT NOT NULL,
 entry_names JSONB NOT NULL,
 empty_names JSONB NOT NULL,
 vault_version BIGINT NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 UNIQUE(tenant_id,id),
 UNIQUE(tenant_id,source_task_id,source_artifact_id),
 FOREIGN KEY(tenant_id,source_task_id) REFERENCES tasks(tenant_id,id),
 FOREIGN KEY(tenant_id,source_task_id,source_artifact_id) REFERENCES artifacts(tenant_id,task_id,id)
);
CREATE TRIGGER credential_bundle_immutable BEFORE UPDATE OR DELETE ON credential_bundles FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER credential_bundle_no_truncate BEFORE TRUNCATE ON credential_bundles FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();

CREATE TABLE credential_connectors (
 id UUID PRIMARY KEY,
 tenant_id UUID NOT NULL REFERENCES tenants(id),
 name TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('gitlab','jenkins','databricks')),
 base_url TEXT NOT NULL,
 username_variable TEXT NOT NULL DEFAULT '',
 token_variable TEXT NOT NULL,
 allow_private BOOLEAN NOT NULL DEFAULT false,
 allow_http BOOLEAN NOT NULL DEFAULT false,
 enabled BOOLEAN NOT NULL DEFAULT true,
 version BIGINT NOT NULL DEFAULT 1,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 UNIQUE(tenant_id,id)
);

CREATE TABLE credential_checks (
 id UUID PRIMARY KEY,
 tenant_id UUID NOT NULL,
 task_id UUID NOT NULL,
 work_order_id UUID NOT NULL,
 bundle_id UUID NOT NULL,
 connector_id UUID NOT NULL,
 connector_version BIGINT NOT NULL,
 result JSONB NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 UNIQUE(tenant_id,work_order_id,bundle_id,connector_id,connector_version),
 FOREIGN KEY(tenant_id,task_id,work_order_id) REFERENCES work_orders(tenant_id,task_id,id),
 FOREIGN KEY(tenant_id,bundle_id) REFERENCES credential_bundles(tenant_id,id),
 FOREIGN KEY(tenant_id,connector_id) REFERENCES credential_connectors(tenant_id,id)
);
CREATE TRIGGER credential_check_immutable BEFORE UPDATE OR DELETE ON credential_checks FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER credential_check_no_truncate BEFORE TRUNCATE ON credential_checks FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
