use chrono::{Duration, Utc};
use lottery_dlt::{
    analysis::{self, BacktestRequest},
    fetch,
    model::{choose, Draw, Settings, Ticket, TOTAL_OUTCOMES},
    plan::{generate, GenerateRequest, Mode, Strategy},
    prizes::{self, level, settle},
    store::{SaveRequest, Store},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

struct TestDb {
    store: Store,
    dir: PathBuf,
}
impl TestDb {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "dlt-test-{}-{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        Self {
            store: Store::open(dir.join("test.sqlite3")).unwrap(),
            dir,
        }
    }
}
impl Drop for TestDb {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
fn fixture() -> Draw {
    serde_json::from_str::<Vec<Draw>>(include_str!("fixtures/26014.json"))
        .unwrap()
        .remove(0)
}
fn request() -> GenerateRequest {
    GenerateRequest {
        issue: "26119".into(),
        ..Default::default()
    }
}
fn ticket(f: u8, b: u8) -> Ticket {
    let draw = fixture();
    let mut front = draw.front[..f as usize].to_vec();
    front.extend([2, 3, 4, 5, 7].into_iter().take(5 - f as usize));
    let mut back = draw.back[..b as usize].to_vec();
    back.extend([2, 3].into_iter().take(2 - b as usize));
    Ticket { front, back }.validate().unwrap()
}
fn save_request(store: &Store, req: &GenerateRequest) -> SaveRequest {
    SaveRequest {
        request: req.clone(),
        preview_id: store.generate(req).unwrap().id,
        deadline: (Utc::now() + Duration::hours(2)).to_rfc3339(),
        label: "测试原票组".into(),
    }
}
fn history(n: usize) -> Vec<Draw> {
    // Synthetic test data only; never shipped into application storage.
    let start = chrono::NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    (0..n)
        .map(|i| Draw {
            issue: format!("25{:03}", i + 1),
            date: (start + Duration::days(i as i64)).to_string(),
            front: (0..5).map(|j| ((i * 7 + j * 3) % 35 + 1) as u8).collect(),
            back: vec![(i % 12 + 1) as u8, ((i + 5) % 12 + 1) as u8],
            awards: BTreeMap::new(),
            pool_before_fen: None,
            source: "synthetic_test".into(),
        })
        .collect()
}

#[test]
fn combinations_and_probability_partition() {
    assert_eq!(choose(35, 5) * choose(12, 2), TOTAL_OUTCOMES);
    assert_eq!(choose(35, 36), 0);
    assert_eq!(choose(35, 0), 1);
    let theory = prizes::theory();
    let rows = theory["rows"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|v| v["outcomes"].as_u64().unwrap())
            .sum::<u64>(),
        TOTAL_OUTCOMES
    );
    assert!(
        (rows
            .iter()
            .map(|v| v["probability"].as_f64().unwrap())
            .sum::<f64>()
            - 1.)
            .abs()
            < 1e-12
    );
    assert_eq!(
        theory["first_prize_probability"].as_f64().unwrap(),
        1. / TOTAL_OUTCOMES as f64
    );
}

#[test]
fn all_eighteen_hit_conditions_across_rule_boundary() {
    let modern = [
        [None, None, Some(7)],
        [None, None, Some(7)],
        [None, Some(7), Some(6)],
        [Some(7), Some(6), Some(5)],
        [Some(5), Some(4), Some(3)],
        [Some(3), Some(2), Some(1)],
    ];
    let old = [
        [None, None, Some(9)],
        [None, None, Some(9)],
        [None, Some(9), Some(8)],
        [Some(9), Some(8), Some(6)],
        [Some(7), Some(5), Some(4)],
        [Some(3), Some(2), Some(1)],
    ];
    for f in 0..=5 {
        for b in 0..=2 {
            assert_eq!(level(f, b, true), modern[f as usize][b as usize]);
            assert_eq!(level(f, b, false), old[f as usize][b as usize]);
            let result = settle(&[ticket(f, b)], false, 1, &fixture()).unwrap();
            assert_eq!(result.tickets[0].level, modern[f as usize][b as usize]);
        }
    }
    assert_eq!(prizes::rule_version("26013").unwrap(), "dlt-2019-9");
    assert_eq!(prizes::rule_version("26014").unwrap(), "dlt-2026-7");
    assert!(prizes::rule_version("19018").is_err());
}

