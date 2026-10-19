CREATE TABLE context_requests (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    task_id UUID NOT NULL,
    request_key UUID NOT NULL,
    requested_by UUID NOT NULL,
    goal_snapshot TEXT NOT NULL,
    criteria JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','delivered')),
    delivery JSONB,
    pack_id UUID,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id,task_id,request_key),
    UNIQUE (tenant_id,task_id,id),
    FOREIGN KEY (tenant_id,task_id) REFERENCES tasks(tenant_id,id)
);
CREATE INDEX context_request_queue ON context_requests(tenant_id,status,id);
CREATE TABLE context_packs (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    task_id UUID NOT NULL,
    context_request_id UUID NOT NULL UNIQUE,
    created_by UUID NOT NULL,
    manifest JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id,task_id,id),
    FOREIGN KEY (tenant_id,task_id,context_request_id) REFERENCES context_requests(tenant_id,task_id,id)
);
CREATE TABLE context_pack_refs (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    task_id UUID NOT NULL,
    pack_id UUID NOT NULL,
    item_id UUID NOT NULL,
    version INTEGER NOT NULL,
    document_sha256 TEXT NOT NULL,
    UNIQUE (pack_id,item_id,version),
    FOREIGN KEY (tenant_id,task_id,pack_id) REFERENCES context_packs(tenant_id,task_id,id),
    FOREIGN KEY (tenant_id,item_id,version) REFERENCES knowledge_revisions(tenant_id,item_id,version)
);
CREATE INDEX context_task_packs ON context_packs(tenant_id,task_id,id);

CREATE FUNCTION guard_context_delivery() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
    IF OLD.status <> 'pending' OR NEW.status <> 'delivered' OR NEW.pack_id IS NULL OR NEW.delivery IS NULL
       OR (to_jsonb(NEW) - ARRAY['status','delivery','pack_id'])
          IS DISTINCT FROM (to_jsonb(OLD) - ARRAY['status','delivery','pack_id']) THEN
        RAISE EXCEPTION 'context request is immutable and delivery is final';
    END IF;
    RETURN NEW;
END;
$$;
CREATE TRIGGER context_delivery_only BEFORE UPDATE ON context_requests
    FOR EACH ROW EXECUTE FUNCTION guard_context_delivery();
CREATE TRIGGER context_request_preserved BEFORE DELETE ON context_requests
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER context_request_no_truncate BEFORE TRUNCATE ON context_requests
    EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER context_pack_preserved BEFORE UPDATE OR DELETE ON context_packs
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER context_pack_no_truncate BEFORE TRUNCATE ON context_packs
    EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER context_ref_preserved BEFORE UPDATE OR DELETE ON context_pack_refs
    FOR EACH ROW EXECUTE FUNCTION reject_knowledge_history_mutation();
CREATE TRIGGER context_ref_no_truncate BEFORE TRUNCATE ON context_pack_refs
    EXECUTE FUNCTION reject_knowledge_history_mutation();
