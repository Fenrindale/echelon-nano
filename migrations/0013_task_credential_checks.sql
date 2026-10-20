-- Task-local identities do not require a workspace connector registry entry.
CREATE TABLE task_credential_checks (
 id UUID PRIMARY KEY,
 tenant_id UUID NOT NULL,
 task_id UUID NOT NULL,
 work_order_id UUID NOT NULL,
 bundle_id UUID NOT NULL,
 binding_index INT NOT NULL CHECK(binding_index >= 0 AND binding_index < 8),
 binding_sha256 TEXT NOT NULL,
 result JSONB NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 UNIQUE(tenant_id,work_order_id,bundle_id,binding_index,binding_sha256),
 FOREIGN KEY(tenant_id,task_id,work_order_id) REFERENCES work_orders(tenant_id,task_id,id),
 FOREIGN KEY(tenant_id,bundle_id) REFERENCES credential_bundles(tenant_id,id)
);
CREATE TRIGGER task_credential_check_immutable BEFORE UPDATE OR DELETE ON task_credential_checks FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER task_credential_check_no_truncate BEFORE TRUNCATE ON task_credential_checks FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
