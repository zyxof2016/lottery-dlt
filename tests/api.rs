use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use lottery_dlt::{
    store::Store,
    web::{self, App},
};
use serde_json::{json, Value};
use tower::ServiceExt;

fn app(base: &str, protected: bool) -> axum::Router {
    let dir = std::env::temp_dir().join(format!(
        "dlt-api-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    let store = Store::open(dir.join("test.sqlite3")).unwrap();
    web::router(
        App::new(
            store,
            protected.then(|| "test-admin-token-123456".into()),
            base.into(),
        )
        .unwrap(),
    )
}
async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

async fn call(
    router: &axum::Router,
    method: &str,
    path: &str,
    input: Option<Value>,
) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("content-type", "application/json")
                .body(input.map_or_else(Body::empty, |v| Body::from(v.to_string())))
                .unwrap(),
        )
        .await
        .unwrap();
    (response.status(), json_body(response).await)
}

#[tokio::test]
async fn overview_distinguishes_empty_pending_and_a_real_zero_prize() {
    let router = app("/dlt", false);
    let (_, empty) = call(&router, "GET", "/dlt/api/overview", None).await;
    assert_eq!(empty["settled_count"], 0);
    assert!(empty["settled_prize_fen"].is_null());
    assert!(empty["settled_net_fen"].is_null());
    let (_, preview) = call(
        &router,
        "POST",
        "/dlt/api/plans/generate",
        Some(
            json!({"issue":"26119","mode":"compound","front_pool":5,"back_pool":2,
            "front_numbers":[1,2,3,4,5],"back_numbers":[1,2]}),
        ),
    )
    .await;
    let (status, _) = call(
        &router,
        "POST",
        "/dlt/api/records",
        Some(
            json!({"request":preview["request"],"preview_id":preview["id"],
            "deadline":(chrono::Utc::now()+chrono::Duration::hours(1)).to_rfc3339(),
            "label":"synthetic overview fixture"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, pending) = call(&router, "GET", "/dlt/api/overview", None).await;
    assert_eq!(pending["pending_count"], 1);
    assert!(pending["settled_prize_fen"].is_null());
    assert!(pending["settled_net_fen"].is_null());
    let (status, _) = call(
        &router,
        "POST",
        "/dlt/api/draws/import",
        Some(
            json!([{"issue":"26119","date":"2026-01-01","front":[6,7,8,9,10],
            "back":[3,4],"source":"synthetic-overview-fixture"}]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let before = call(&router, "GET", "/dlt/api/export", None).await.1;
    let (_, settled) = call(&router, "GET", "/dlt/api/overview", None).await;
    assert_eq!(settled["settled_count"], 1);
    assert_eq!(settled["settled_prize_fen"], 0);
    assert_eq!(settled["settled_net_fen"], -200);
    let after = call(&router, "GET", "/dlt/api/export", None).await.1;
    for key in [
        "records",
        "draws",
        "recommendations",
        "settings",
        "selection_evidence",
        "audit_entries_verified",
    ] {
        assert_eq!(before[key], after[key], "read changed {key}");
    }
}

#[tokio::test]
async fn persistent_jobs_and_pagination_have_real_http_results() {
    let protected = app("", true);
    assert_eq!(
        call(&protected, "GET", "/api/jobs", None).await.0,
        StatusCode::UNAUTHORIZED
    );
    let router = app("", false);
    let (status,queued)=call(&router,"POST","/api/jobs",Some(json!({"kind":"optimize","input":{"request":{"issue":"26119","bets":7},"objective":"coverage","trials":1000}}))).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    let path = format!("/api/jobs/{}", queued["id"].as_str().unwrap());
    let mut completed = None;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while std::time::Instant::now() < deadline {
        let (_, job) = call(&router, "GET", &path, None).await;
        if job["state"] == "completed" {
            completed = Some(job);
            break;
        }
        assert_ne!(job["state"], "failed");
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let job = completed.expect("job must reach a terminal state");
    assert_eq!(job["result"]["candidates"].as_array().unwrap().len(), 3);
    assert_eq!(job["progress"], 1.0);
    let (_, page) = call(&router, "GET", "/api/draws?page=1&limit=50", None).await;
    assert_eq!(page["total"], 0);
    assert_eq!(page["draws"], json!([]));
    assert_eq!(
        call(&router, "GET", "/api/draws?page=0", None).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let (_, records) = call(
        &router,
        "GET",
        "/api/records?page=1&limit=20&kind=personal",
        None,
    )
    .await;
    assert_eq!(records["total"], 0);
    let (_, recs) = call(&router, "GET", "/api/recommendations?page=1&limit=5", None).await;
    assert_eq!(recs["total"], 0);
    assert_eq!(recs["batches"], json!([]));
}

#[tokio::test]
async fn record_status_filters_before_pagination_and_includes_unpriced_draws() {
    let router = app("", false);
    let deadline = (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339();
    let mut previews = Vec::new();
    for issue in ["26119", "26120", "26121"] {
        let (status, preview) = call(
            &router,
            "POST",
            "/api/plans/generate",
            Some(json!({"issue":issue,"bets":1})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status,_)=call(&router,"POST","/api/records",Some(json!({"request":preview["request"],"preview_id":preview["id"],"deadline":deadline,"label":issue}))).await;
        assert_eq!(status, StatusCode::OK);
        previews.push(preview);
    }
    let ticket = &previews[0]["tickets"][0];
    let (status,_)=call(&router,"POST","/api/draws/import",Some(json!([{"issue":"26119","date":"2026-01-01","front":ticket["front"],"back":ticket["back"],"source":"synthetic-isolated-status-test"}]))).await;
    assert_eq!(status, StatusCode::OK);
    let before = call(&router, "GET", "/api/export", None).await.1;
    let (status, page) = call(
        &router,
        "GET",
        "/api/records?page=1&limit=1&kind=personal&status=pending",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(page["total"], 2);
    assert_eq!(page["records"][0]["plan"]["request"]["issue"], "26121");
    let (_, page) = call(
        &router,
        "GET",
        "/api/records?page=2&limit=1&kind=personal&status=pending",
        None,
    )
    .await;
    assert_eq!(page["total"], 2);
    assert_eq!(page["records"][0]["plan"]["request"]["issue"], "26120");
    let (_, page) = call(
        &router,
        "GET",
        "/api/records?page=1&limit=1&kind=personal&status=settled&issue=261",
        None,
    )
    .await;
    assert_eq!(page["total"], 1);
    assert_eq!(page["records"][0]["plan"]["request"]["issue"], "26119");
    assert!(page["records"][0]["settlement"]["total_prize_fen"].is_null());
    let (_, overview) = call(&router, "GET", "/api/overview", None).await;
    assert_eq!(overview["settled_count"], 1);
    assert!(overview["settled_prize_fen"].is_null());
    assert!(overview["settled_net_fen"].is_null());
    assert_eq!(
        call(&router, "GET", "/api/records?page=1&status=unknown", None)
            .await
            .0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let after = call(&router, "GET", "/api/export", None).await.1;
    for key in ["records", "draws", "audit_entries_verified"] {
        assert_eq!(before[key], after[key]);
    }
}

#[tokio::test]
async fn simulation_rejects_stale_preview_after_seed_change() {
    let router = app("", false);
    let (_, preview) = call(
        &router,
        "POST",
        "/api/plans/generate",
        Some(json!({"issue":"26119"})),
    )
    .await;
    let input = json!({"request":preview["request"],"preview_id":preview["id"],"trials":1000});
    let (status, report) = call(&router, "POST", "/api/plans/simulate", Some(input.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(report["plan_id"], preview["id"]);
    call(&router, "PUT", "/api/config", Some(json!({"seed":473728}))).await;
    let (status, _) = call(&router, "POST", "/api/plans/simulate", Some(input)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn saved_ticket_survives_reopen_and_settles_without_regeneration() {
    let dir = std::env::temp_dir().join(format!("dlt-workflow-{}", rand::random::<u64>()));
    let path = dir.join("isolated-test.sqlite3");
    let make_router =
        || web::router(App::new(Store::open(&path).unwrap(), None, String::new()).unwrap());
    let router = make_router();
    let req = json!({"issue":"26119","mode":"compound","front_pool":5,"back_pool":2,"front_numbers":[1,2,3,4,5],"back_numbers":[1,2]});
    let (_, preview) = call(&router, "POST", "/api/plans/generate", Some(req)).await;
    let input = json!({"request":preview["request"],"preview_id":preview["id"],"deadline":(chrono::Utc::now()+chrono::Duration::hours(1)).to_rfc3339(),"label":"isolated workflow test"});
    assert_eq!(
        call(&router, "POST", "/api/records", Some(input)).await.0,
        StatusCode::OK
    );
    assert!(
        call(&router, "GET", "/api/recommendations", None).await.1["records"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    drop(router);
    let router = make_router();
    call(&router, "PUT", "/api/config", Some(json!({"seed":243895}))).await;
    let (_, records) = call(&router, "GET", "/api/records", None).await;
    assert_eq!(records["records"][0]["plan"], preview);
    // Synthetic fixture only in this disposable test DB. No actual prediction.
    let mut draw = json!({"issue":"26119","date":"2026-01-01","front":[1,2,3,4,5],"back":[1,2],"source":"isolated_test"});
    assert_eq!(
        call(
            &router,
            "POST",
            "/api/draws/import",
            Some(json!({"draws":[draw.clone()]}))
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, records) = call(&router, "GET", "/api/records", None).await;
    let settlement = &records["records"][0]["settlement"];
    assert_eq!(settlement["winning_tickets"], 1);
    assert!(settlement["total_prize_fen"].is_null());
    draw["awards"] = json!({"1":{"basic_fen":123450000}});
    assert_eq!(
        call(&router, "POST", "/api/draws/import", Some(json!([draw])))
            .await
            .0,
        StatusCode::OK
    );
    let (_, records) = call(&router, "GET", "/api/records", None).await;
    assert_eq!(records["records"][0]["plan"], preview);
    assert_eq!(
        records["records"][0]["settlement"]["total_prize_fen"],
        123450000
    );
    assert_eq!(records["records"][0]["settlement"]["net_fen"], 123449800);
    assert_eq!(
        call(&router, "GET", "/api/export", None).await.1["records"],
        records["records"]
    );
    drop(router);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn official_response_import_and_admin_validation_are_real() {
    let router = app("", false);
    let input: Value = serde_json::from_str(include_str!("fixtures/official-25049.json")).unwrap();
    let (status, result) = call(&router, "POST", "/api/draws/import", Some(input)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(result["changed"], 1);
    let (_, result) = call(&router, "GET", "/api/draws", None).await;
    assert_eq!(result["draws"][0]["issue"], "25049");
    assert_eq!(result["draws"][0]["awards"]["9"]["basic_fen"], 500);
    assert_eq!(
        call(&app("", true), "GET", "/api/auth", None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn root_assets_and_subpath_api_work() {
    let router = app("/dlt", false);
    let response = router
        .clone()
        .oneshot(Request::builder().uri("/dlt/").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("content-security-policy").is_some());
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("/dlt/app.js"));
    assert!(!html.contains("__BASE_PATH__"));
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/dlt/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(json_body(response).await["status"], "ok");
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/dlt/api/overview")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let value = json_body(response).await;
    assert_eq!(value["record_count"], 0);
    assert_eq!(value["data_quality"]["stale"], true);
    let response = router
        .oneshot(
            Request::builder()
                .uri("/dlt/api/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn mutation_and_export_auth_and_cross_origin_checks() {
    let router = app("", true);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/config")
                .header("content-type", "application/json")
                .body(Body::from("{\"seed\":42}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/config")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-admin-token-123456")
                .header("host", "localhost:6792")
                .header("origin", "https://evil.invalid")
                .body(Body::from("{\"seed\":42}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/config")
                .header("content-type", "application/json")
                .header("authorization", "Bearer test-admin-token-123456")
                .header("host", "localhost:6792")
                .header("origin", "http://localhost:6792")
                .body(Body::from("{\"seed\":\"18446744073709551615\"}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(json_body(response).await["seed"], "18446744073709551615");
}

#[tokio::test]
async fn generation_does_not_save_and_save_uses_original_preview() {
    let router = app("", false);
    let payload = json!({"issue":"26119","strategy":"coverage","bets":7});
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/plans/generate")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let plan = json_body(response).await;
    assert_eq!(plan["front_pool"].as_array().unwrap().len(), 35);
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/records")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(json_body(response).await["records"]
        .as_array()
        .unwrap()
        .is_empty());
    let input = json!({"request":plan["request"],"preview_id":plan["id"],"deadline":(chrono::Utc::now()+chrono::Duration::hours(1)).to_rfc3339(),"label":"API test"});
    for _ in 0..2 {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/records")
                    .header("content-type", "application/json")
                    .body(Body::from(input.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            json_body(response).await["plan"]["tickets"],
            plan["tickets"]
        );
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/records")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        json_body(response).await["records"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let response=router.oneshot(Request::builder().method("POST").uri("/api/plans/generate").header("content-type","application/json").body(Body::from("{\"issue\":\"26119\",\"mode\":\"compound\",\"front_pool\":35,\"back_pool\":12}")).unwrap()).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
