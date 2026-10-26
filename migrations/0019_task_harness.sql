-- Operational history stays Task-local and is never a central knowledge item.
CREATE TABLE execution_context_archives (
    id bigserial PRIMARY KEY,
    run_id uuid NOT NULL REFERENCES execution_runs(id),
    role text NOT NULL,
    phase text NOT NULL,
    round integer NOT NULL,
    history jsonb NOT NULL,
    sha256 text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE(run_id,role,phase,round,sha256)
);
CREATE TRIGGER execution_context_archives_immutable BEFORE UPDATE OR DELETE ON execution_context_archives
FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();

CREATE TABLE worker_job_bindings (
    id uuid PRIMARY KEY,
    tenant_id uuid NOT NULL,
    task_id uuid NOT NULL REFERENCES tasks(id),
    work_order_id uuid NOT NULL REFERENCES work_orders(id),
    request_hash text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TRIGGER worker_job_bindings_immutable BEFORE UPDATE OR DELETE ON worker_job_bindings
FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TABLE worker_job_results (
    id uuid PRIMARY KEY REFERENCES worker_job_bindings(id),
    result jsonb NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE TRIGGER worker_job_results_immutable BEFORE UPDATE OR DELETE ON worker_job_results
FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER worker_job_bindings_no_truncate BEFORE TRUNCATE ON worker_job_bindings FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER worker_job_results_no_truncate BEFORE TRUNCATE ON worker_job_results FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER execution_context_archives_no_truncate BEFORE TRUNCATE ON execution_context_archives FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
