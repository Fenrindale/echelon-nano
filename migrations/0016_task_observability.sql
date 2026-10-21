-- Read projections only: existing receipts, role scopes and histories are unchanged.
CREATE INDEX execution_event_run_role_cursor ON execution_events(run_id,role,seq);
CREATE INDEX ingestion_event_job_role_cursor ON knowledge_ingestion_events(job_id,actor_role,seq);
CREATE INDEX ingestion_submission_origin ON knowledge_ingestion_events((details->>'submission_id'),job_id,ordinal) WHERE kind='handler.submitted';
CREATE INDEX audit_task_cursor ON audit_records(tenant_id,task_id,seq);
CREATE INDEX submission_task_time ON knowledge_submissions(tenant_id,source_task_id,created_at,id);
