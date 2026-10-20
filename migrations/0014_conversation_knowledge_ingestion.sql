-- Task sources and staged model decisions are separate from curated knowledge.
CREATE TABLE knowledge_ingestion_jobs (
 id UUID PRIMARY KEY,
 tenant_id UUID NOT NULL,
 task_id UUID NOT NULL,
 request_key UUID NOT NULL,
 goal TEXT NOT NULL,
 source_manifest JSONB NOT NULL,
 status TEXT NOT NULL DEFAULT 'loading' CHECK(status IN ('loading','queued','running','paused','interrupted','failed','completed','cancelled')),
 control TEXT NOT NULL DEFAULT 'continue' CHECK(control IN ('continue','pause','cancel')),
 connection_id TEXT,
 model TEXT,
 plan JSONB,
 checkpoint JSONB NOT NULL DEFAULT '{}',
 work_order_id UUID,
 total_units INTEGER NOT NULL DEFAULT 0,
 completed_units INTEGER NOT NULL DEFAULT 0,
 model_calls INTEGER NOT NULL DEFAULT 0,
 total_tokens BIGINT NOT NULL DEFAULT 0,
 inflight JSONB,
 error_code TEXT,
 result JSONB,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 UNIQUE(tenant_id,task_id,request_key),
 UNIQUE(tenant_id,task_id,id),
 FOREIGN KEY(tenant_id,task_id) REFERENCES tasks(tenant_id,id),
 FOREIGN KEY(tenant_id,task_id,work_order_id) REFERENCES work_orders(tenant_id,task_id,id)
);
CREATE UNIQUE INDEX knowledge_ingestion_one_active_task ON knowledge_ingestion_jobs(tenant_id,task_id) WHERE status IN ('queued','running');
CREATE TABLE knowledge_ingestion_units (
 job_id UUID NOT NULL REFERENCES knowledge_ingestion_jobs(id),
 ordinal INTEGER NOT NULL CHECK(ordinal>=0),
 source_id TEXT NOT NULL,
 locator TEXT NOT NULL,
 metadata JSONB NOT NULL,
 payload JSONB NOT NULL,
 input_sha256 TEXT NOT NULL,
 sanitized_sha256 TEXT NOT NULL,
 source_artifact_id UUID REFERENCES artifacts(id),
 status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending','running','done','failed')),
 state JSONB NOT NULL DEFAULT '{}',
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 PRIMARY KEY(job_id,ordinal)
);
CREATE TABLE knowledge_ingestion_events (
 seq BIGSERIAL PRIMARY KEY,
 job_id UUID NOT NULL REFERENCES knowledge_ingestion_jobs(id),
 ordinal INTEGER,
 actor_role TEXT NOT NULL,
 kind TEXT NOT NULL,
 details JSONB NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX knowledge_ingestion_event_job ON knowledge_ingestion_events(job_id,seq);
CREATE FUNCTION relay_ingestion_activity() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 INSERT INTO activity_events(tenant_id,task_id,actor,kind,data)
 SELECT tenant_id,task_id,NEW.actor_role,'ingestion.' || NEW.kind,
 jsonb_build_object('job_id',NEW.job_id,'ordinal',NEW.ordinal,'details',NEW.details)
 FROM knowledge_ingestion_jobs WHERE id=NEW.job_id;
 RETURN NEW;
END $$;
CREATE TRIGGER ingestion_activity AFTER INSERT ON knowledge_ingestion_events FOR EACH ROW EXECUTE FUNCTION relay_ingestion_activity();
CREATE TRIGGER knowledge_ingestion_event_immutable BEFORE UPDATE OR DELETE ON knowledge_ingestion_events FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER knowledge_ingestion_event_no_truncate BEFORE TRUNCATE ON knowledge_ingestion_events FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE FUNCTION protect_knowledge_ingestion_source() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.job_id<>OLD.job_id OR NEW.ordinal<>OLD.ordinal OR NEW.source_id<>OLD.source_id OR NEW.locator<>OLD.locator
 OR NEW.metadata<>OLD.metadata OR NEW.payload<>OLD.payload OR NEW.input_sha256<>OLD.input_sha256 OR NEW.sanitized_sha256<>OLD.sanitized_sha256
 OR (OLD.source_artifact_id IS NOT NULL AND NEW.source_artifact_id IS DISTINCT FROM OLD.source_artifact_id)
 OR (OLD.status='done' AND NEW IS DISTINCT FROM OLD) THEN
  RAISE EXCEPTION 'ingestion source and completed unit are immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER knowledge_ingestion_source_immutable BEFORE UPDATE ON knowledge_ingestion_units FOR EACH ROW EXECUTE FUNCTION protect_knowledge_ingestion_source();
CREATE TRIGGER knowledge_ingestion_source_no_delete BEFORE DELETE ON knowledge_ingestion_units FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER knowledge_ingestion_source_no_truncate BEFORE TRUNCATE ON knowledge_ingestion_units FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE FUNCTION protect_knowledge_ingestion_job() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
 IF NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.task_id<>OLD.task_id OR NEW.request_key<>OLD.request_key OR NEW.goal<>OLD.goal OR NEW.source_manifest<>OLD.source_manifest
 OR (OLD.plan IS NOT NULL AND NEW.plan IS DISTINCT FROM OLD.plan)
 OR (OLD.work_order_id IS NOT NULL AND NEW.work_order_id IS DISTINCT FROM OLD.work_order_id)
 OR (OLD.status IN ('completed','cancelled') AND NEW IS DISTINCT FROM OLD) THEN
  RAISE EXCEPTION 'ingestion identity, plan and terminal result are immutable';
 END IF;
 RETURN NEW;
END $$;
CREATE TRIGGER knowledge_ingestion_job_immutable BEFORE UPDATE ON knowledge_ingestion_jobs FOR EACH ROW EXECUTE FUNCTION protect_knowledge_ingestion_job();
CREATE TRIGGER knowledge_ingestion_job_no_delete BEFORE DELETE ON knowledge_ingestion_jobs FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER knowledge_ingestion_job_no_truncate BEFORE TRUNCATE ON knowledge_ingestion_jobs FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
