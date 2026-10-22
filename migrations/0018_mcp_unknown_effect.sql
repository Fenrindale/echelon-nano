CREATE UNIQUE INDEX mcp_unresolved_effect ON mcp_call_receipts(tenant_id,task_id,request_hash) WHERE status IN ('pending','unknown');
