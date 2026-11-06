-- Full protected results remain in immutable execution_actions.outcome.
-- References record their original authority; a summary never changes that scope.
CREATE TABLE execution_output_refs (
 id uuid PRIMARY KEY, run_id uuid NOT NULL, call_id text NOT NULL,
 role text NOT NULL, work_order_id uuid REFERENCES work_orders(id),
 sha256 text NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(run_id,call_id),
 FOREIGN KEY(run_id,call_id) REFERENCES execution_actions(run_id,call_id)
);
CREATE TRIGGER execution_output_ref_immutable BEFORE UPDATE OR DELETE ON execution_output_refs FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER execution_output_ref_no_truncate BEFORE TRUNCATE ON execution_output_refs FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TABLE execution_context_summaries (
 id uuid PRIMARY KEY, run_id uuid NOT NULL REFERENCES execution_runs(id),
 session_key text NOT NULL, source_sha256 text NOT NULL,
 summary jsonb NOT NULL, created_at timestamptz NOT NULL DEFAULT now(),
 UNIQUE(run_id,session_key,source_sha256)
);
CREATE TRIGGER execution_context_summary_immutable BEFORE UPDATE OR DELETE ON execution_context_summaries FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER execution_context_summary_no_truncate BEFORE TRUNCATE ON execution_context_summaries FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
