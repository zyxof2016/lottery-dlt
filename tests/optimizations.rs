use anyhow::Result;
use chrono::{Duration, Utc};
use lottery_dlt::{
    analysis::{self, BacktestRequest},
    model::{Draw, Settings},
    operations::{self, Schedule},
    optimizer::{self, OptimizeRequest},
    plan::{self, GenerateRequest, Mode, Strategy},
    research::{self, ResearchRequest},
    store::{SaveRequest, Store},
};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf};
struct Db {
    store: Store,
    root: PathBuf,
}
impl Db {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("dlt-optimization-{}", rand::random::<u64>()));
        let store = Store::open(root.join("test.sqlite3")).unwrap();
        Self { store, root }
    }
}
impl Drop for Db {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn history(n: usize) -> Vec<Draw> {
    let start = chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    (0..n)
        .map(|i| Draw {
            issue: format!("25{:03}", i + 1),
            date: (start + Duration::days(i as i64)).to_string(),
            front: (0..5).map(|j| ((i * 7 + j * 3) % 35 + 1) as u8).collect(),
            back: vec![(i % 12 + 1) as u8, ((i + 5) % 12 + 1) as u8],
            awards: BTreeMap::new(),
            pool_before_fen: None,
            source: "isolated_test".into(),
        })
        .collect()
}
fn req() -> GenerateRequest {
    GenerateRequest {
        issue: "26119".into(),
        ..Default::default()
    }
}
fn saved(store: &Store) -> Result<()> {
    let req = req();
    let plan = store.generate(&req)?;
    store.save(
        &SaveRequest {
            request: req,
            preview_id: plan.id,
            deadline: (Utc::now() + Duration::hours(1)).to_rfc3339(),
            label: "隔离验证".into(),
        },
        "personal",
    )?;
    Ok(())
}
#[test]
fn financial_options_preserve_numbers_across_all_modes_and_history_free_strategies() {
    for mode in [Mode::Single, Mode::Compound, Mode::Dantuo] {
        let request = GenerateRequest {
            mode,
            front_pool: 7,
            back_pool: 3,
            front_banker_count: 2,
            back_banker_count: 1,
            ..req()
        };
        let base = plan::generate(&request, 42, &history(10)).unwrap();
        for (additional, multiplier) in [(false, 3), (true, 1), (true, 3)] {
            let altered = plan::generate(
                &GenerateRequest {
                    additional,
                    multiplier,
                    ..request.clone()
                },
                42,
                &history(10),
            )
            .unwrap();
            assert_eq!(base.tickets, altered.tickets);
            assert_ne!(base.id, altered.id);
            assert_eq!(
                altered.cost_fen,
                base.tickets.len() as u64 * if additional { 300 } else { 200 } * multiplier as u64
            );
        }
    }
    for strategy in [Strategy::Random, Strategy::Coverage, Strategy::Structural] {
        let r = GenerateRequest {
            strategy,
            bets: 7,
            ..req()
        };
        assert_eq!(
            plan::generate(&r, 42, &history(10)).unwrap().tickets,
            plan::generate(&r, 42, &history(20)).unwrap().tickets
        );
    }
}
#[test]
fn complete_backtests_replay_exact_tickets_for_compound_dantuo_and_manual_pools() {
    let history = history(50);
    for mode in [Mode::Compound, Mode::Dantuo] {
        let config = BacktestRequest {
            mode,
            strategy: Strategy::Random,
            front_pool: 7,
            back_pool: 3,
            front_banker_count: 2,
            back_banker_count: 1,
            warmup: 10,
            limit: 8,
            window: 10,
            multiplier: 3,
            additional: true,
            ..Default::default()
        };
        let report = analysis::backtest(&history, &config).unwrap();
        for trace in report["trace"].as_array().unwrap() {
            let target = history
                .iter()
                .position(|d| d.issue == trace["issue"].as_str().unwrap())
                .unwrap();
            let expected = plan::generate(
                &config.template(&history[target].issue),
                config.seed,
                &history[target - 10..target],
            )
            .unwrap();
            assert_eq!(
                trace["tickets"],
                serde_json::to_value(expected.tickets).unwrap()
            );
            assert_eq!(trace["cost_fen"], expected.cost_fen);
        }
    }
    let config = BacktestRequest {
        mode: Mode::Compound,
        strategy: Strategy::Random,
        front_pool: 5,
        back_pool: 2,
        front_numbers: vec![1, 2, 3, 4, 5],
        back_numbers: vec![1, 2],
        warmup: 10,
        limit: 8,
        ..Default::default()
    };
    let report = analysis::backtest(&history, &config).unwrap();
    assert_eq!(report["actual_bets"], 1);
    assert_eq!(
        report["trace"][0]["tickets"][0]["front"],
        json!([1, 2, 3, 4, 5])
    );
}
#[test]
fn strict_schedule_refuses_unknown_stale_and_changed_deadline() {
    let db = Db::new();
    let mut draw = history(1).remove(0);
    draw.issue = "26118".into();
    draw.date = Utc::now().date_naive().to_string();
    db.store.import(&[draw]).unwrap();
    db.store.strict_schedule(true).unwrap();
    let request = req();
    let plan = db.store.generate(&request).unwrap();
    let deadline = (Utc::now() + Duration::hours(1)).to_rfc3339();
    let input = SaveRequest {
        request,
        preview_id: plan.id,
        deadline: deadline.clone(),
        label: "test".into(),
    };
    assert!(db.store.save(&input, "personal").is_err());
    let mut schedule = Schedule {
        issue: "26119".into(),
        deadline: deadline.clone(),
        source_url: operations::SCHEDULE_URL.into(),
        source_hash: "a".repeat(64),
        verified_at: (Utc::now() - Duration::hours(3)).to_rfc3339(),
        basis: "isolated_test".into(),
    };
    db.store.set_schedule(&schedule).unwrap();
    assert!(db.store.save(&input, "personal").is_err());
    schedule.verified_at = Utc::now().to_rfc3339();
    db.store.set_schedule(&schedule).unwrap();
    assert!(db
        .store
        .save(
            &SaveRequest {
                deadline: (Utc::now() + Duration::hours(2)).to_rfc3339(),
                ..input.clone()
            },
            "personal"
        )
        .is_err());
    assert!(db.store.save(&input, "personal").is_ok());
}
#[test]
fn calendar_derivation_checks_all_year_dates_and_holidays() {
    use chrono::Datelike;
    let now = chrono::DateTime::parse_from_rfc3339("2026-01-06T04:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let mut draws = history(1);
    draws[0].issue = "26001".into();
    draws[0].date = "2026-01-05".into();
    let current = json!({"success":true,"value":{"dlt":{"lotteryDrawNum":"26001","lotteryDrawResult":"01 04 07 10 13 01 06","lotteryDrawTime":"2026-01-05"}}});
    let closures = json!({"success":true,"value":{"dateArr":["20260103"]}});
    let result =
        operations::derive_schedule_at(&draws, &current, &closures, "销售截止21:00", now).unwrap();
    assert_eq!(result.issue, "26002");
    assert_eq!(
        chrono::DateTime::parse_from_rfc3339(&result.deadline)
            .unwrap()
            .day(),
        7
    );
    draws[0].date = "2026-01-03".into();
    assert!(
        operations::derive_schedule_at(&draws, &current, &closures, "销售截止21:00", now).is_err()
    );
}
#[test]
fn optimizer_uses_equal_budgets_and_independent_validation() {
    let input = OptimizeRequest {
        request: GenerateRequest { bets: 7, ..req() },
        objective: "seen".into(),
        trials: 1000,
    };
    let result = optimizer::optimize(&history(30), 42, &input, &|_, _| Ok(())).unwrap();
    let candidates = result["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 3);
    for row in candidates {
        assert_eq!(row["plan"]["cost_fen"], 1400);
        let request: GenerateRequest =
            serde_json::from_value(row["plan"]["request"].clone()).unwrap();
        assert_eq!(
            row["plan"]["id"],
            plan::generate(&request, 42, &history(30)).unwrap().id
        );
    }
    assert_ne!(
        result["independent_validation"]["simulation_seed"],
        candidates[0]["selection_simulation"]["simulation_seed"]
    );
    let structural = &candidates[2]["structure"];
    assert_eq!(structural["front_union"], 35);
    assert_eq!(structural["unique_back_pairs"], 7);
}
#[test]
fn research_aggregates_seeds_by_period_and_keeps_holdout_disjoint() {
    let request = ResearchRequest {
        config: BacktestRequest {
            warmup: 10,
            limit: 10,
            bets: 3,
            ..Default::default()
        },
        seeds: vec![11, 12],
        validation_periods: 10,
    };
    let report = research::evaluate(&history(50), &request, &|_, _| Ok(())).unwrap();
    assert!(report["selection_end"].as_str() < report["validation_start"].as_str());
    assert_eq!(report["reports"].as_array().unwrap().len(), 20);
    for comparison in report["validation_comparisons"].as_array().unwrap() {
        assert_eq!(comparison["paired"]["n"], 10);
        let p = comparison["holm_adjusted_p_approx"].as_f64().unwrap();
        assert!((0.0..=1.0).contains(&p));
    }
    for row in report["reports"].as_array().unwrap() {
        for trace in row["report"]["trace"].as_array().unwrap() {
            assert!(trace["data_cutoff"].as_str() < trace["issue"].as_str());
        }
    }
}
#[test]
fn background_jobs_freeze_inputs_cancel_and_report_interrupted_restart() {
    let db = Db::new();
    db.store.import(&history(50)).unwrap();
    let input = json!({"warmup":10,"limit":5,"bets":2});
    let job = db.store.submit_job("backtest", input.clone()).unwrap();
    db.store.update_settings(&Settings { seed: 999 }).unwrap();
    db.store.import(&history(51)[50..]).unwrap();
    db.store.run_job(&job.id).unwrap();
    let completed = db.store.job(&job.id).unwrap();
    assert_eq!(completed.state, "completed");
    assert_eq!(completed.history_hash, job.history_hash);
    assert_eq!(completed.result.unwrap()["last_issue"], "25050");
    let cancelled = db.store.submit_job("backtest", input.clone()).unwrap();
    db.store.cancel_job(&cancelled.id).unwrap();
    assert!(db.store.run_job(&cancelled.id).is_err());
    assert_eq!(db.store.job(&cancelled.id).unwrap().state, "cancelled");
    let running = db.store.submit_job("backtest", input).unwrap();
    let mut value = serde_json::to_value(&running).unwrap();
    value["state"] = json!("running");
    let conn = rusqlite::Connection::open(&db.store.path).unwrap();
    conn.execute(
        "UPDATE jobs SET state='running',body=?1 WHERE id=?2",
        rusqlite::params![value.to_string(), running.id],
    )
    .unwrap();
    assert!(db.store.recover_jobs().unwrap().is_empty());
    assert_eq!(db.store.job(&running.id).unwrap().state, "interrupted");
    let cancelled_at = std::cell::Cell::new(0);
    assert!(analysis::backtest_with(
        &history(50),
        &BacktestRequest {
            warmup: 10,
            limit: 10,
            ..Default::default()
        },
        &|done, _| {
            cancelled_at.set(done);
            anyhow::ensure!(done < 3, "cancel");
            Ok(())
        }
    )
    .is_err());
    assert_eq!(cancelled_at.get(), 3);
}
#[test]
fn verified_backup_restores_original_records_and_never_overwrites() {
    let db = Db::new();
    db.store.import(&history(5)).unwrap();
    saved(&db.store).unwrap();
    let backup = db.root.join("backup.sqlite3");
    let report = db.store.verified_backup(&backup).unwrap();
    assert_eq!(report["restore_verified"], true);
    assert_eq!(report["records_verified"], 1);
    let bytes = std::fs::read(&backup).unwrap();
    let target = db.root.join("restored.sqlite3");
    Store::restore(&backup, &target).unwrap();
    assert_eq!(bytes, std::fs::read(&backup).unwrap());
    assert!(Store::restore(&backup, &target).is_err());
    assert_eq!(
        Store::open(target).unwrap().records().unwrap()[0].id,
        db.store.records().unwrap()[0].id
    );
    assert_eq!(
        db.store.draw_page(2, 2, "").unwrap()["draws"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        db.store.record_page(1, 1, "26", Some("personal")).unwrap()["total"],
        1
    );
}
#[test]
fn automation_freezes_once_then_settles_without_rebuilding() {
    let db = Db::new();
    let request = req();
    let schedule = Schedule {
        issue: request.issue.clone(),
        deadline: (Utc::now() + Duration::minutes(30)).to_rfc3339(),
        source_url: operations::SCHEDULE_URL.into(),
        source_hash: "b".repeat(64),
        verified_at: Utc::now().to_rfc3339(),
        basis: "isolated_test".into(),
    };
    db.store.set_schedule(&schedule).unwrap();
    db.store.automate().unwrap();
    let before = db.store.recommendations().unwrap();
    assert_eq!(before["records"].as_array().unwrap().len(), 5);
    db.store.update_settings(&Settings { seed: 87231 }).unwrap();
    db.store.automate().unwrap();
    assert_eq!(db.store.recommendations().unwrap(), before);
    let mut draw = history(1).remove(0);
    draw.issue = request.issue;
    draw.date = "2026-01-01".into();
    db.store.import(&[draw]).unwrap();
    let after = db.store.recommendations().unwrap();
    for old in before["records"].as_array().unwrap() {
        let new = after["records"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["id"] == old["id"])
            .unwrap();
        assert_eq!(old["plan"], new["plan"]);
        assert!(!new["settlement"].is_null());
    }
}

#[test]
fn recommendation_batch_cannot_be_created_late_even_when_all_tickets_exist() {
    let db = Db::new();
    let deadline = (Utc::now() + Duration::hours(1)).to_rfc3339();
    for (_, request) in plan::presets("26119") {
        let p = db.store.generate(&request).unwrap();
        db.store
            .save(
                &SaveRequest {
                    request,
                    preview_id: p.id,
                    deadline: deadline.clone(),
                    label: "personal".into(),
                },
                "personal",
            )
            .unwrap();
    }
    assert!(db
        .store
        .archive_presets("26119", &(Utc::now() - Duration::seconds(1)).to_rfc3339())
        .is_err());
    assert_eq!(db.store.records().unwrap().len(), 5);
    assert_eq!(db.store.recommendations().unwrap()["batches"], json!([]));
    let expired = Schedule {
        issue: "26119".into(),
        deadline: (Utc::now() - Duration::seconds(1)).to_rfc3339(),
        source_url: operations::SCHEDULE_URL.into(),
        source_hash: "a".repeat(64),
        verified_at: Utc::now().to_rfc3339(),
        basis: "isolated_test".into(),
    };
    let conn = rusqlite::Connection::open(&db.store.path).unwrap();
    conn.execute(
        "INSERT INTO schedules VALUES(?1,?2,?3)",
        rusqlite::params![
            expired.issue,
            serde_json::to_string(&expired).unwrap(),
            lottery_dlt::digest(&expired).unwrap()
        ],
    )
    .unwrap();
    db.store.automate().unwrap();
    assert_eq!(db.store.automation_events().unwrap()[0]["status"], "missed");
    assert_eq!(db.store.recommendations().unwrap()["batches"], json!([]));
}
