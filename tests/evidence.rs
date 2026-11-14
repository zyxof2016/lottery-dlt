use lottery_dlt::{
    digest,
    model::{Draw, Settings},
    plan::{generate, generate_with_evidence, GenerateRequest, Mode, Strategy},
    store::{SaveRequest, SavedPlan, Store},
    web::{self, App},
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
struct TempStore(Store);
impl TempStore {
    fn new() -> Self {
        Self(
            Store::open(
                std::env::temp_dir()
                    .join(format!("dlt-evidence-{}", rand::random::<u64>()))
                    .join("test.sqlite3"),
            )
            .unwrap(),
        )
    }
}
impl Drop for TempStore {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.path.parent().unwrap());
    }
}
fn history(count: usize, back_balanced: bool) -> Vec<Draw> {
    (0..count)
        .map(|i| Draw {
            issue: format!("26{:03}", i + 1),
            date: (chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
                + chrono::Duration::days(i as i64))
            .to_string(),
            front: (0..5).map(|j| ((i * 5 + j) % 35 + 1) as u8).collect(),
            back: if back_balanced {
                vec![((i * 2) % 12 + 1) as u8, ((i * 2 + 1) % 12 + 1) as u8]
            } else {
                vec![1, 2]
            },
            awards: Default::default(),
            pool_before_fen: Some(0),
            source: "synthetic-evidence-tests".into(),
        })
        .collect()
}
fn input() -> GenerateRequest {
    GenerateRequest {
        issue: "26120".into(),
        strategy: Strategy::Hot,
        front_pool: 12,
        back_pool: 6,
        window: 42,
        ..Default::default()
    }
}
fn save(store: &Store, request: GenerateRequest) -> SavedPlan {
    let plan = store.generate(&request).unwrap();
    store
        .save(
            &SaveRequest {
                request,
                preview_id: plan.id,
                deadline: (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                label: "actual test archive".into(),
            },
            "personal",
        )
        .unwrap()
}
#[test]
fn evidence_capture_preserves_all_fourteen_v14_generator_outputs() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/generator-v3-golden.json")).unwrap();
    let draws: Vec<Draw> = serde_json::from_value(fixture["history"].clone()).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let request: GenerateRequest = serde_json::from_value(case["request"].clone()).unwrap();
        let plan = generate(&request, 42, &draws).unwrap();
        let (captured, evidence) = generate_with_evidence(&request, 42, &draws).unwrap();
        assert_eq!(serde_json::to_value(plan).unwrap(), case["plan"]);
        assert_eq!(serde_json::to_value(&captured).unwrap(), case["plan"]);
        evidence.validate(&captured).unwrap();
    }
}
#[test]
fn exact_flat_scores_and_tie_cutoff_are_recorded_without_probability_claims() {
    let draws = history(42, true);
    let request = input();
    let (plan, evidence) = generate_with_evidence(&request, 42, &draws).unwrap();
    assert_eq!(evidence.front.all_scores_equal, Some(true));
    assert_eq!(evidence.back.all_scores_equal, Some(true));
    assert_eq!(evidence.front.boundary_tie, Some((12, 1, 35)));
    assert_eq!(evidence.back.boundary_tie, Some((6, 1, 12)));
    assert!(evidence
        .front
        .numbers
        .iter()
        .all(|r| r.score == Some(6) && r.tie_count == Some(35)));
    assert_eq!(
        evidence
            .front
            .numbers
            .iter()
            .filter(|r| r.in_candidate_pool)
            .count(),
        12
    );
    evidence.validate(&plan).unwrap();
    let mut extra = draws.clone();
    extra.push(Draw {
        issue: request.issue.clone(),
        date: "2026-03-01".into(),
        ..draws[0].clone()
    });
    assert_eq!(
        evidence,
        generate_with_evidence(&request, 42, &extra).unwrap().1
    );
    let draw = &extra[42];
    let review = evidence.review(Some(draw));
    for (zone, total) in [("front", 5), ("back", 2)] {
        let parts = ["covered", "in_pool_not_ticketed", "outside_pool"];
        assert_eq!(
            parts
                .iter()
                .map(|key| review[zone][key].as_array().unwrap().len())
                .sum::<usize>(),
            total
        );
    }
    assert!(evidence.review(None)["front"]["covered"].is_null());
}
#[test]
fn unscored_strategies_remain_unknown_and_manual_pools_are_not_rank_prefixes() {
    let draws = history(7, false);
    for strategy in [Strategy::Random, Strategy::Coverage, Strategy::Structural] {
        let (plan, evidence) = generate_with_evidence(
            &GenerateRequest {
                strategy,
                ..input()
            },
            42,
            &draws,
        )
        .unwrap();
        assert_eq!(evidence.front.all_scores_equal, None);
        assert!(evidence
            .front
            .numbers
            .iter()
            .all(|r| r.score.is_none() && r.tie_count.is_none()));
        evidence.validate(&plan).unwrap();
    }
    let (plan, evidence) = generate_with_evidence(
        &GenerateRequest {
            mode: Mode::Compound,
            front_pool: 5,
            back_pool: 2,
            front_numbers: vec![1, 2, 3, 4, 5],
            back_numbers: vec![1, 2],
            ..input()
        },
        42,
        &draws,
    )
    .unwrap();
    assert_eq!(evidence.front.candidate_basis, "manual_pool");
    assert_eq!(evidence.front.boundary_tie, None);
    assert_eq!(evidence.front.all_scores_equal, Some(true));
    evidence.validate(&plan).unwrap();
    let mut broken = evidence.clone();
    broken.front.numbers[0].tie_count = Some(1);
    assert!(broken.validate(&plan).is_err());
}
#[test]
fn archived_evidence_survives_seed_changes_awards_and_backup_and_detects_corruption() {
    let temp = TempStore::new();
    let store = &temp.0;
    store.update_settings(&Settings { seed: 42 }).unwrap();
    store.import(&history(7, false)).unwrap();
    let saved = save(store, input());
    let original = store.selection_evidence(&saved.id).unwrap();
    assert_eq!(original["status"], "recorded");
    store.update_settings(&Settings { seed: 999 }).unwrap();
    assert_eq!(store.selection_evidence(&saved.id).unwrap(), original);
    let draw = Draw {
        issue: "26120".into(),
        date: "2026-03-01".into(),
        front: saved.plan.tickets[0].front.clone(),
        back: saved.plan.tickets[0].back.clone(),
        ..history(1, false)[0].clone()
    };
    store.import(&[draw]).unwrap();
    let reviewed = store.selection_evidence(&saved.id).unwrap();
    assert_eq!(reviewed["evidence"], original["evidence"]);
    assert_eq!(reviewed["content_hash"], original["content_hash"]);
    assert_eq!(reviewed["review"]["draw_issue"], "26120");
    let export = store.export().unwrap();
    let backup = store.path.parent().unwrap().join("backup.sqlite3");
    assert_eq!(
        store.verified_backup(&backup).unwrap()["restore_verified"],
        true
    );
    assert_eq!(
        Store::open(&backup).unwrap().export().unwrap()["selection_evidence"],
        export["selection_evidence"]
    );
    Connection::open(&store.path)
        .unwrap()
        .execute(
            "UPDATE selection_evidence SET content_hash='broken' WHERE plan_id=?1",
            [&saved.id],
        )
        .unwrap();
    assert!(store.selection_evidence(&saved.id).is_err());
    assert!(store.record_page(1, 20, "", None).is_err());
    assert!(store.export().is_err());
}
#[test]
fn old_archives_are_not_backfilled_and_failed_evidence_inserts_roll_back_the_plan() {
    let temp = TempStore::new();
    let store = &temp.0;
    store.update_settings(&Settings { seed: 42 }).unwrap();
    let request = GenerateRequest {
        issue: "26120".into(),
        ..Default::default()
    };
    let plan = store.generate(&request).unwrap();
    let legacy = SavedPlan {
        id: plan.id.clone(),
        plan,
        kind: "personal".into(),
        label: "old original label".into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        deadline: (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
        settlement: None,
        integrity_valid: true,
        schedule: None,
    };
    let body = serde_json::to_string(&legacy).unwrap();
    Connection::open(&store.path)
        .unwrap()
        .execute(
            "INSERT INTO plans VALUES(?1,?2,?3,?4,?5)",
            params![
                legacy.id,
                legacy.plan.request.issue,
                legacy.kind,
                body,
                digest(&legacy).unwrap()
            ],
        )
        .unwrap();
    assert_eq!(
        store.selection_evidence(&legacy.id).unwrap()["status"],
        "not_recorded"
    );
    let returned = store
        .save(
            &SaveRequest {
                request,
                preview_id: legacy.id.clone(),
                deadline: legacy.deadline.clone(),
                label: "new label".into(),
            },
            "personal",
        )
        .unwrap();
    assert_eq!(returned.label, legacy.label);
    store.archive_presets("26120", &legacy.deadline).unwrap();
    assert_eq!(
        store.selection_evidence(&legacy.id).unwrap()["status"],
        "not_recorded"
    );
    assert_eq!(
        store.export().unwrap()["selection_evidence"]
            .as_object()
            .unwrap()
            .len(),
        4
    );
    let atomic = TempStore::new();
    Connection::open(&atomic.0.path).unwrap().execute_batch("CREATE TRIGGER reject_evidence BEFORE INSERT ON selection_evidence BEGIN SELECT RAISE(ABORT,'synthetic evidence failure'); END;").unwrap();
    let request = GenerateRequest {
        issue: "26120".into(),
        ..Default::default()
    };
    let plan = atomic.0.generate(&request).unwrap();
    assert!(atomic
        .0
        .save(
            &SaveRequest {
                request,
                preview_id: plan.id,
                deadline: legacy.deadline,
                label: "rollback".into()
            },
            "personal"
        )
        .is_err());
    assert!(atomic.0.records().unwrap().is_empty());
    assert_eq!(atomic.0.verify_audit().unwrap(), 0);
}
#[tokio::test]
async fn evidence_http_enforces_preview_auth_detects_stale_inputs_and_exposes_archived_evidence() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let temp = TempStore::new();
    let store = &temp.0;
    store.import(&history(7, false)).unwrap();
    let request = input();
    let preview = store.generate(&request).unwrap();
    let saved = save(store, request.clone());
    let app = web::router(
        App::new(
            store.clone(),
            Some("evidence-test-token-123456".into()),
            "/dlt".into(),
        )
        .unwrap(),
    );
    let req = || {
        Request::builder()
            .method("POST")
            .uri("/dlt/api/plans/explain")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"request":request,"preview_id":preview.id}).to_string(),
            ))
            .unwrap()
    };
    assert_eq!(
        app.clone().oneshot(req()).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    let mut authorized = req();
    authorized.headers_mut().insert(
        "authorization",
        "Bearer evidence-test-token-123456".parse().unwrap(),
    );
    assert_eq!(
        app.clone().oneshot(authorized).await.unwrap().status(),
        StatusCode::OK
    );
    store.update_settings(&Settings { seed: 0 }).unwrap();
    let mut stale = req();
    stale.headers_mut().insert(
        "authorization",
        "Bearer evidence-test-token-123456".parse().unwrap(),
    );
    assert_eq!(
        app.clone().oneshot(stale).await.unwrap().status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let uri = format!("/dlt/api/records/{}/evidence", saved.id);
    let response = app
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(body["evidence"]["front"]["all_scores_equal"], true);
    Connection::open(&store.path)
        .unwrap()
        .execute(
            "UPDATE selection_evidence SET content_hash='bad' WHERE plan_id=?1",
            [&saved.id],
        )
        .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.oneshot(
            Request::builder()
                .uri("/dlt/api/records/missing/evidence")
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
}
