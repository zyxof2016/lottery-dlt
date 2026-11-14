use lottery_dlt::{
    digest,
    model::{Award, Draw, Settings},
    plan::{generate, presets, GenerateRequest},
    store::{RecommendationBatch, RecommendationEntry, SavedPlan, Store},
    web::{self, App},
};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

struct Fixture(Store);
impl Fixture {
    fn new() -> Self {
        Self(
            Store::open(
                std::env::temp_dir()
                    .join(format!("dlt-review-{}", rand::random::<u64>()))
                    .join("test.sqlite3"),
            )
            .unwrap(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.path.parent().unwrap());
    }
}
fn insert_record(store: &Store, record: &SavedPlan) {
    Connection::open(&store.path)
        .unwrap()
        .execute(
            "INSERT INTO plans VALUES(?1,?2,?3,?4,?5)",
            params![
                record.id,
                record.plan.request.issue,
                record.kind,
                serde_json::to_string(record).unwrap(),
                digest(record).unwrap()
            ],
        )
        .unwrap();
}
// Historical archives below are explicitly synthetic test fixtures in a new DB.
fn batch(store: &Store, issue: &str, date: &str) -> Vec<SavedPlan> {
    let records: Vec<_> = presets(issue)
        .into_iter()
        .map(|(label, request)| {
            let plan = generate(&request, 42, &[]).unwrap();
            let saved = SavedPlan {
                id: plan.id.clone(),
                label: label.into(),
                kind: "recommendation".into(),
                created_at: format!("{date}T10:00:00Z"),
                deadline: format!("{date}T13:00:00Z"),
                plan,
                settlement: None,
                integrity_valid: true,
                schedule: None,
            };
            insert_record(store, &saved);
            saved
        })
        .collect();
    let batch = RecommendationBatch {
        issue: issue.into(),
        created_at: format!("{date}T11:00:00Z"),
        deadline: format!("{date}T13:00:00Z"),
        entries: records
            .iter()
            .map(|r| RecommendationEntry {
                label: r.label.clone(),
                plan_id: r.id.clone(),
            })
            .collect(),
    };
    Connection::open(&store.path)
        .unwrap()
        .execute(
            "INSERT INTO recommendation_batches VALUES(?1,?2,?3)",
            params![
                issue,
                serde_json::to_string(&batch).unwrap(),
                digest(&batch).unwrap()
            ],
        )
        .unwrap();
    records
}
fn draw(issue: &str, date: &str, record: &SavedPlan) -> Draw {
    Draw {
        issue: issue.into(),
        date: date.into(),
        front: record.plan.tickets[0].front.clone(),
        back: record.plan.tickets[0].back.clone(),
        awards: Default::default(),
        pool_before_fen: Some(0),
        source: "synthetic-test-only".into(),
    }
}
#[test]
fn review_keeps_pending_missing_and_unknown_money_separate_and_is_read_only() {
    let f = Fixture::new();
    let store = &f.0;
    let records = batch(store, "26030", "2026-03-01");
    batch(store, "26032", "2026-03-05");
    let winning = draw("26030", "2026-03-01", &records[2]);
    let gap = draw("26031", "2026-03-03", &records[0]);
    store.import(&[winning.clone(), gap]).unwrap();
    let before = store.export().unwrap();
    let r = store.live_review(30).unwrap();
    assert_eq!(r["period_count"], 3);
    assert_eq!(r["periods"][0]["status"], "pending");
    assert_eq!(r["periods"][1]["status"], "missed");
    assert_eq!(r["periods"][2]["status"], "prize_pending");
    assert_eq!(r["summary"]["archived_periods"], 2);
    assert_eq!(r["summary"]["settled_records"], 5);
    assert_eq!(r["summary"]["pending_records"], 5);
    assert_eq!(r["summary"]["settled_cost_fen"], 8800);
    assert_eq!(r["summary"]["pending_cost_fen"], 8800);
    assert!(r["summary"]["net_fen"].is_null());
    let pending = &r["periods"][0]["entries"][0];
    assert!(pending["front_union_hits"].is_null() && pending["settlement"].is_null());
    let compound = &r["periods"][2]["entries"][2];
    assert_eq!(compound["front_union_hits"], 5);
    assert_eq!(compound["back_union_hits"], 2);
    assert!(compound["settlement"].get("tickets").is_none());
    assert_eq!(r, store.live_review(30).unwrap());
    let after = store.export().unwrap();
    for key in [
        "records",
        "recommendations",
        "audit_entries_verified",
        "settings",
        "draws",
    ] {
        assert_eq!(before[key], after[key]);
    }
    store.update_settings(&Settings { seed: 999 }).unwrap();
    assert_eq!(r, store.live_review(30).unwrap());
    let mut enriched = winning;
    enriched.awards.insert(
        1,
        Award {
            basic_fen: Some(1_000_000_000),
            additional_fen: None,
        },
    );
    enriched.awards.insert(
        2,
        Award {
            basic_fen: Some(10_000_000),
            additional_fen: None,
        },
    );
    store.import(&[enriched]).unwrap();
    let updated = store.live_review(30).unwrap();
    assert_eq!(updated["periods"][2]["status"], "settled");
    assert!(updated["summary"]["net_fen"].is_i64());
    assert_ne!(updated["content_hash"], r["content_hash"]);
}
#[test]
fn comparisons_use_only_common_settled_equal_budget_archives() {
    let f = Fixture::new();
    let records = batch(&f.0, "26030", "2026-03-01");
    batch(&f.0, "26031", "2026-03-03");
    f.0.import(&[draw("26030", "2026-03-01", &records[2])])
        .unwrap();
    let r = f.0.live_review(30).unwrap();
    let matching: Vec<_> = r["comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["common_periods"] == 1)
        .collect();
    assert_eq!(matching.len(), 1);
    let pair = &matching[0]["pairs"][0];
    assert_eq!(pair["cost_fen_each"], 1200);
    assert_eq!(pair["issue"], "26030");
    assert!(pair["net_delta_fen"].is_null());
    assert!(matching[0]["total_net_delta_fen"].is_null());
    let none = r["comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["common_periods"] == 0)
        .unwrap();
    assert!(none["mean_win_delta"].is_null());
    assert!(none["total_net_delta_fen"].is_null());
    assert_eq!(none["budget_mismatch_issues"], json!(["26030"]));
    assert_eq!(f.0.live_review(1).unwrap()["summary"]["settled_records"], 0);
    assert!(f.0.live_review(0).is_err() && f.0.live_review(121).is_err());
}

#[test]
fn budget_increments_preserve_pending_and_missing_awards_without_rewriting_archives() {
    let f = Fixture::new();
    let records = batch(&f.0, "26030", "2026-03-01");
    batch(&f.0, "26032", "2026-03-05");
    f.0.import(&[
        draw("26030", "2026-03-01", &records[2]),
        draw("26031", "2026-03-03", &records[0]),
    ])
    .unwrap();
    let original = f.0.export().unwrap();
    let report = f.0.live_review(30).unwrap();
    let pairs = report["budget_increments"]["comparisons"]
        .as_array()
        .unwrap();
    assert_eq!(pairs.len(), 10);
    for row in pairs {
        assert_eq!(row["common_periods"], 1);
        assert_eq!(row["pending_periods"], 1);
        assert_eq!(row["issues"], json!(["26030"]));
    }
    let unpriced = pairs
        .iter()
        .find(|r| r["priced_common_periods"] == 0)
        .unwrap();
    assert!(unpriced["net_delta_fen"].is_null());
    assert!(report["periods"][1]["budget_comparisons"]
        .as_array()
        .unwrap()
        .is_empty());
    let after = f.0.export().unwrap();
    for key in [
        "records",
        "recommendations",
        "draws",
        "audit_entries_verified",
    ] {
        assert_eq!(original[key], after[key]);
    }
    assert_eq!(report, f.0.live_review(30).unwrap());
}
#[test]
fn unbatched_and_late_or_date_mismatched_archives_do_not_enter_results() {
    let f = Fixture::new();
    let records = batch(&f.0, "26030", "2026-03-01");
    let conn = Connection::open(&f.0.path).unwrap();
    let mut late = records[0].clone();
    late.created_at = "2026-03-01T14:00:00Z".into();
    conn.execute(
        "UPDATE plans SET body=?1,content_hash=?2 WHERE id=?3",
        params![
            serde_json::to_string(&late).unwrap(),
            digest(&late).unwrap(),
            late.id
        ],
    )
    .unwrap();
    f.0.import(&[draw("26030", "2026-03-01", &records[2])])
        .unwrap();
    let r = f.0.live_review(30).unwrap();
    assert_eq!(r["periods"][0]["status"], "excluded");
    assert_eq!(r["summary"]["settled_records"], 4);
    assert_eq!(
        r["periods"][0]["source_comparisons"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert!(r["periods"][0]["source_comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["from_plan_id"] != records[0].id && p["to_plan_id"] != records[0].id));
    let g = Fixture::new();
    let records = batch(&g.0, "26030", "2026-03-01");
    g.0.import(&[draw("26030", "2026-03-02", &records[0])])
        .unwrap();
    assert_eq!(
        g.0.live_review(30).unwrap()["summary"]["settled_records"],
        0
    );
    Connection::open(&g.0.path)
        .unwrap()
        .execute("DELETE FROM recommendation_batches", [])
        .unwrap();
    let r = g.0.live_review(30).unwrap();
    assert_eq!(r["summary"]["archived_records"], 0);
    assert_eq!(r["periods"][0]["unbatched_records"], 5);
}
#[test]
fn personal_records_cannot_displace_recommendations_and_personal_references_are_kept() {
    let f = Fixture::new();
    let records = batch(&f.0, "26030", "2026-03-01");
    let conn = Connection::open(&f.0.path).unwrap();
    let mut reused = records[0].clone();
    reused.kind = "personal".into();
    conn.execute(
        "UPDATE plans SET kind='personal',body=?1,content_hash=?2 WHERE id=?3",
        params![
            serde_json::to_string(&reused).unwrap(),
            digest(&reused).unwrap(),
            reused.id
        ],
    )
    .unwrap();
    for seed in 0..55 {
        let plan = generate(
            &GenerateRequest {
                issue: "26030".into(),
                bets: 1,
                ..Default::default()
            },
            seed,
            &[],
        )
        .unwrap();
        insert_record(
            &f.0,
            &SavedPlan {
                id: plan.id.clone(),
                plan,
                kind: "personal".into(),
                ..reused.clone()
            },
        );
    }
    let page = f.0.recommendation_page(1, 5, "").unwrap();
    assert_eq!(page["total"], 1);
    assert_eq!(page["records"].as_array().unwrap().len(), 5);
    assert!(page["records"]
        .as_array()
        .unwrap()
        .iter()
        .any(|r| r["id"] == reused.id && r["kind"] == "personal"));
}
#[tokio::test]
async fn review_api_is_public_works_under_base_path_and_fails_closed_on_corruption() {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let f = Fixture::new();
    let records = batch(&f.0, "26030", "2026-03-01");
    let router = web::router(
        App::new(
            f.0.clone(),
            Some("test-admin-token-123456".into()),
            "/dlt".into(),
        )
        .unwrap(),
    );
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/dlt/api/recommendations/live-review?limit=30")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let r: Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(r["summary"]["pending_periods"], 1);
    for path in ["/dlt/pools.js", "/dlt/reviews-ui.js"] {
        assert_eq!(
            router
                .clone()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
    Connection::open(&f.0.path)
        .unwrap()
        .execute(
            "UPDATE plans SET content_hash='corrupt' WHERE id=?1",
            [&records[0].id],
        )
        .unwrap();
    let response = router
        .oneshot(
            Request::builder()
                .uri("/dlt/api/recommendations/live-review")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[test]
fn source_overlap_and_replacements_are_read_only_and_preserve_budget_constraints() {
    let f = Fixture::new();
    let earlier = batch(&f.0, "26029", "2026-02-27");
    let records = batch(&f.0, "26030", "2026-03-01");
    batch(&f.0, "26031", "2026-03-03");
    f.0.import(&[
        draw("26029", "2026-02-27", &earlier[3]),
        draw("26030", "2026-03-01", &records[2]),
    ])
    .unwrap();
    let before = f.0.export().unwrap();
    let r = f.0.live_review(30).unwrap();
    assert_eq!(
        r["periods"][0]["source_comparisons"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    let groups = r["replacement_review"]["comparisons"].as_array().unwrap();
    assert_eq!(groups.len(), 1);
    let group = &groups[0];
    assert_eq!(group["common_periods"], 2);
    assert_eq!(group["pending_periods"], 1);
    assert_eq!(group["cost_fen_each"], 1200);
    assert_eq!(group["same_zone_sizes"], false);
    assert_eq!(group["pairs"][0]["issue"], "26029");
    assert_eq!(group["pairs"][1]["issue"], "26030");
    assert_eq!(group["pairs"][2]["issue"], "26031");
    for name in ["front", "back"] {
        let rows: Vec<_> = group["pairs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|p| p["drawn"] == true)
            .collect();
        let gained: i64 = rows
            .iter()
            .map(|p| p[name]["added_hits"].as_array().unwrap().len() as i64)
            .sum();
        let lost: i64 = rows
            .iter()
            .map(|p| p[name]["removed_hits"].as_array().unwrap().len() as i64)
            .sum();
        assert_eq!(group[name]["net_hit_delta"], gained - lost);
    }
    let pending = &r["periods"][0]["source_comparisons"][0];
    assert!(pending["front"]["added_hits"].is_null());
    assert!(pending["net_delta_fen"].is_null());
    assert_eq!(pending["from_selection_evidence"]["status"], "not_recorded");
    let old = r["periods"][1]["budget_comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| {
            p["from_plan_id"] == group["pairs"][1]["from_plan_id"]
                && p["to_plan_id"] == group["pairs"][1]["to_plan_id"]
        })
        .unwrap();
    assert_eq!(
        old["front"]["hit_delta"],
        group["pairs"][1]["front"]["hit_delta"]
    );
    let after = f.0.export().unwrap();
    for key in [
        "records",
        "recommendations",
        "selection_evidence",
        "settings",
        "draws",
        "audit_entries_verified",
    ] {
        assert_eq!(before[key], after[key]);
    }
    f.0.update_settings(&Settings { seed: 123456 }).unwrap();
    assert_eq!(r, f.0.live_review(30).unwrap());
}
#[test]
fn no_settled_archives_have_unknown_profit_instead_of_a_zero_return() {
    let f = Fixture::new();
    let empty = f.0.live_review(30).unwrap();
    assert!(empty["summary"]["total_prize_fen"].is_null());
    assert!(empty["summary"]["net_fen"].is_null());
    assert_eq!(empty["replacement_review"]["comparisons"], json!([]));
    batch(&f.0, "26030", "2026-03-01");
    let pending = f.0.live_review(30).unwrap();
    assert_eq!(pending["summary"]["settled_periods"], 0);
    assert!(pending["summary"]["net_fen"].is_null());
    let g = &pending["replacement_review"]["comparisons"][0];
    assert_eq!(g["common_periods"], 0);
    assert_eq!(g["pending_periods"], 1);
    assert!(g["front"]["net_hit_delta"].is_null());
    assert!(g["net_delta_fen"].is_null());
    assert_eq!(
        pending["strategies"][0]["hit_review"]["drawdown_status"],
        "no_settled_periods"
    );
    assert!(pending["strategies"][0]["hit_review"]["max_drawdown_fen"].is_null());
    assert!(pending["strategies"][0]["hit_review"]["mean_joint_gap"].is_null());
    assert_eq!(
        pending["periods"][0]["entries"][0]["hit_gap"]["status"],
        "pending"
    );
}
#[test]
fn hit_gap_splits_each_archive_without_writing_or_pricing_a_missing_jackpot() {
    let f = Fixture::new();
    let records = batch(&f.0, "26030", "2026-03-01");
    batch(&f.0, "26032", "2026-03-05");
    f.0.import(&[draw("26030", "2026-03-01", &records[2])])
        .unwrap();
    let before = f.0.export().unwrap();
    let report = f.0.live_review(30).unwrap();
    let after = f.0.export().unwrap();
    for key in [
        "records",
        "draws",
        "recommendations",
        "audit_entries_verified",
    ] {
        assert_eq!(before[key], after[key]);
    }
    assert_eq!(report["schema_version"], 2);
    let period = report["periods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|period| period["issue"] == "26030")
        .unwrap();
    let entry = period["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["plan_id"] == records[2].id)
        .unwrap();
    assert!(entry["hit_gap"]["jackpot_tickets"].as_u64().unwrap() >= 1);
    assert_eq!(entry["hit_gap"]["joint"]["best_gap"], 0);
    assert!(entry["hit_gap"]["prize_split"]["floating_prize_fen"].is_null());
    assert!(entry["hit_gap"]["floating_covers_cost"].is_null());
    for period in report["periods"].as_array().unwrap() {
        for entry in period["entries"].as_array().unwrap() {
            let gap = &entry["hit_gap"];
            if gap["status"] == "pending" {
                assert!(gap["joint"].is_null() && gap["jackpot_tickets"].is_null());
                continue;
            }
            for zone in ["front", "back"] {
                assert_eq!(
                    gap[zone]["outside_pool"].as_u64().unwrap()
                        + gap[zone]["in_pool_not_on_best_ticket"].as_u64().unwrap(),
                    gap[zone]["gap"].as_u64().unwrap()
                );
            }
            assert!(
                gap["joint"]["patterns"][0]["examples"]
                    .as_array()
                    .unwrap()
                    .len()
                    <= 3
            );
        }
    }
    let review = report["strategies"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["label"] == entry["label"])
        .unwrap();
    assert_eq!(review["hit_review"]["drawdown_status"], "missing_awards");
    assert!(review["hit_review"]["max_drawdown_fen"].is_null());
    assert!(review["hit_review"]["jackpot_periods"].as_u64().unwrap() >= 1);
    assert!(review["hit_review"]["pending_periods"].as_u64().unwrap() >= 1);
}
