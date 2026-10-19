ALTER TABLE tasks ADD COLUMN contract JSONB NOT NULL DEFAULT '{}';
ALTER TABLE tasks ADD COLUMN archived BOOLEAN NOT NULL DEFAULT false;
ALTER TABLE tasks ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();
ALTER TABLE work_orders ADD COLUMN request_key UUID;
CREATE UNIQUE INDEX work_order_request_unique ON work_orders(tenant_id,task_id,request_key) WHERE request_key IS NOT NULL;
ALTER TABLE observations ADD COLUMN request_key UUID;
CREATE UNIQUE INDEX observation_request_unique ON observations(tenant_id,task_id,work_order_id,request_key) WHERE request_key IS NOT NULL;
ALTER TABLE execution_runs DROP CONSTRAINT execution_runs_status_check;
ALTER TABLE execution_runs ADD CONSTRAINT execution_runs_status_check CHECK(status IN ('running','completed','failed','exhausted','interrupted','paused','cancelled'));
ALTER TABLE execution_runs ADD COLUMN control TEXT NOT NULL DEFAULT 'continue' CHECK(control IN ('continue','pause','cancel'));
ALTER TABLE execution_runs ADD COLUMN generation BIGINT NOT NULL DEFAULT 1;
CREATE TABLE execution_actions (
    run_id UUID NOT NULL REFERENCES execution_runs(id),
    call_id TEXT NOT NULL,
    request JSONB NOT NULL,
    outcome JSONB,
    PRIMARY KEY(run_id,call_id)
);
CREATE TABLE artifacts (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    task_id UUID NOT NULL,
    work_order_id UUID,
    request_key UUID NOT NULL,
    name TEXT NOT NULL,
    media_type TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    body BYTEA NOT NULL CHECK(octet_length(body)<=1048576),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY(tenant_id,task_id) REFERENCES tasks(tenant_id,id),
    FOREIGN KEY(tenant_id,task_id,work_order_id) REFERENCES work_orders(tenant_id,task_id,id),
    UNIQUE(tenant_id,task_id,request_key)
);
CREATE TRIGGER artifact_immutable BEFORE UPDATE OR DELETE ON artifacts FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER artifact_no_truncate BEFORE TRUNCATE ON artifacts FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TABLE secret_refs (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id),
    name TEXT NOT NULL,
    purpose TEXT NOT NULL,
    allowed_urls JSONB NOT NULL,
    version BIGINT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE tool_receipts (
    tenant_id UUID NOT NULL,
    task_id UUID NOT NULL,
    work_order_id UUID NOT NULL,
    request_key UUID NOT NULL,
    request_hash TEXT NOT NULL,
    tool TEXT NOT NULL,
    status TEXT NOT NULL,
    result JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(tenant_id,task_id,work_order_id,request_key),
    FOREIGN KEY(tenant_id,task_id,work_order_id) REFERENCES work_orders(tenant_id,task_id,id)
);
CREATE TABLE activity_events (
    seq BIGSERIAL PRIMARY KEY,
    tenant_id UUID,
    task_id UUID,
    actor TEXT NOT NULL,
    kind TEXT NOT NULL,
    data JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER activity_immutable BEFORE UPDATE OR DELETE ON activity_events FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER activity_no_truncate BEFORE TRUNCATE ON activity_events FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE FUNCTION relay_audit_activity() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 INSERT INTO activity_events(tenant_id,task_id,actor,kind,data) VALUES(NEW.tenant_id,NEW.task_id,NEW.actor_role,NEW.action,jsonb_build_object('outcome',NEW.outcome,'details',NEW.details));
 RETURN NEW; END; $$;
CREATE TRIGGER audit_activity AFTER INSERT ON audit_records FOR EACH ROW EXECUTE FUNCTION relay_audit_activity();
CREATE FUNCTION relay_execution_activity() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 INSERT INTO activity_events(tenant_id,task_id,actor,kind,data)
 SELECT r.tenant_id,r.task_id,NEW.role,NEW.kind,jsonb_build_object('run_id',r.id,'phase',NEW.details->'phase','tool',NEW.details->'tool','error',NEW.details->'error','usage',jsonb_build_object('input_tokens',NEW.details->'usage'->'input_tokens','output_tokens',NEW.details->'usage'->'output_tokens','total_tokens',NEW.details->'usage'->'total_tokens')) FROM execution_runs r WHERE r.id=NEW.run_id;
 RETURN NEW; END; $$;
CREATE TRIGGER execution_activity AFTER INSERT ON execution_events FOR EACH ROW EXECUTE FUNCTION relay_execution_activity();
CREATE TABLE ui_pairings (code_hash TEXT PRIMARY KEY, expires_at TIMESTAMPTZ NOT NULL);
CREATE TABLE ui_sessions (token_hash TEXT PRIMARY KEY, csrf TEXT NOT NULL, expires_at TIMESTAMPTZ NOT NULL);
