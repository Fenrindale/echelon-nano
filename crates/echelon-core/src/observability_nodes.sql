WITH task_scope AS (
 SELECT * FROM tasks WHERE tenant_id=$1 AND id=$2
), used_protocols AS (
 SELECT manifest->>'protocol_id' AS id,(manifest->>'protocol_version')::int AS version FROM context_packs WHERE tenant_id=$1 AND task_id=$2 AND manifest->>'protocol_id' IS NOT NULL
 UNION SELECT contract->'protocol'->>'id',(contract->'protocol'->>'version')::int FROM work_orders WHERE tenant_id=$1 AND task_id=$2 AND contract->'protocol'->>'id' IS NOT NULL
), direct_knowledge AS (
 SELECT item_id,version FROM knowledge_revisions WHERE tenant_id=$1 AND source_task_id=$2
 UNION SELECT item_id,version FROM context_pack_refs WHERE tenant_id=$1 AND task_id=$2
 UNION SELECT m.item_id,m.item_version FROM protocol_members m JOIN used_protocols p ON p.id=m.protocol_id::text AND p.version=m.protocol_version WHERE m.tenant_id=$1
), visible_knowledge AS (
 SELECT * FROM direct_knowledge
 UNION SELECT e.target_id,e.target_version FROM knowledge_edges e
 JOIN direct_knowledge k ON k.item_id=e.source_id AND k.version=e.source_version WHERE e.tenant_id=$1
), nodes AS (
 SELECT 'task:'||id AS id,'goal' AS kind,'handler' AS role,goal AS title,
 goal AS summary,status,created_at,'[]'::jsonb AS parents,
 jsonb_build_object('completion_conditions',contract->'completion_conditions','verification',contract->'verification') AS detail
 FROM task_scope
 UNION ALL
 SELECT 'run:'||id,'run',CASE WHEN config->>'mode'='dream' THEN 'master' ELSE 'handler' END,
 CASE WHEN config->>'mode'='dream' THEN 'Master 감사 실행' ELSE '업무 실행' END,
 COALESCE(state->'result'->'handler'->>'summary',error_code,state->>'phase',''),status,created_at,
 jsonb_build_array(jsonb_build_object('id','task:'||task_id,'relation','starts')),
 jsonb_build_object('run_id',id,'phase',state->'phase','round',state->'round','error_code',error_code,'result',state->'result','session_source','execution')
 FROM execution_runs WHERE tenant_id=$1 AND task_id=$2
 UNION ALL
 SELECT 'request:'||id,'context_request','handler','중앙 지식 요청',goal_snapshot,status,created_at,
 COALESCE((SELECT jsonb_agg(jsonb_build_object('id','run:'||r.id,'relation','requests')) FROM execution_runs r
 WHERE r.tenant_id=$1 AND r.task_id=$2 AND r.state->'request'->>'id'=q.id::text),jsonb_build_array(jsonb_build_object('id','task:'||task_id,'relation','requests'))),
 jsonb_build_object('criteria',criteria,'request_id',id)
 FROM context_requests q WHERE tenant_id=$1 AND task_id=$2
 UNION ALL
 SELECT 'pack:'||id,'context_pack','analyst','Context Pack 전달',
 '고정 참조 '||(SELECT count(*) FROM context_pack_refs r WHERE r.pack_id=p.id)::text||'건','delivered',created_at,
 jsonb_build_array(jsonb_build_object('id','request:'||context_request_id,'relation','delivers'))||
 CASE WHEN manifest->>'protocol_id' IS NULL THEN '[]'::jsonb ELSE jsonb_build_array(jsonb_build_object('id','protocol:'||(manifest->>'protocol_id')||':'||(manifest->>'protocol_version'),'relation','uses')) END||
 COALESCE((SELECT jsonb_agg(jsonb_build_object('id','knowledge:'||r.item_id||':'||r.version,'relation','pins')) FROM context_pack_refs r WHERE r.pack_id=p.id AND r.tenant_id=$1),'[]'::jsonb),
 jsonb_build_object('pack_id',id,'manifest',manifest)
 FROM context_packs p WHERE tenant_id=$1 AND task_id=$2
 UNION ALL
 SELECT 'order:'||w.id,'work_order','handler',COALESCE(w.contract->>'objective','Work Order'),w.target,w.status,w.created_at,
 jsonb_build_array(jsonb_build_object('id','task:'||w.task_id,'relation','delegates'))||
 CASE WHEN w.contract->'protocol'->>'id' IS NULL THEN '[]'::jsonb ELSE jsonb_build_array(jsonb_build_object('id','protocol:'||(w.contract->'protocol'->>'id')||':'||(w.contract->'protocol'->>'version'),'relation','uses')) END||
 COALESCE((SELECT jsonb_agg(jsonb_build_object('id','artifact:'||a,'relation','grants')) FROM jsonb_array_elements_text(COALESCE(w.contract->'input_artifacts','[]')) a),'[]'::jsonb)||
 COALESCE((SELECT jsonb_agg(DISTINCT jsonb_build_object('id','pack:'||(r->>'pack_id'),'relation','uses')) FROM jsonb_array_elements(COALESCE(w.contract->'references','[]')) r),'[]'::jsonb)||
 COALESCE((SELECT jsonb_agg(DISTINCT jsonb_build_object('id','run:'||a.run_id,'relation','delegates')) FROM execution_actions a JOIN execution_runs x ON x.id=a.run_id WHERE x.tenant_id=$1 AND x.task_id=$2 AND a.request->>'tool'='agent.delegate' AND a.outcome->'result'->>'work_order_id'=w.id::text),'[]'::jsonb)||
 COALESCE((SELECT jsonb_agg(jsonb_build_object('id','ingestion:'||j.id,'relation','delegates')) FROM knowledge_ingestion_jobs j WHERE j.tenant_id=$1 AND j.task_id=$2 AND j.work_order_id=w.id),'[]'::jsonb),
 jsonb_build_object('work_order_id',w.id,'contract',w.contract)
 FROM work_orders w WHERE w.tenant_id=$1 AND w.task_id=$2
 UNION ALL
 SELECT 'result:'||r.work_order_id,'result','agent','Work Order 결과',COALESCE(r.result->>'summary',''),COALESCE(r.result->>'outcome','completed'),r.created_at,
 jsonb_build_array(jsonb_build_object('id','order:'||r.work_order_id,'relation','produces')),
 jsonb_build_object('work_order_id',r.work_order_id,'result',r.result)
 FROM work_order_results r WHERE r.tenant_id=$1 AND r.task_id=$2
 UNION ALL
 SELECT 'observation:'||id,'observation','agent','업무 관찰',COALESCE(content->>'summary',content->>'content','관찰 기록'),COALESCE(content->>'outcome','recorded'),created_at,
 jsonb_build_array(jsonb_build_object('id','order:'||work_order_id,'relation','observes')),jsonb_build_object('observation_id',id,'content',content)
 FROM observations WHERE tenant_id=$1 AND task_id=$2
 UNION ALL
 SELECT 'artifact:'||id,'artifact','agent',name,media_type,CASE WHEN work_order_id IS NULL THEN 'input' ELSE 'completed' END,created_at,
 jsonb_build_array(jsonb_build_object('id',CASE WHEN work_order_id IS NULL THEN 'task:'||task_id ELSE 'order:'||work_order_id END,'relation',CASE WHEN work_order_id IS NULL THEN 'attaches' ELSE 'produces' END)),
 jsonb_build_object('artifact_id',id,'sha256',sha256,'bytes',octet_length(body),'work_order_id',work_order_id)
 FROM artifacts WHERE tenant_id=$1 AND task_id=$2
 UNION ALL
 SELECT 'ingestion:'||id,'ingestion','handler','대화 본문 분석',completed_units::text||' / '||total_units::text||' 구간',status,created_at,
 jsonb_build_array(jsonb_build_object('id','task:'||task_id,'relation','starts')),
 jsonb_build_object('job_id',id,'completed_units',completed_units,'total_units',total_units,'error_code',error_code,'inflight',inflight,'source_manifest',source_manifest,'session_source','ingestion')
 FROM knowledge_ingestion_jobs WHERE tenant_id=$1 AND task_id=$2
 UNION ALL
 SELECT 'unit:'||u.job_id||':'||u.ordinal,'source_unit','agent','본문 구간 '||(u.ordinal+1)::text,u.locator,u.status,u.created_at,
 jsonb_build_array(jsonb_build_object('id','ingestion:'||u.job_id,'relation','analyzes'))||
 CASE WHEN u.source_artifact_id IS NULL THEN '[]'::jsonb ELSE jsonb_build_array(jsonb_build_object('id','artifact:'||u.source_artifact_id,'relation','reads')) END,
 jsonb_build_object('job_id',u.job_id,'ordinal',u.ordinal,'source_id',u.source_id,'locator',u.locator,'input_sha256',u.input_sha256,'source_artifact_id',u.source_artifact_id,'empty_reason',u.state->'empty_reason','published',u.state->'published','session_source','ingestion')
 FROM knowledge_ingestion_units u JOIN knowledge_ingestion_jobs j ON j.id=u.job_id WHERE j.tenant_id=$1 AND j.task_id=$2
 UNION ALL
 SELECT 'submission:'||s.id,'submission','handler',s.document->>'title',s.rationale,s.status,s.created_at,
 COALESCE((SELECT jsonb_agg(DISTINCT jsonb_build_object('id','unit:'||e.job_id||':'||e.ordinal,'relation','submits'))
 FROM knowledge_ingestion_events e JOIN knowledge_ingestion_jobs j ON j.id=e.job_id WHERE j.tenant_id=$1 AND j.task_id=$2 AND e.kind='handler.submitted' AND e.details->>'submission_id'=s.id::text AND e.ordinal IS NOT NULL),
 jsonb_build_array(jsonb_build_object('id','task:'||s.source_task_id,'relation','submits'))),
 jsonb_build_object('submission_id',s.id,'category',s.document->'category','decision',s.decision,'decided_at',s.decided_at,'result_item_id',s.result_item_id,'result_version',s.result_version)
 FROM knowledge_submissions s WHERE s.tenant_id=$1 AND s.source_task_id=$2
 UNION ALL
 SELECT 'decision:'||id,'decision','analyst',CASE status WHEN 'accepted' THEN '중앙 지식 채택' WHEN 'local' THEN 'Task 지역 보존' ELSE '지식 제출 반려' END,
 COALESCE(decision->>'reason',''),status,decided_at,
 jsonb_build_array(jsonb_build_object('id','submission:'||id,'relation','decides')),
 jsonb_build_object('submission_id',id,'decision',decision,'item_id',result_item_id,'version',result_version)
 FROM knowledge_submissions WHERE tenant_id=$1 AND source_task_id=$2 AND decided_at IS NOT NULL
 UNION ALL
 SELECT 'knowledge:'||r.item_id||':'||r.version,'knowledge','analyst',r.document->>'title',
 COALESCE(r.document->>'applicability',''),r.document->>'truth_state',r.created_at,
 COALESCE((SELECT jsonb_agg(jsonb_build_object('id','decision:'||s.id,'relation','adopts')) FROM knowledge_submissions s WHERE s.tenant_id=$1 AND s.source_task_id=$2 AND s.id=r.submission_id),'[]'::jsonb)||
 COALESCE((SELECT jsonb_agg(jsonb_build_object('id','knowledge:'||e.target_id||':'||e.target_version,'relation',e.kind)) FROM knowledge_edges e WHERE e.tenant_id=$1 AND e.source_id=r.item_id AND e.source_version=r.version),'[]'::jsonb),
 jsonb_build_object('item_id',r.item_id,'version',r.version,'category',r.document->'category','outcome',r.document->'outcome','truth_state',r.document->'truth_state','retention_scope',k.retention_scope,'tags',r.document->'tags','sources',r.document->'sources','assessment',r.document->'assessment','reason',r.reason)
 FROM visible_knowledge v JOIN knowledge_revisions r ON r.tenant_id=$1 AND r.item_id=v.item_id AND r.version=v.version JOIN knowledge_items k ON k.tenant_id=r.tenant_id AND k.id=r.item_id
 UNION ALL
 SELECT 'review:'||id,'review','analyst','중앙 지식 재심사',COALESCE(error_code,'재사용 판단 기준에 따른 보관 범위 심사'),status,created_at,
 jsonb_build_array(jsonb_build_object('id','task:'||task_id,'relation','reviews')),jsonb_build_object('job_id',id,'error_code',error_code,'session_source','retention')
 FROM knowledge_retention_jobs WHERE tenant_id=$1 AND task_id=$2
 UNION ALL
 SELECT 'protocol:'||r.protocol_id||':'||r.version,'protocol','master','프로토콜:'||COALESCE(r.document->>'codename',''),COALESCE(r.document->>'description',''),COALESCE(r.document->>'status','recorded'),r.created_at,
 COALESCE((SELECT jsonb_agg(jsonb_build_object('id','knowledge:'||m.item_id||':'||m.item_version,'relation','pins')) FROM protocol_members m WHERE m.tenant_id=$1 AND m.protocol_id=r.protocol_id AND m.protocol_version=r.version),'[]'::jsonb),
 jsonb_build_object('protocol_id',r.protocol_id,'version',r.version,'document',r.document)
 FROM used_protocols p JOIN protocol_revisions r ON r.tenant_id=$1 AND r.protocol_id::text=p.id AND r.version=p.version
 UNION ALL
 SELECT 'maintenance:'||id,'maintenance','master','감사 제안 · '||kind,reason,status,created_at,
 jsonb_build_array(jsonb_build_object('id','task:'||task_id,'relation','audits')),jsonb_build_object('proposal_id',id,'kind',kind,'decision',decision)
 FROM maintenance_proposals WHERE tenant_id=$1 AND task_id=$2
 UNION ALL
 SELECT 'ingestion_event:'||e.seq,'milestone',e.actor_role,e.kind,COALESCE(e.details->>'code',e.details->>'reason',''),COALESCE(e.details->>'status',e.kind),e.created_at,
 jsonb_build_array(jsonb_build_object('id','ingestion:'||j.id,'relation','changes')),
 jsonb_build_object('job_id',j.id,'event',e.details,'session_source','ingestion','ordinal',e.ordinal)
 FROM knowledge_ingestion_events e JOIN knowledge_ingestion_jobs j ON j.id=e.job_id WHERE j.tenant_id=$1 AND j.task_id=$2 AND e.kind IN ('ingestion.stopped','ingestion.resumed','ingestion.interrupted','ingestion.completed','handler.planned')
 UNION ALL
 SELECT 'milestone:'||e.seq,'milestone',e.role,COALESCE(e.details->>'tool',e.kind),COALESCE(a.request->'input'->>'reason',e.details->>'reason',e.details->>'code',''),e.kind,e.created_at,
 jsonb_build_array(jsonb_build_object('id','run:'||r.id,'relation','changes')),
 jsonb_build_object('run_id',r.id,'event',e.details,'request',a.request,'outcome',a.outcome,'session_source','execution')
 FROM execution_events e JOIN execution_runs r ON r.id=e.run_id LEFT JOIN execution_actions a ON a.run_id=e.run_id AND a.call_id=e.details->>'call_id'
 WHERE r.tenant_id=$1 AND r.task_id=$2 AND (e.kind IN ('execution.controlled','execution.round_started') OR (e.kind='tool.completed' AND e.details->>'tool' IN ('run.replan','run.request_input','run.finish','run.configure_contract','task.plan')) OR e.kind IN ('tool.failed','tool.denied'))
)
