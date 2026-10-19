-- Model decisions are untrusted. Runs retain checkpoints; interrupted calls are never replayed.
ALTER TABLE work_orders ADD COLUMN contract JSONB NOT NULL DEFAULT '{}';
ALTER TABLE work_orders ADD CONSTRAINT work_orders_scope_unique UNIQUE(tenant_id,task_id,id);
CREATE TABLE work_order_results (
    work_order_id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    task_id UUID NOT NULL,
    result JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY(tenant_id,task_id,work_order_id) REFERENCES work_orders(tenant_id,task_id,id)
);
CREATE TRIGGER work_result_immutable BEFORE UPDATE OR DELETE ON work_order_results
    FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER work_result_no_truncate BEFORE TRUNCATE ON work_order_results
    FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TABLE execution_runs (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    task_id UUID NOT NULL,
    request_key UUID NOT NULL,
    config JSONB NOT NULL,
    state JSONB NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('running','completed','failed','exhausted','interrupted')),
    calls INTEGER NOT NULL DEFAULT 0,
    error_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    FOREIGN KEY(tenant_id,task_id) REFERENCES tasks(tenant_id,id),
    UNIQUE(tenant_id,task_id,request_key)
);
CREATE UNIQUE INDEX execution_one_running_task ON execution_runs(tenant_id,task_id) WHERE status='running';
CREATE TABLE execution_events (
    seq BIGSERIAL PRIMARY KEY,
    run_id UUID NOT NULL REFERENCES execution_runs(id),
    role TEXT NOT NULL,
    kind TEXT NOT NULL,
    details JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TRIGGER execution_event_immutable BEFORE UPDATE OR DELETE ON execution_events
    FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER execution_event_no_truncate BEFORE TRUNCATE ON execution_events
    FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
