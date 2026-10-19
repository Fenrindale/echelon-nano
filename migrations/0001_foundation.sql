CREATE TABLE tenants (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE tasks (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id),
    goal TEXT NOT NULL,
    plan JSONB NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'draft',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE work_orders (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id),
    task_id UUID NOT NULL REFERENCES tasks(id),
    target TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'ready',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE observations (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id),
    task_id UUID NOT NULL REFERENCES tasks(id),
    work_order_id UUID NOT NULL REFERENCES work_orders(id),
    content JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE knowledge_items (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL REFERENCES tenants(id),
    category TEXT NOT NULL CHECK (category IN ('data','info','intel')),
    title TEXT NOT NULL,
    version INTEGER NOT NULL DEFAULT 1,
    truth_state TEXT NOT NULL DEFAULT 'unverified',
    outcome TEXT NOT NULL DEFAULT 'not_applicable',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE oauth_connections (
    id TEXT PRIMARY KEY,
    credential_version BIGINT NOT NULL,
    generation BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    plan_enabled BOOLEAN NOT NULL,
    status TEXT NOT NULL DEFAULT 'connected',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE TABLE audit_records (
    seq BIGSERIAL PRIMARY KEY,
    tenant_id UUID,
    task_id UUID,
    actor_role TEXT NOT NULL,
    action TEXT NOT NULL,
    outcome TEXT NOT NULL,
    details JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE FUNCTION reject_audit_mutation() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN RAISE EXCEPTION 'audit records are append only'; END;
$$;
CREATE TRIGGER audit_append_only BEFORE UPDATE OR DELETE ON audit_records
    FOR EACH ROW EXECUTE FUNCTION reject_audit_mutation();
