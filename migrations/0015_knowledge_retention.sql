-- Placement is independent of category, truth and outcome. Legacy records await
-- review; neither historical revisions nor delivered Context Packs are rewritten.
ALTER TABLE knowledge_items ADD COLUMN retention_scope TEXT NOT NULL DEFAULT 'review_needed'
    CHECK (retention_scope IN ('central','task','review_needed'));
CREATE INDEX knowledge_retention_lookup ON knowledge_items(tenant_id,retention_scope,id);
CREATE TABLE knowledge_curation_effects (
    tenant_id UUID NOT NULL,
    request_key UUID NOT NULL,
    payload JSONB NOT NULL,
    result JSONB NOT NULL,
    PRIMARY KEY(tenant_id,request_key)
);
CREATE TRIGGER curation_append_only BEFORE UPDATE OR DELETE ON knowledge_curation_effects
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER curation_no_truncate BEFORE TRUNCATE ON knowledge_curation_effects
    EXECUTE FUNCTION reject_knowledge_history_mutation();
ALTER TABLE knowledge_submissions DROP CONSTRAINT knowledge_submissions_status_check;
ALTER TABLE knowledge_submissions ADD CONSTRAINT knowledge_submissions_status_check
    CHECK (status IN ('pending','accepted','local','rejected'));
CREATE OR REPLACE FUNCTION guard_submission_decision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.status <> 'pending' OR NEW.status NOT IN ('accepted','local','rejected')
       OR (to_jsonb(NEW) - ARRAY['status','decision','result_item_id','result_version','decided_by','decided_at'])
          IS DISTINCT FROM (to_jsonb(OLD) - ARRAY['status','decision','result_item_id','result_version','decided_by','decided_at']) THEN
        RAISE EXCEPTION 'submission payload is immutable and decision is final';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TABLE knowledge_retention_jobs (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    task_id UUID NOT NULL,
    request_key UUID NOT NULL,
    connection_id TEXT NOT NULL,
    model TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued','running','paused','completed','failed')),
    control TEXT NOT NULL DEFAULT 'continue' CHECK (control IN ('continue','pause')),
    model_calls INTEGER NOT NULL DEFAULT 0,
    total_tokens BIGINT NOT NULL DEFAULT 0,
    error_code TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(tenant_id,task_id,request_key),
    FOREIGN KEY(tenant_id,task_id) REFERENCES tasks(tenant_id,id)
);
CREATE UNIQUE INDEX one_retention_review_per_tenant ON knowledge_retention_jobs(tenant_id)
    WHERE status IN ('queued','running','paused');
CREATE TABLE knowledge_retention_units (
    job_id UUID NOT NULL REFERENCES knowledge_retention_jobs(id),
    ordinal INTEGER NOT NULL,
    snapshot JSONB NOT NULL,
    response JSONB,
    result JSONB,
    PRIMARY KEY(job_id,ordinal)
);
CREATE FUNCTION guard_retention_unit() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.job_id<>NEW.job_id OR OLD.ordinal<>NEW.ordinal OR OLD.snapshot IS DISTINCT FROM NEW.snapshot
       OR (OLD.response IS NOT NULL AND OLD.response IS DISTINCT FROM NEW.response)
       OR (OLD.result IS NOT NULL AND OLD.result IS DISTINCT FROM NEW.result) THEN
        RAISE EXCEPTION 'retention inputs and committed decisions are immutable';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER retention_unit_guard BEFORE UPDATE ON knowledge_retention_units
    FOR EACH ROW EXECUTE FUNCTION guard_retention_unit();
CREATE TRIGGER retention_unit_preserved BEFORE DELETE ON knowledge_retention_units
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER retention_unit_no_truncate BEFORE TRUNCATE ON knowledge_retention_units
    EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER retention_job_preserved BEFORE DELETE ON knowledge_retention_jobs
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