#[test]
fn fixed_prizes_use_opening_pool_and_missing_money_is_unknown() {
    let mut draw = fixture();
    draw.awards.clear();
    assert_eq!(
        settle(&[ticket(3, 0)], false, 1, &draw)
            .unwrap()
            .total_prize_fen,
        None
    );
    draw.pool_before_fen = Some(79_999_999_999);
    assert_eq!(
        settle(&[ticket(3, 0)], true, 2, &draw)
            .unwrap()
            .total_prize_fen,
        Some(1000)
    );
    draw.pool_before_fen = Some(80_000_000_000);
    let s = settle(&[ticket(3, 0)], true, 2, &draw).unwrap();
    assert_eq!(s.total_prize_fen, Some(1400));
    assert_eq!(s.cost_fen, 600);
    assert_eq!(s.net_fen, Some(800));
    assert_eq!(
        settle(&[ticket(5, 2)], false, 1, &draw)
            .unwrap()
            .total_prize_fen,
        None
    );
    assert_eq!(
        settle(&[ticket(0, 0)], false, 1, &draw)
            .unwrap()
            .total_prize_fen,
        Some(0)
    );
    let mut old = draw;
    old.issue = "26013".into();
    assert_eq!(
        settle(&[ticket(5, 0)], false, 1, &old)
            .unwrap()
            .total_prize_fen,
        Some(1_000_000)
    );
}

#[test]
fn published_awards_override_standard_prizes_and_additional_uses_published_amount() {
    let mut draw = fixture();
    draw.pool_before_fen = Some(80_000_000_000);
    assert_eq!(
        settle(&[ticket(5, 0)], true, 1, &draw)
            .unwrap()
            .total_prize_fen,
        Some(500_000)
    );
    assert_eq!(
        settle(&[ticket(5, 1)], true, 1, &draw)
            .unwrap()
            .total_prize_fen,
        Some(43_588_600)
    );
    let s = settle(&[ticket(5, 2)], true, 1, &draw).unwrap();
    assert_eq!(s.total_prize_fen, None);
    assert_eq!(s.winning_tickets, 1);
}

#[test]
fn generator_is_reproducible_unique_and_canonical() {
    for strategy in [
        Strategy::Random,
        Strategy::Coverage,
        Strategy::Hot,
        Strategy::Cold,
        Strategy::Omission,
    ] {
        let req = GenerateRequest {
            strategy,
            bets: 60,
            ..request()
        };
        let p = generate(&req, 42, &history(60)).unwrap();
        let again = generate(&req, 42, &history(60)).unwrap();
        assert_eq!(p.id, again.id);
        assert_eq!(p.tickets, again.tickets);
        assert_eq!(p.tickets.iter().collect::<BTreeSet<_>>().len(), 60);
        assert!(p.tickets.iter().all(|t| t.validate().unwrap() == *t));
        assert_eq!(p.cost_fen, 12_000);
        assert_eq!(p.first_prize_probability, 60. / TOTAL_OUTCOMES as f64);
        assert_ne!(p.id, generate(&req, 43, &history(60)).unwrap().id);
    }
}

#[test]
fn balanced_coverage_uses_all_front_numbers_in_seven_bets() {
    let p = generate(
        &GenerateRequest {
            strategy: Strategy::Coverage,
            bets: 7,
            ..request()
        },
        42,
        &[],
    )
    .unwrap();
    assert_eq!(p.front_pool.len(), 35);
    assert_eq!(p.back_pool.len(), 12);
    assert_eq!(p.front_exposure, vec![1; 35]);
    assert_eq!(p.back_exposure.iter().sum::<usize>(), 14);
    assert_eq!(p.back_exposure.iter().min(), Some(&1));
    assert_eq!(p.back_exposure.iter().max(), Some(&2));
}

