CREATE TABLE task_inputs (
 id UUID PRIMARY KEY,tenant_id UUID NOT NULL,task_id UUID NOT NULL,content TEXT NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),FOREIGN KEY(tenant_id,task_id) REFERENCES tasks(tenant_id,id)
);
ALTER TABLE execution_runs DROP CONSTRAINT execution_runs_status_check;
ALTER TABLE execution_runs ADD CONSTRAINT execution_runs_status_check CHECK(status IN ('queued','running','completed','failed','exhausted','interrupted','paused','cancelled'));
DROP INDEX execution_one_running_task;
CREATE UNIQUE INDEX execution_one_running_task ON execution_runs(tenant_id,task_id) WHERE status IN ('queued','running');
CREATE TRIGGER task_input_immutable BEFORE UPDATE OR DELETE ON task_inputs FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER task_input_no_truncate BEFORE TRUNCATE ON task_inputs FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE INDEX activity_scope_replay ON activity_events(tenant_id,task_id,seq);
CREATE FUNCTION guard_effect_receipt() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'effect receipt is append only'; END IF;
 IF TG_TABLE_NAME='execution_actions' THEN
  IF OLD.outcome IS NOT NULL OR NEW.run_id<>OLD.run_id OR NEW.call_id<>OLD.call_id OR NEW.request<>OLD.request THEN RAISE EXCEPTION 'immutable execution receipt'; END IF;
 ELSE
  IF OLD.status='completed' OR NEW.tenant_id<>OLD.tenant_id OR NEW.task_id<>OLD.task_id OR NEW.work_order_id<>OLD.work_order_id OR NEW.request_key<>OLD.request_key OR NEW.request_hash<>OLD.request_hash OR NEW.tool<>OLD.tool THEN RAISE EXCEPTION 'immutable tool receipt'; END IF;
 END IF;
 RETURN NEW; END; $$;
CREATE TRIGGER execution_action_guard BEFORE UPDATE OR DELETE ON execution_actions FOR EACH ROW EXECUTE FUNCTION guard_effect_receipt();
CREATE TRIGGER execution_action_no_truncate BEFORE TRUNCATE ON execution_actions FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER tool_receipt_guard BEFORE UPDATE OR DELETE ON tool_receipts FOR EACH ROW EXECUTE FUNCTION guard_effect_receipt();
CREATE TRIGGER tool_receipt_no_truncate BEFORE TRUNCATE ON tool_receipts FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TABLE tenant_settings (
 tenant_id UUID PRIMARY KEY REFERENCES tenants(id),accent TEXT NOT NULL DEFAULT '#78cfbf',
 preferences JSONB NOT NULL DEFAULT '{}'
);
CREATE UNIQUE INDEX tenant_administration_task ON tasks(tenant_id) WHERE contract->>'kind'='administration';
CREATE TABLE provider_connections (
 id UUID PRIMARY KEY,tenant_id UUID NOT NULL REFERENCES tenants(id),name TEXT NOT NULL,version BIGINT NOT NULL,
 enabled BOOLEAN NOT NULL DEFAULT true,created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE review_work_tasks (
 tenant_id UUID NOT NULL,review_id UUID NOT NULL REFERENCES review_requests(id),task_id UUID NOT NULL,
 PRIMARY KEY(tenant_id,review_id),FOREIGN KEY(tenant_id,task_id) REFERENCES tasks(tenant_id,id)
);
CREATE TRIGGER review_work_immutable BEFORE UPDATE OR DELETE ON review_work_tasks FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
