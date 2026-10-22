CREATE TABLE mcp_servers (
 id uuid PRIMARY KEY, tenant_id uuid NOT NULL REFERENCES tenants(id), name text NOT NULL,
 version bigint NOT NULL DEFAULT 1, enabled boolean NOT NULL DEFAULT true,
 spec jsonb NOT NULL, tools jsonb NOT NULL DEFAULT '[]', status text NOT NULL DEFAULT 'configured',
 error_code text, updated_at timestamptz NOT NULL DEFAULT now(), UNIQUE(tenant_id,name)
);
CREATE TABLE mcp_server_revisions (
 server_id uuid NOT NULL REFERENCES mcp_servers(id), version bigint NOT NULL,
 spec jsonb NOT NULL, tools jsonb NOT NULL, PRIMARY KEY(server_id,version)
);
CREATE TABLE mcp_call_receipts (
 tenant_id uuid NOT NULL, task_id uuid NOT NULL, work_order_id uuid NOT NULL,
 request_key uuid NOT NULL, request_hash text NOT NULL, server_id uuid NOT NULL,
 server_version bigint NOT NULL, tool text NOT NULL, status text NOT NULL,
 result jsonb, created_at timestamptz NOT NULL DEFAULT now(),
 PRIMARY KEY(tenant_id,task_id,work_order_id,request_key)
);
CREATE TRIGGER mcp_revision_immutable BEFORE UPDATE OR DELETE ON mcp_server_revisions FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER mcp_revision_no_truncate BEFORE TRUNCATE ON mcp_server_revisions FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE FUNCTION guard_mcp_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_OP='DELETE' OR OLD.status<>'pending' OR NEW.status NOT IN ('completed','unknown')
 OR (to_jsonb(OLD)-'status'-'result')<>(to_jsonb(NEW)-'status'-'result')
 THEN RAISE EXCEPTION 'immutable MCP receipt'; END IF;
 RETURN NEW; END; $$;
CREATE TRIGGER mcp_receipt_guard BEFORE UPDATE OR DELETE ON mcp_call_receipts FOR EACH ROW EXECUTE FUNCTION guard_mcp_receipt();
CREATE TRIGGER mcp_receipt_no_truncate BEFORE TRUNCATE ON mcp_call_receipts FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
