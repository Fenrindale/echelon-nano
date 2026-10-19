CREATE INDEX knowledge_negative_history ON knowledge_revisions(tenant_id,item_id,version DESC)
    WHERE document->>'truth_state'='refuted' OR document->>'outcome'='failed';
