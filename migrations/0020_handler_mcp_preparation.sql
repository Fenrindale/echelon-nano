-- Preparation is an infrastructure effect, separate from Agent business calls.
-- Requests and final receipts survive restarts without changing either history.
CREATE TABLE mcp_preparations (
 tenant_id uuid NOT NULL REFERENCES tenants(id),
 task_id uuid NOT NULL REFERENCES tasks(id),
 request_key uuid NOT NULL,
 request_hash text NOT NULL,
 request jsonb NOT NULL,
 server_id uuid NOT NULL REFERENCES mcp_servers(id),
 server_version bigint NOT NULL,
 reused boolean NOT NULL,
 created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(tenant_id,task_id,request_key)
);
CREATE TABLE mcp_preparation_results (
 tenant_id uuid NOT NULL, task_id uuid NOT NULL, request_key uuid NOT NULL,
 result jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(tenant_id,task_id,request_key),
 FOREIGN KEY(tenant_id,task_id,request_key) REFERENCES mcp_preparations(tenant_id,task_id,request_key)
);
CREATE TRIGGER mcp_preparations_immutable BEFORE UPDATE OR DELETE ON mcp_preparations FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER mcp_preparations_no_truncate BEFORE TRUNCATE ON mcp_preparations FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER mcp_preparation_results_immutable BEFORE UPDATE OR DELETE ON mcp_preparation_results FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER mcp_preparation_results_no_truncate BEFORE TRUNCATE ON mcp_preparation_results FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
