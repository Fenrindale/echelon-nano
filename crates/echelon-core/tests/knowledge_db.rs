use echelon_core::{
    knowledge,
    policy::{Claims, Role},
    unix_time,
};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

fn actor(tenant: Uuid, task: Uuid, role: Role) -> Claims {
    Claims {
        iss: "echelon-nano".into(),
        aud: "echelon-core".into(),
        sub: "synthetic-verification".into(),
        role,
        tenant_id: tenant,
        task_id: task,
        work_order_id: None,
        targets: vec![],
        iat: unix_time(),
        exp: unix_time() + 900,
        jti: Uuid::new_v4(),
    }
}
fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}
fn source() -> Value {
    json!({"category":"data","title":"가상 기술문서 위치","storage_mode":"reference","sources":[{"uri":"https://example.com/manual","locator":"synthetic section"}],"tags":["시스템:예시","업무:검증"]})
}
async fn submit(pool: &PgPool, h: &Claims, document: Value, target: Option<(Uuid, i32)>) -> Value {
    knowledge::submit(pool,h,&json!({"op":"submit","request_id":Uuid::new_v4(),"document":document,"target_id":target.map(|t|t.0),"base_version":target.map(|t|t.1),"rationale":"Owned synthetic verification"})).await.unwrap()
}
fn acceptance(submission: &Value) -> Value {
    json!({"submission_id":submission["submission"]["id"],"decision":"accept","reason":"Submitted evidence selected for reuse"})
}
async fn accept(pool: &PgPool, a: &Claims, submission: &Value) -> Value {
    knowledge::publish(pool, a, &acceptance(submission))
        .await
        .unwrap()
}
async fn get(pool: &PgPool, a: &Claims, item: Uuid) -> Value {
    knowledge::read(pool, a, &json!({"op":"get","item_id":item}))
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "requires the isolated compose.verify.yaml PostgreSQL database"]
async fn central_knowledge_transactions_and_history() {
    let url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL required for database verification");
    let pool = PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .unwrap();
    let db: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        db, "echelon_verification",
        "Never run destructive fixture checks against a work database"
    );
    sqlx::migrate!("../../migrations").run(&pool).await.unwrap();
    let tenant = Uuid::new_v4();
    let other = Uuid::new_v4();
    let task = Uuid::new_v4();
    let later = Uuid::new_v4();
    let foreign = Uuid::new_v4();
    for t in [tenant, other] {
        sqlx::query("INSERT INTO tenants(id,name) VALUES($1,'Owned verification')")
            .bind(t)
            .execute(&pool)
            .await
            .unwrap();
    }
    for (t, task) in [(tenant, task), (tenant, later), (other, foreign)] {
        sqlx::query("INSERT INTO tasks(id,tenant_id,goal) VALUES($1,$2,'Synthetic fixture')")
            .bind(task)
            .bind(t)
            .execute(&pool)
            .await
            .unwrap();
    }
    let h = actor(tenant, task, Role::Handler);
    let a = actor(tenant, later, Role::Analyst);
    let other_a = actor(other, foreign, Role::Analyst);
    let later_h = actor(tenant, later, Role::Handler);

    let request = json!({"op":"submit","request_id":Uuid::new_v4(),"document":source(),"rationale":"Reusable reference, without copying the manual"});
    let queued = knowledge::submit(&pool, &h, &request).await.unwrap();
    assert_eq!(queued["submission"]["status"], "pending");
    assert!(
        knowledge::read(&pool, &a, &json!({})).await.unwrap()["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let replay = knowledge::submit(&pool, &h, &request).await.unwrap();
    assert_eq!(replay["submission"]["id"], queued["submission"]["id"]);
    assert_eq!(replay["replayed"], true);
    let mut changed = request.clone();
    changed["document"]["title"] = json!("Changed payload");
    assert_eq!(
        knowledge::submit(&pool, &h, &changed)
            .await
            .unwrap_err()
            .code,
        "submission_request_conflict"
    );
    assert_eq!(
        knowledge::publish(&pool, &h, &acceptance(&queued))
            .await
            .unwrap_err()
            .status
            .as_u16(),
        403
    );
    assert_eq!(
        knowledge::read(
            &pool,
            &other_a,
            &json!({"op":"submission","submission_id":queued["submission"]["id"]})
        )
        .await
        .unwrap_err()
        .status
        .as_u16(),
        404
    );
    assert_eq!(
        knowledge::submit(
            &pool,
            &later_h,
            &json!({"op":"status","submission_id":queued["submission"]["id"]})
        )
        .await
        .unwrap_err()
        .status
        .as_u16(),
        404
    );
    let adopted = accept(&pool, &a, &queued).await;
    let root = id(&adopted["submission"]["item_id"]);
    assert_eq!(adopted["submission"]["version"], 1);
    assert_eq!(
        knowledge::publish(&pool, &a, &acceptance(&queued))
            .await
            .unwrap()["replayed"],
        true
    );
    assert_eq!(
        get(&pool, &a, root).await["item"]["document"]["content"],
        ""
    );
    let mut injected = acceptance(&queued);
    injected["content"] = json!("Analyst must not invent source material");
    assert_eq!(
        knowledge::publish(&pool, &a, &injected)
            .await
            .unwrap_err()
            .code,
        "invalid_knowledge_input"
    );

    let info_doc = json!({"category":"info","title":"가상 운영 규칙","storage_mode":"excerpt","content":"A short synthetic rule.","tags":["업무:검증"],"relations":[{"kind":"derived_from","target_id":root,"target_version":1},{"kind":"depends_on","target_id":root,"target_version":1}]});
    let info_submission = submit(&pool, &h, info_doc, None).await;
    let info = id(&accept(&pool, &a, &info_submission).await["submission"]["item_id"]);
    let intel_doc = json!({"category":"intel","title":"가상 업무 판단","storage_mode":"excerpt","content":"Apply the synthetic rule in the fixture only.","relations":[{"kind":"derived_from","target_id":info,"target_version":1}]});
    let intel_submission = submit(&pool, &h, intel_doc, None).await;
    let intel = id(&accept(&pool, &a, &intel_submission).await["submission"]["item_id"]);
    assert_eq!(
        knowledge::read(
            &pool,
            &a,
            &json!({"op":"list","category":"info","tags":["업무:검증"],"query":"운영"})
        )
        .await
        .unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        knowledge::read(&pool, &a, &json!({"op":"list","query":"%"}))
            .await
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let mut failed = source();
    failed["truth_state"] = json!("refuted");
    failed["outcome"] = json!("failed");
    failed["assessment"] = json!({"conditions":"Synthetic account in fixture","attempted_action":"Read unavailable route","observed":"Denied","reason":"Fixture permission absent","avoid":"Repeat without a grant","retry_when":"New authorized scope is proven"});
    let failed_submission = submit(&pool, &h, failed, Some((root, 1))).await;
    accept(&pool, &a, &failed_submission).await;
    let current = get(&pool, &a, root).await;
    let tags = current["item"]["document"]["tags"].as_array().unwrap();
    assert!(tags.contains(&json!("실패")));
    assert!(tags.contains(&json!("거짓")));
    let warnings = get(&pool, &a, intel).await;
    assert!(
        warnings["dependency_warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["item_id"] == json!(root) && v["current_version"] == 2)
    );
    let classified=knowledge::classify(&pool,&a,&json!({"item_id":root,"expected_version":2,"category":"info","tags":["업무:검증"],"reason":"Metadata curation only"})).await.unwrap();
    assert_eq!(classified["version"], 3);
    assert_eq!(
        get(&pool, &a, root).await["item"]["document"]["truth_state"],
        "refuted"
    );
    assert!(
        get(&pool, &a, root).await["item"]["document"]["tags"]
            .as_array()
            .unwrap()
            .contains(&json!("거짓"))
    );
    assert_eq!(
        knowledge::classify(
            &pool,
            &a,
            &json!({"item_id":root,"expected_version":2,"tags":[],"reason":"Stale write"})
        )
        .await
        .unwrap_err()
        .status
        .as_u16(),
        409
    );
    assert_eq!(knowledge::classify(&pool,&a,&json!({"item_id":root,"expected_version":3,"tags":[],"content":"forged","reason":"Forbidden body edit"})).await.unwrap_err().status.as_u16(),400);
    let mut repaired = source();
    repaired["truth_state"] = json!("supported");
    repaired["outcome"] = json!("success");
    let repair = submit(&pool, &later_h, repaired, Some((root, 3))).await;
    accept(&pool, &a, &repair).await;
    let current = get(&pool, &a, root).await;
    assert_eq!(current["item"]["version"], 4);
    assert_eq!(current["negative_history"].as_array().unwrap().len(), 2);
    let old = knowledge::read(&pool, &a, &json!({"op":"get","item_id":root,"version":2}))
        .await
        .unwrap();
    assert_eq!(old["item"]["document"]["outcome"], "failed");
    assert_eq!(old["item"]["document"]["assessment"]["observed"], "Denied");
    assert_eq!(
        knowledge::read(&pool, &a, &json!({"op":"list","negative_history":true}))
            .await
            .unwrap()["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let page = knowledge::read(&pool, &a, &json!({"op":"history","item_id":root,"limit":2}))
        .await
        .unwrap();
    assert_eq!(page["revisions"].as_array().unwrap().len(), 2);
    assert_eq!(page["next_before_version"], 3);
    let next = knowledge::read(
        &pool,
        &a,
        &json!({"op":"history","item_id":root,"before_version":3}),
    )
    .await
    .unwrap();
    assert_eq!(next["revisions"][0]["version"], 2);

    // Two accepted updates must never silently overwrite the same base version.
    let one = submit(&pool, &h, source(), Some((root, 4))).await;
    let two = submit(&pool, &later_h, source(), Some((root, 4))).await;
    let first_request = acceptance(&one);
    let second_request = acceptance(&two);
    let (first, second) = tokio::join!(
        knowledge::publish(&pool, &a, &first_request),
        knowledge::publish(&pool, &a, &second_request)
    );
    assert_ne!(first.is_ok(), second.is_ok());
    let error = first.err().or(second.err()).unwrap();
    assert_eq!(error.code, "knowledge_version_conflict");
    assert_eq!(get(&pool, &a, root).await["item"]["version"], 5);
    let foreign_item_submission =
        submit(&pool, &actor(other, foreign, Role::Handler), source(), None).await;
    let foreign_item =
        id(&accept(&pool, &other_a, &foreign_item_submission).await["submission"]["item_id"]);
    let mut bad = source();
    bad["relations"] = json!([{"kind":"references","target_id":foreign_item,"target_version":1}]);
    let err=knowledge::submit(&pool,&h,&json!({"op":"submit","request_id":Uuid::new_v4(),"document":bad,"rationale":"Must reject a foreign edge"})).await.unwrap_err();
    assert_eq!(err.code, "relation_target_unavailable");
    assert_eq!(
        get(&pool, &a, root).await["item"]["document"]["sources"][0]["uri"],
        "https://example.com/manual"
    );
    let mut cyclic = source();
    cyclic["relations"] = json!([{"kind":"depends_on","target_id":info,"target_version":1}]);
    assert_eq!(knowledge::submit(&pool,&h,&json!({"op":"submit","request_id":Uuid::new_v4(),"target_id":root,"base_version":5,"document":cyclic,"rationale":"Cycle rejection"})).await.unwrap_err().code,"dependency_cycle");
    let rejected = submit(&pool, &h, source(), None).await;
    let rejection = json!({"submission_id":rejected["submission"]["id"],"decision":"reject","reason":"One-off evidence, not reusable"});
    let result = knowledge::publish(&pool, &a, &rejection).await.unwrap();
    assert_eq!(result["submission"]["status"], "rejected");
    assert!(result["submission"]["item_id"].is_null());
    assert_eq!(
        knowledge::publish(&pool, &a, &acceptance(&rejected))
            .await
            .unwrap_err()
            .code,
        "submission_already_decided"
    );
    assert_eq!(
        knowledge::publish(&pool, &a, &rejection).await.unwrap()["replayed"],
        true
    );

    for statement in [
        "UPDATE knowledge_revisions SET reason='changed' WHERE tenant_id=$1",
        "DELETE FROM knowledge_revisions WHERE tenant_id=$1",
        "DELETE FROM knowledge_items WHERE tenant_id=$1",
        "UPDATE knowledge_tags SET tag='changed' WHERE tenant_id=$1",
        "DELETE FROM knowledge_edges WHERE tenant_id=$1",
        "UPDATE knowledge_submissions SET rationale='changed' WHERE tenant_id=$1",
    ] {
        assert!(
            sqlx::query(statement)
                .bind(tenant)
                .execute(&pool)
                .await
                .is_err(),
            "History guard failed"
        );
    }
    assert!(
        sqlx::query("TRUNCATE knowledge_revisions,knowledge_tags,knowledge_edges")
            .execute(&pool)
            .await
            .is_err()
    );
    let history = knowledge::read(&pool, &a, &json!({"op":"history","item_id":root}))
        .await
        .unwrap();
    assert_eq!(history["revisions"].as_array().unwrap().len(), 5);
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_records WHERE tenant_id=$1 AND action='knowledge.decided'",
    )
    .bind(tenant)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit_count, 7);
    pool.close().await;
}
