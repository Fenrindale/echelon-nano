CREATE FUNCTION guard_work_contract() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'work history is append only'; END IF;
 IF (to_jsonb(NEW)-'status')<>(to_jsonb(OLD)-'status') OR OLD.status<>'ready' OR NEW.status<>'completed'
 OR NOT EXISTS(SELECT 1 FROM work_order_results WHERE work_order_id=OLD.id AND tenant_id=OLD.tenant_id AND task_id=OLD.task_id)
 THEN RAISE EXCEPTION 'immutable work contract or invalid completion'; END IF;
 RETURN NEW; END; $$;
CREATE TRIGGER work_contract_guard BEFORE UPDATE OR DELETE ON work_orders FOR EACH ROW EXECUTE FUNCTION guard_work_contract();
CREATE TRIGGER work_no_truncate BEFORE TRUNCATE ON work_orders FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER observation_immutable BEFORE UPDATE OR DELETE ON observations FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER observation_no_truncate BEFORE TRUNCATE ON observations FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER alias_no_truncate BEFORE TRUNCATE ON knowledge_aliases FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER review_work_no_truncate BEFORE TRUNCATE ON review_work_tasks FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE FUNCTION guard_run_identity() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'run history is append only'; END IF;
 IF NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.task_id<>OLD.task_id OR NEW.request_key<>OLD.request_key OR NEW.config<>OLD.config OR NEW.created_at<>OLD.created_at OR NEW.calls<OLD.calls OR NEW.generation<OLD.generation OR OLD.status IN ('completed','cancelled')
 THEN RAISE EXCEPTION 'immutable run identity, budget or terminal checkpoint'; END IF;
 RETURN NEW; END; $$;
CREATE TRIGGER run_identity_guard BEFORE UPDATE OR DELETE ON execution_runs FOR EACH ROW EXECUTE FUNCTION guard_run_identity();
CREATE TRIGGER run_no_truncate BEFORE TRUNCATE ON execution_runs FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
