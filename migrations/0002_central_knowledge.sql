ALTER TABLE tasks ADD CONSTRAINT tasks_tenant_identity UNIQUE (tenant_id,id);
ALTER TABLE knowledge_items ADD CONSTRAINT knowledge_tenant_identity UNIQUE (tenant_id,id);
ALTER TABLE knowledge_items ADD COLUMN updated_at TIMESTAMPTZ NOT NULL DEFAULT now();

CREATE TABLE knowledge_submissions (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id),
    source_task_id UUID NOT NULL,
    request_id UUID NOT NULL,
    submitted_by UUID NOT NULL,
    target_id UUID,
    base_version INTEGER,
    document JSONB NOT NULL,
    rationale TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','accepted','rejected')),
    decision JSONB,
    result_item_id UUID,
    result_version INTEGER,
    decided_by UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    decided_at TIMESTAMPTZ,
    UNIQUE (tenant_id,source_task_id,request_id),
    FOREIGN KEY (tenant_id,source_task_id) REFERENCES tasks(tenant_id,id),
    FOREIGN KEY (tenant_id,target_id) REFERENCES knowledge_items(tenant_id,id),
    CHECK ((target_id IS NULL) = (base_version IS NULL)),
    CHECK (base_version IS NULL OR base_version > 0)
);

CREATE TABLE knowledge_revisions (
    tenant_id UUID NOT NULL,
    item_id UUID NOT NULL,
    version INTEGER NOT NULL CHECK (version > 0),
    document JSONB NOT NULL,
    source_task_id UUID,
    submission_id UUID REFERENCES knowledge_submissions(id),
    actor_task_id UUID,
    actor_role TEXT NOT NULL,
    actor_session UUID,
    reason TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant_id,item_id,version),
    FOREIGN KEY (tenant_id,item_id) REFERENCES knowledge_items(tenant_id,id),
    FOREIGN KEY (tenant_id,source_task_id) REFERENCES tasks(tenant_id,id),
    FOREIGN KEY (tenant_id,actor_task_id) REFERENCES tasks(tenant_id,id)
);

-- Keep any Phase 0 metadata as history without inventing missing source material.
INSERT INTO knowledge_revisions(tenant_id,item_id,version,document,actor_role,reason)
SELECT tenant_id,id,version,jsonb_build_object(
    'category',category,'title',title,'truth_state',truth_state,'outcome',outcome,
    'storage_mode','reference','content','','tags','[]'::jsonb,'sources','[]'::jsonb,
    'relations','[]'::jsonb,'freshness','review_needed','applicability','',
    'assessment',NULL), 'migration','Phase 0 metadata; source material requires a Handler submission'
FROM knowledge_items;

CREATE TABLE knowledge_tags (
    tenant_id UUID NOT NULL,
    item_id UUID NOT NULL,
    version INTEGER NOT NULL,
    tag TEXT NOT NULL,
    PRIMARY KEY (tenant_id,item_id,version,tag),
    FOREIGN KEY (tenant_id,item_id,version) REFERENCES knowledge_revisions(tenant_id,item_id,version)
);
CREATE INDEX knowledge_tag_lookup ON knowledge_tags(tenant_id,tag,item_id,version);

CREATE TABLE knowledge_edges (
    tenant_id UUID NOT NULL,
    source_id UUID NOT NULL,
    source_version INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('references','derived_from','supports','contradicts','depends_on','supersedes')),
    target_id UUID NOT NULL,
    target_version INTEGER NOT NULL,
    PRIMARY KEY (tenant_id,source_id,source_version,kind,target_id,target_version),
    FOREIGN KEY (tenant_id,source_id,source_version) REFERENCES knowledge_revisions(tenant_id,item_id,version),
    FOREIGN KEY (tenant_id,target_id,target_version) REFERENCES knowledge_revisions(tenant_id,item_id,version),
    CHECK (source_id <> target_id OR source_version <> target_version)
);
CREATE INDEX knowledge_reverse_edges ON knowledge_edges(tenant_id,target_id,target_version);
CREATE INDEX knowledge_item_lookup ON knowledge_items(tenant_id,category,id);
CREATE INDEX knowledge_submission_queue ON knowledge_submissions(tenant_id,status,id);

CREATE FUNCTION reject_knowledge_history_mutation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'knowledge history is append only'; END;
$$;
CREATE TRIGGER revision_append_only BEFORE UPDATE OR DELETE ON knowledge_revisions
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER tag_append_only BEFORE UPDATE OR DELETE ON knowledge_tags
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER edge_append_only BEFORE UPDATE OR DELETE ON knowledge_edges
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER item_preserved BEFORE DELETE ON knowledge_items
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER submission_preserved BEFORE DELETE ON knowledge_submissions
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();

CREATE FUNCTION guard_submission_decision() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.status <> 'pending' OR NEW.status NOT IN ('accepted','rejected')
       OR (to_jsonb(NEW) - ARRAY['status','decision','result_item_id','result_version','decided_by','decided_at'])
          IS DISTINCT FROM (to_jsonb(OLD) - ARRAY['status','decision','result_item_id','result_version','decided_by','decided_at']) THEN
        RAISE EXCEPTION 'submission payload is immutable and decision is final';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER submission_decision_only BEFORE UPDATE ON knowledge_submissions
    FOR EACH ROW EXECUTE FUNCTION guard_submission_decision();

CREATE TRIGGER revision_no_truncate BEFORE TRUNCATE ON knowledge_revisions
    EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER tag_no_truncate BEFORE TRUNCATE ON knowledge_tags
    EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER edge_no_truncate BEFORE TRUNCATE ON knowledge_edges
    EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER item_no_truncate BEFORE TRUNCATE ON knowledge_items
    EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER submission_no_truncate BEFORE TRUNCATE ON knowledge_submissions
    EXECUTE FUNCTION reject_knowledge_history_mutation();