#[test]
fn compound_and_dantuo_expand_exactly_and_preserve_bankers() {
    let compound = GenerateRequest {
        mode: Mode::Compound,
        front_pool: 7,
        back_pool: 3,
        ..request()
    };
    let p = generate(&compound, 3, &[]).unwrap();
    assert_eq!(p.tickets.len(), 63);
    assert_eq!(p.cost_fen, 12600);
    let req = GenerateRequest {
        mode: Mode::Dantuo,
        front_pool: 7,
        back_pool: 3,
        front_banker_count: 2,
        back_banker_count: 1,
        front_numbers: vec![1, 2, 3, 4, 5, 6, 7],
        back_numbers: vec![1, 2, 3],
        front_bankers: vec![1, 2],
        back_bankers: vec![1],
        ..request()
    };
    let p = generate(&req, 3, &[]).unwrap();
    assert_eq!(p.tickets.len(), 20);
    assert!(p
        .tickets
        .iter()
        .all(|t| t.front.contains(&1) && t.front.contains(&2) && t.back.contains(&1)));
    assert_eq!(p.tickets.iter().collect::<BTreeSet<_>>().len(), 20);
    let mut reversed = req;
    reversed.front_numbers.reverse();
    reversed.front_bankers.reverse();
    assert_eq!(p.id, generate(&reversed, 3, &[]).unwrap().id);
}

#[test]
fn invalid_parameters_and_unbounded_expansions_are_rejected() {
    let mut req = request();
    req.bets = 0;
    assert!(generate(&req, 0, &[]).is_err());
    let mut req = request();
    req.issue = "2026119".into();
    assert!(generate(&req, 0, &[]).is_err());
    let mut req = request();
    req.mode = Mode::Compound;
    req.front_pool = 35;
    req.back_pool = 12;
    assert!(generate(&req, 0, &[]).is_err());
    let mut req = request();
    req.mode = Mode::Dantuo;
    req.front_banker_count = 5;
    assert!(generate(&req, 0, &[]).is_err());
    let mut req = request();
    req.mode = Mode::Dantuo;
    req.front_pool = 5;
    assert!(generate(&req, 0, &[]).is_err());
    let mut req = request();
    req.multiplier = 0;
    assert!(generate(&req, 0, &[]).is_err());
    let mut req = request();
    req.strategy = Strategy::Hot;
    assert!(generate(&req, 0, &[]).is_err());
    assert!(Ticket {
        front: vec![1, 1, 2, 3, 4],
        back: vec![1, 2]
    }
    .validate()
    .is_err());
}

#[test]
fn future_draws_cannot_change_a_historical_prediction() {
    let draws = history(100);
    for strategy in [
        Strategy::Random,
        Strategy::Hot,
        Strategy::Cold,
        Strategy::Omission,
    ] {
        let req = GenerateRequest {
            issue: draws[50].issue.clone(),
            strategy,
            ..request()
        };
        let before = generate(&req, 42, &draws[..50]).unwrap();
        let with_future = generate(&req, 42, &draws).unwrap();
        assert_eq!(before.id, with_future.id);
        assert_eq!(before.tickets, with_future.tickets);
        assert_eq!(before.data_cutoff, Some(draws[49].issue.clone()));
    }
}

