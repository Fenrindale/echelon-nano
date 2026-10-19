ALTER TABLE knowledge_items ADD COLUMN external_model_allowed BOOLEAN NOT NULL DEFAULT true;
CREATE TABLE protocols (
 id UUID PRIMARY KEY, tenant_id UUID NOT NULL REFERENCES tenants(id), version INTEGER NOT NULL,
 codename TEXT NOT NULL, description TEXT NOT NULL, status TEXT NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(), UNIQUE(tenant_id,id), UNIQUE(tenant_id,codename)
);
CREATE TABLE protocol_revisions (
 tenant_id UUID NOT NULL, protocol_id UUID NOT NULL, version INTEGER NOT NULL,
 document JSONB NOT NULL, actor_task_id UUID NOT NULL, reason TEXT NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY(tenant_id,protocol_id,version),
 FOREIGN KEY(tenant_id,protocol_id) REFERENCES protocols(tenant_id,id),
 FOREIGN KEY(tenant_id,actor_task_id) REFERENCES tasks(tenant_id,id)
);
CREATE TABLE protocol_members (
 tenant_id UUID NOT NULL, protocol_id UUID NOT NULL, protocol_version INTEGER NOT NULL,
 item_id UUID NOT NULL, item_version INTEGER NOT NULL, required BOOLEAN NOT NULL,
 PRIMARY KEY(tenant_id,protocol_id,protocol_version,item_id,item_version),
 FOREIGN KEY(tenant_id,protocol_id,protocol_version) REFERENCES protocol_revisions(tenant_id,protocol_id,version),
 FOREIGN KEY(tenant_id,item_id,item_version) REFERENCES knowledge_revisions(tenant_id,item_id,version)
);
CREATE TRIGGER protocol_revision_immutable BEFORE UPDATE OR DELETE ON protocol_revisions FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER protocol_revision_no_truncate BEFORE TRUNCATE ON protocol_revisions FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER protocol_member_immutable BEFORE UPDATE OR DELETE ON protocol_members FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER protocol_member_no_truncate BEFORE TRUNCATE ON protocol_members FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TABLE maintenance_proposals (
 id UUID PRIMARY KEY, tenant_id UUID NOT NULL, task_id UUID NOT NULL, request_key UUID NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('revise','merge','split','protocol','verification')),
 reason TEXT NOT NULL, payload JSONB NOT NULL, status TEXT NOT NULL DEFAULT 'pending',
 decision JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 FOREIGN KEY(tenant_id,task_id) REFERENCES tasks(tenant_id,id), UNIQUE(tenant_id,task_id,request_key)
);
CREATE FUNCTION guard_maintenance_proposal() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN
 IF TG_OP='DELETE' THEN RAISE EXCEPTION 'maintenance history is append only'; END IF;
 IF OLD.status<>'pending' OR NEW.id<>OLD.id OR NEW.tenant_id<>OLD.tenant_id OR NEW.task_id<>OLD.task_id OR NEW.request_key<>OLD.request_key OR NEW.kind<>OLD.kind OR NEW.reason<>OLD.reason OR NEW.payload<>OLD.payload OR NEW.created_at<>OLD.created_at OR NEW.status NOT IN ('applied','rejected') OR NEW.decision IS NULL THEN RAISE EXCEPTION 'invalid maintenance decision'; END IF;
 RETURN NEW; END; $$;
CREATE TRIGGER maintenance_proposal_guard BEFORE UPDATE OR DELETE ON maintenance_proposals FOR EACH ROW EXECUTE FUNCTION guard_maintenance_proposal();
CREATE TRIGGER maintenance_no_truncate BEFORE TRUNCATE ON maintenance_proposals FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TABLE review_requests (
 id UUID PRIMARY KEY, tenant_id UUID NOT NULL REFERENCES tenants(id), source_proposal UUID NOT NULL REFERENCES maintenance_proposals(id),
 item_id UUID, reason TEXT NOT NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
 FOREIGN KEY(tenant_id,item_id) REFERENCES knowledge_items(tenant_id,id)
);
CREATE TRIGGER review_request_immutable BEFORE UPDATE OR DELETE ON review_requests FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
CREATE TRIGGER review_request_no_truncate BEFORE TRUNCATE ON review_requests FOR EACH STATEMENT EXECUTE FUNCTION reject_audit_mutation();
CREATE TABLE knowledge_aliases (
 tenant_id UUID NOT NULL, source_id UUID NOT NULL, target_id UUID NOT NULL, reason TEXT NOT NULL,
 created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY(tenant_id,source_id), CHECK(source_id<>target_id),
 FOREIGN KEY(tenant_id,source_id) REFERENCES knowledge_items(tenant_id,id),
 FOREIGN KEY(tenant_id,target_id) REFERENCES knowledge_items(tenant_id,id)
);
CREATE TRIGGER knowledge_alias_immutable BEFORE UPDATE OR DELETE ON knowledge_aliases FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