#[test]
fn save_is_idempotent_and_seed_changes_do_not_rewrite_original_tickets() {
    let db = TestDb::new();
    db.store.update_settings(&Settings { seed: 42 }).unwrap();
    let input = save_request(&db.store, &request());
    let saved = db.store.save(&input, "personal").unwrap();
    let mut repeated = input.clone();
    repeated.label = "不应覆盖".into();
    assert_eq!(
        db.store.save(&repeated, "personal").unwrap().label,
        saved.label
    );
    assert_eq!(db.store.records().unwrap().len(), 1);
    db.store.update_settings(&Settings { seed: 43 }).unwrap();
    assert_ne!(db.store.generate(&request()).unwrap().id, saved.id);
    assert_eq!(
        db.store.records().unwrap()[0].plan.tickets,
        saved.plan.tickets
    );
    assert!(db.store.verify_audit().unwrap() >= 2);
}

#[test]
fn stale_preview_deadline_and_known_draws_cannot_create_records() {
    let db = TestDb::new();
    let input = save_request(&db.store, &request());
    db.store.update_settings(&Settings { seed: 19 }).unwrap();
    assert!(db.store.save(&input, "personal").is_err());
    let mut input = save_request(&db.store, &request());
    input.deadline = (Utc::now() - Duration::seconds(1)).to_rfc3339();
    assert!(db.store.save(&input, "personal").is_err());
    db.store.import(&[fixture()]).unwrap();
    assert!(db
        .store
        .generate(&GenerateRequest {
            issue: "26014".into(),
            ..request()
        })
        .is_err());
    assert!(db.store.records().unwrap().is_empty());
}

#[test]
fn draw_import_is_atomic_on_conflicts_and_can_enrich_missing_awards() {
    let db = TestDb::new();
    let mut bare = fixture();
    bare.awards.clear();
    assert_eq!(db.store.import(&[bare.clone()]).unwrap(), 1);
    assert_eq!(db.store.import(&[fixture()]).unwrap(), 1);
    assert_eq!(db.store.import(&[fixture()]).unwrap(), 0);
    let mut conflict = fixture();
    conflict.front = vec![1, 2, 3, 4, 5];
    let mut new = bare;
    new.issue = "26015".into();
    new.date = "2026-02-04".into();
    assert!(db.store.import(&[conflict, new]).is_err());
    assert_eq!(db.store.draws().unwrap().len(), 1);
    let mut conflict = fixture();
    conflict.awards.get_mut(&3).unwrap().basic_fen = Some(999);
    assert!(db.store.import(&[conflict]).is_err());
    assert_eq!(
        db.store.draws().unwrap()[0]
            .awards
            .get(&3)
            .unwrap()
            .basic_fen,
        Some(500_000)
    );
}

#[test]
fn original_ticket_settles_after_import_and_unknown_award_enrichment() {
    let db = TestDb::new();
    let req = GenerateRequest {
        issue: "26014".into(),
        mode: Mode::Compound,
        front_pool: 5,
        back_pool: 2,
        front_numbers: fixture().front,
        back_numbers: fixture().back,
        ..request()
    };
    let input = save_request(&db.store, &req);
    db.store.save(&input, "personal").unwrap();
    let mut draw = fixture();
    draw.awards.clear();
    db.store.import(&[draw]).unwrap();
    let unknown = db.store.records().unwrap().remove(0);
    assert_eq!(unknown.settlement.unwrap().total_prize_fen, None);
    db.store.import(&[fixture()]).unwrap();
    let known = db.store.records().unwrap().remove(0);
    assert_eq!(known.id, input.preview_id);
    assert_eq!(
        known.settlement.unwrap().total_prize_fen,
        Some(1_000_000_000)
    );
}

#[test]
fn audit_and_record_corruption_are_detected() {
    let db = TestDb::new();
    let input = save_request(&db.store, &request());
    db.store.save(&input, "personal").unwrap();
    let conn = rusqlite::Connection::open(&db.store.path).unwrap();
    conn.execute("UPDATE audit SET object_id='tampered'", [])
        .unwrap();
    assert!(db.store.verify_audit().is_err());
    conn.execute("UPDATE plans SET content_hash='tampered'", [])
        .unwrap();
    assert!(db.store.records().is_err());
}

#[test]
fn official_parser_rejects_bad_payloads_and_never_uses_after_draw_pool() {
    let value = json!({"success":true,"value":{"list":[{"lotteryDrawNum":"26014","lotteryDrawTime":"2026-02-02 21:25:00","lotteryDrawResult":"16 18 23 34 35 01 06","poolBalanceAfterdraw":"842732986.13"}]}});
    let records = fetch::parse_official(&value).unwrap();
    assert_eq!(records[0].front, fixture().front);
    assert_eq!(records[0].pool_before_fen, None);
    assert!(records[0].awards.is_empty());
    assert!(fetch::parse_official(&json!({"value":{}})).is_err());
    let mut bad = value;
    bad["value"]["list"][0]["lotteryDrawResult"] = json!("01 01 02 03 04 01 02");
    assert!(fetch::parse_official(&bad).is_err());
    assert_eq!(
        fetch::load(std::path::Path::new("tests/fixtures/26014.json"))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn backtest_is_read_only_causal_and_research_uses_equal_budgets() {
    let db = TestDb::new();
    db.store.import(&history(80)).unwrap();
    let config = BacktestRequest {
        warmup: 10,
        limit: 20,
        window: 10,
        bets: 3,
        ..Default::default()
    };
    let result = analysis::backtest(&db.store.draws().unwrap(), &config).unwrap();
    assert_eq!(result["periods"], 20);
    assert_eq!(result["cost_fen"], 12000);
    for t in result["trace"].as_array().unwrap() {
        assert!(t["data_cutoff"].as_str().unwrap() < t["issue"].as_str().unwrap());
    }
    let research = analysis::research(&db.store.draws().unwrap(), &config).unwrap();
    for report in research["reports"].as_array().unwrap() {
        assert_eq!(report["cost_fen"], 12000);
        assert_eq!(report["first_issue"], result["first_issue"]);
        assert_eq!(report["last_issue"], result["last_issue"]);
    }
    assert!(db.store.records().unwrap().is_empty());
}

#[test]
fn statistics_censor_omission_and_seed_roundtrips_all_bits() {
    let stats = analysis::statistics(&[], 60).unwrap();
    assert_eq!(stats["window"], 0);
    assert_eq!(stats["front"][0]["omission_censored"], true);
    let settings = Settings { seed: u64::MAX };
    let text = serde_json::to_string(&settings).unwrap();
    assert!(text.contains("\"18446744073709551615\""));
    assert_eq!(
        serde_json::from_str::<Settings>(&text).unwrap().seed,
        u64::MAX
    );
    assert_eq!(
        serde_json::from_str::<Settings>("{\"seed\":42}")
            .unwrap()
            .seed,
        42
    );
}

#[test]
fn preset_archive_is_atomic_and_reuses_an_already_saved_ticket_group() {
    let db = TestDb::new();
    let input = save_request(&db.store, &request());
    let personal = db.store.save(&input, "personal").unwrap();
    let expired = (Utc::now() - Duration::seconds(1)).to_rfc3339();
    assert!(db.store.archive_presets("26119", &expired).is_err());
    assert_eq!(db.store.records().unwrap().len(), 1);
    assert!(db.store.recommendations().unwrap()["records"]
        .as_array()
        .unwrap()
        .is_empty());
    let archive = db.store.archive_presets("26119", &input.deadline).unwrap();
    assert_eq!(archive.len(), 5);
    assert_eq!(archive[0].id, personal.id);
    assert_eq!(db.store.records().unwrap().len(), 5);
    db.store.archive_presets("26119", &input.deadline).unwrap();
    assert_eq!(db.store.records().unwrap().len(), 5);
    let before = db.store.recommendations().unwrap();
    assert_eq!(before["batches"].as_array().unwrap().len(), 1);
    assert_eq!(before["records"].as_array().unwrap().len(), 5);
    // A reused personal record belongs to both views without rewriting it.
    assert_eq!(
        db.store
            .records()
            .unwrap()
            .iter()
            .find(|r| r.id == personal.id)
            .unwrap()
            .kind,
        "personal"
    );
    db.store
        .update_settings(&Settings { seed: 874321 })
        .unwrap();
    let repeated = db.store.archive_presets("26119", &expired).unwrap();
    assert_eq!(
        archive.iter().map(|r| &r.id).collect::<Vec<_>>(),
        repeated.iter().map(|r| &r.id).collect::<Vec<_>>()
    );
    assert_eq!(db.store.recommendations().unwrap(), before);
    let reopened = Store::open(&db.store.path).unwrap();
    assert_eq!(reopened.recommendations().unwrap(), before);
}

#[test]
fn experimental_single_honors_the_exact_candidate_pool() {
    let mut draws = history(10);
    for draw in &mut draws {
        draw.front = vec![1, 2, 3, 4, 5];
        draw.back = vec![1, 2];
    }
    let req = GenerateRequest {
        strategy: Strategy::Hot,
        front_pool: 5,
        back_pool: 2,
        bets: 1,
        ..request()
    };
    let plan = generate(&req, 42, &draws).unwrap();
    assert_eq!(plan.tickets[0].front, vec![1, 2, 3, 4, 5]);
    assert_eq!(plan.tickets[0].back, vec![1, 2]);
    assert!(generate(&GenerateRequest { bets: 2, ..req }, 42, &draws).is_err());
}

#[test]
fn inactive_parameters_cannot_change_the_ticket_group() {
    let original = generate(&request(), 42, &[]).unwrap();
    let altered = GenerateRequest {
        front_pool: 30,
        back_pool: 11,
        window: 1,
        front_banker_count: 4,
        ..request()
    };
    assert_eq!(original.id, generate(&altered, 42, &[]).unwrap().id);
    let compound = GenerateRequest {
        mode: Mode::Compound,
        ..request()
    };
    assert_eq!(
        generate(&compound, 42, &[]).unwrap().id,
        generate(
            &GenerateRequest {
                bets: 99,
                ..compound.clone()
            },
            42,
            &[]
        )
        .unwrap()
        .id
    );
    assert!(generate(
        &GenerateRequest {
            strategy: Strategy::Coverage,
            ..compound
        },
        42,
        &[]
    )
    .is_err());
}

#[test]
fn recommendation_manifest_tampering_is_detected() {
    let db = TestDb::new();
    let input = save_request(&db.store, &request());
    db.store.archive_presets("26119", &input.deadline).unwrap();
    let conn = rusqlite::Connection::open(&db.store.path).unwrap();
    conn.execute(
        "UPDATE recommendation_batches SET body=replace(body,'随机','伪造')",
        [],
    )
    .unwrap();
    assert!(db.store.recommendations().is_err());
    assert!(db.store.export().is_err());
    assert!(db.store.archive_presets("26119", &input.deadline).is_err());
}

#[test]
fn actual_ticket_simulation_agrees_with_single_ticket_exact_probability() {
    let plan = generate(
        &GenerateRequest {
            bets: 1,
            ..request()
        },
        42,
        &[],
    )
    .unwrap();
    let result = analysis::simulate(&plan, 20_000).unwrap();
    let observed = result["any_prize_probability"].as_f64().unwrap();
    let exact = prizes::theory()["any_prize_probability"].as_f64().unwrap();
    assert!((observed - exact).abs() < 0.01);
    assert_eq!(
        result["first_prize_probability_exact"],
        json!(1. / TOTAL_OUTCOMES as f64)
    );
    assert!(result["confidence_interval_95"][0].as_f64().unwrap() < observed);
    assert!(result["confidence_interval_95"][1].as_f64().unwrap() > observed);
    assert!(analysis::simulate(&plan, 0).is_err());
}

#[test]
fn promotion_rows_cannot_replace_basic_or_additional_awards() {
    let real = fetch::load(std::path::Path::new("tests/fixtures/official-25049.json"))
        .unwrap()
        .remove(0);
    assert_eq!(real.awards[&1].basic_fen, Some(936_863_900));
    assert_eq!(real.awards[&2].additional_fen, Some(18_219_800));
    assert_eq!(real.awards[&9].basic_fen, Some(500));
    let mut value = json!({"success":true,"value":{"list":[{"lotteryDrawNum":"25049","lotteryDrawTime":"2025-05-05","lotteryDrawResult":"09 20 22 29 34 03 08","prizeLevelList":[
        {"prizeLevel":"一等奖","stakeAmount":"9,368,639"},
        {"prizeLevel":"一等奖基本派奖","stakeAmount":"1,666,666"},
        {"prizeLevel":"二等奖","stakeAmount":"227,748"},
        {"prizeLevel":"二等奖(追加)","stakeAmount":"182,198"},
        {"prizeLevel":"二等奖追加派奖","stakeAmount":"47,059"},
        {"prizeLevel":"九等奖","stakeAmount":"5"},
        {"prizeLevel":"九等奖派奖","stakeAmount":"2.50"}
    ]}]}});
    for _ in 0..2 {
        let draw = fetch::parse_official(&value).unwrap().remove(0);
        assert_eq!(draw.awards[&1].basic_fen, Some(936_863_900));
        assert_eq!(draw.awards[&2].basic_fen, Some(22_774_800));
        assert_eq!(draw.awards[&2].additional_fen, Some(18_219_800));
        assert_eq!(draw.awards[&9].basic_fen, Some(500));
        value["value"]["list"][0]["prizeLevelList"]
            .as_array_mut()
            .unwrap()
            .reverse();
    }
    value["value"]["list"][0]["prizeLevelList"]
        .as_array_mut()
        .unwrap()
        .push(json!({"prizeLevel":"九等奖","stakeAmount":"99"}));
    assert!(fetch::parse_official(&value).is_err());
}

#[test]
fn award_maintenance_is_previewable_audited_and_never_changes_tickets() {
    let db = TestDb::new();
    let req = GenerateRequest {
        mode: Mode::Compound,
        front_pool: 5,
        back_pool: 2,
        front_numbers: fixture().front,
        back_numbers: fixture().back,
        issue: "26014".into(),
        ..request()
    };
    let input = save_request(&db.store, &req);
    let saved = db.store.save(&input, "personal").unwrap();
    let mut wrong = fixture();
    wrong.source = fetch::OFFICIAL_URL.into();
    wrong.awards.get_mut(&1).unwrap().basic_fen = Some(999);
    db.store.import(&[wrong.clone()]).unwrap();
    let mut correct = fixture();
    correct.source = fetch::OFFICIAL_URL.into();
    let old_audit = db.store.verify_audit().unwrap();
    assert_eq!(
        db.store
            .repair_official_awards(&[correct.clone()], false)
            .unwrap()["changed"],
        1
    );
    assert_eq!(db.store.draws().unwrap()[0].awards[&1].basic_fen, Some(999));
    assert_eq!(db.store.verify_audit().unwrap(), old_audit);
    assert_eq!(
        db.store
            .repair_official_awards(&[correct.clone()], true)
            .unwrap()["changed"],
        1
    );
    assert_eq!(db.store.verify_audit().unwrap(), old_audit + 1);
    let record = db.store.records().unwrap().remove(0);
    assert_eq!(record.plan.tickets, saved.plan.tickets);
    assert_eq!(record.created_at, saved.created_at);
    assert_eq!(
        record.settlement.unwrap().total_prize_fen,
        Some(1_000_000_000)
    );
    assert_eq!(
        db.store
            .repair_official_awards(&[correct.clone()], true)
            .unwrap()["changed"],
        0
    );
    let mut bad = correct.clone();
    bad.front = vec![1, 2, 3, 4, 5];
    assert!(db.store.repair_official_awards(&[bad], true).is_err());
    let mut manual = correct;
    manual.source = "manual".into();
    assert!(db.store.repair_official_awards(&[manual], true).is_err());
}
