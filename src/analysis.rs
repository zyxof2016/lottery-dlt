use crate::{
    model::{validate_history, Draw},
    plan::{generate, GenerateRequest, Mode, Strategy},
    prizes::{fixed_yuan, level, settle, theory},
};
use anyhow::{ensure, Result};
use rand::{seq::SliceRandom, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub fn statistics(history: &[Draw], window: usize) -> Result<Value> {
    ensure!((1..=500).contains(&window), "统计窗口须为 1～500");
    let history = validate_history(history)?;
    let rows = &history[history.len().saturating_sub(window)..];
    let zone = |upper: u8, front: bool| {
        (1..=upper).map(|n| {
            let found = |d: &&Draw| if front { d.front.contains(&n) } else { d.back.contains(&n) };
            let count = rows.iter().filter(found).count();
            let omission = rows.iter().rev().position(|d| if front {d.front.contains(&n)} else {d.back.contains(&n)}).unwrap_or(rows.len());
            json!({"number":n,"count":count,"omission":omission,"omission_censored":omission==rows.len(),
                "expected_count":rows.len() as f64 * if front {5./35.} else {2./12.}})
        }).collect::<Vec<_>>()
    };
    Ok(
        json!({"window":rows.len(),"data_cutoff":rows.last().map(|d|&d.issue),"front":zone(35,true),"back":zone(12,false),
        "note":"频次与遗漏是历史描述；窗口外未出现的遗漏仅为下界，不表示下一期更可能开出。"}),
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BacktestRequest {
    pub strategy: Strategy,
    pub bets: usize,
    pub window: usize,
    pub warmup: usize,
    pub limit: usize,
    #[serde(with = "crate::model::seed_text")]
    pub seed: u64,
    pub additional: bool,
    pub multiplier: u8,
    pub mode: Mode,
    pub front_pool: usize,
    pub back_pool: usize,
    pub front_banker_count: usize,
    pub back_banker_count: usize,
    pub front_numbers: Vec<u8>,
    pub back_numbers: Vec<u8>,
    pub front_bankers: Vec<u8>,
    pub back_bankers: Vec<u8>,
}
impl Default for BacktestRequest {
    fn default() -> Self {
        Self {
            strategy: Strategy::Coverage,
            bets: 5,
            window: 60,
            warmup: 60,
            limit: 300,
            seed: 20261015,
            additional: false,
            multiplier: 1,
            mode: Mode::Single,
            front_pool: 12,
            back_pool: 6,
            front_banker_count: 1,
            back_banker_count: 0,
            front_numbers: vec![],
            back_numbers: vec![],
            front_bankers: vec![],
            back_bankers: vec![],
        }
    }
}

impl BacktestRequest {
    pub fn template(&self, issue: &str) -> GenerateRequest {
        GenerateRequest {
            issue: issue.into(),
            strategy: self.strategy,
            bets: self.bets,
            window: self.window,
            additional: self.additional,
            multiplier: self.multiplier,
            mode: self.mode,
            front_pool: self.front_pool,
            back_pool: self.back_pool,
            front_banker_count: self.front_banker_count,
            back_banker_count: self.back_banker_count,
            front_numbers: self.front_numbers.clone(),
            back_numbers: self.back_numbers.clone(),
            front_bankers: self.front_bankers.clone(),
            back_bankers: self.back_bankers.clone(),
        }
    }
}
pub type Progress<'a> = &'a dyn Fn(usize, usize) -> Result<()>;
pub fn backtest(history: &[Draw], request: &BacktestRequest) -> Result<Value> {
    backtest_with(history, request, &|_, _| Ok(()))
}
pub fn backtest_with(
    history: &[Draw],
    request: &BacktestRequest,
    progress: Progress<'_>,
) -> Result<Value> {
    ensure!(
        (request.mode != Mode::Single || (1..=100).contains(&request.bets))
            && (1..=1500).contains(&request.limit),
        "回测最多100注、1500期，注数×期数不能超过100000"
    );
    ensure!(
        (5..=500).contains(&request.warmup) && (1..=500).contains(&request.window),
        "预热须为5～500期，窗口须为1～500期"
    );
    let draws = validate_history(history)?;
    ensure!(
        draws.len() > request.warmup,
        "历史仅 {} 期，须多于预热 {} 期；请同步更多数据",
        draws.len(),
        request.warmup
    );
    let eligible: Vec<_> = (request.warmup..draws.len())
        .filter(|&i| draws[i].issue.as_str() >= "19019")
        .collect();
    ensure!(!eligible.is_empty(), "暂无19019期及以后的可核算开奖");
    let indices = &eligible[eligible.len().saturating_sub(request.limit)..];
    let mut trace = Vec::new();
    let mut winning_issues = 0;
    let mut winning_tickets = 0;
    let mut total_cost = 0u64;
    let mut priced_cost = 0u64;
    let mut priced_prize = 0u64;
    let mut priced_issues = 0;
    let mut full_front_hits = 0;
    let mut full_back_hits = 0;
    let mut recovered_issues = 0;
    let mut actual_bets = 0;
    for (position, &i) in indices.iter().enumerate() {
        progress(position, indices.len())?;
        let req = request.template(&draws[i].issue);
        let prior = &draws[i.saturating_sub(request.window)..i];
        let plan = generate(&req, request.seed, prior)?;
        ensure!(
            plan.tickets.len() * indices.len() <= 100_000,
            "实际注数×期数不能超过100000"
        );
        actual_bets = plan.tickets.len();
        let settlement = settle(&plan.tickets, req.additional, req.multiplier, &draws[i])?;
        winning_issues += usize::from(settlement.winning_tickets > 0);
        winning_tickets += settlement.winning_tickets;
        total_cost += plan.cost_fen;
        if let Some(prize) = settlement.total_prize_fen {
            recovered_issues += usize::from(prize >= plan.cost_fen);
            priced_cost += plan.cost_fen;
            priced_prize += prize;
            priced_issues += 1;
        }
        let front_hits = plan
            .front_pool
            .iter()
            .filter(|n| draws[i].front.contains(n))
            .count();
        let back_hits = plan
            .back_pool
            .iter()
            .filter(|n| draws[i].back.contains(n))
            .count();
        full_front_hits += usize::from(front_hits == 5);
        full_back_hits += usize::from(back_hits == 2);
        trace.push(json!({"issue":draws[i].issue,"data_cutoff":plan.data_cutoff,"plan_id":plan.id,
            "tickets":plan.tickets,"generator_version":plan.generator_version,"history_hash":plan.history_hash,"rule_version":settlement.rule_version,"winning_tickets":settlement.winning_tickets,"front_union_hits":front_hits,"back_union_hits":back_hits,
            "front_coverage":plan.front_pool.len(),"back_coverage":plan.back_pool.len(),"prize_fen":settlement.total_prize_fen,
            "cost_fen":plan.cost_fen,"best_front_hits":settlement.tickets.iter().map(|t|t.front_hits).max(),
            "best_back_hits":settlement.tickets.iter().map(|t|t.back_hits).max()}));
    }
    progress(indices.len(), indices.len())?;
    Ok(
        json!({"config":request,"actual_bets":actual_bets,"recovered_issues":recovered_issues,"recovery_rate_priced":if priced_issues>0 {Some(recovered_issues as f64/priced_issues as f64)} else {None},"periods":indices.len(),"first_issue":draws[indices[0]].issue,"last_issue":draws[*indices.last().unwrap()].issue,
        "winning_issues":winning_issues,"issue_win_rate":winning_issues as f64 / indices.len() as f64,"winning_tickets":winning_tickets,
        "cost_fen":total_cost,"priced_issues":priced_issues,"unpriced_issues":indices.len()-priced_issues,"priced_cost_fen":priced_cost,
        "priced_prize_fen":priced_prize,"priced_net_fen":priced_prize as i64-priced_cost as i64,
        "total_prize_fen":(priced_issues==indices.len()).then_some(priced_prize),
        "net_fen":(priced_issues==indices.len()).then_some(priced_prize as i64-total_cost as i64),
        "full_front_coverage_issues":full_front_hits,"full_back_coverage_issues":full_back_hits,"trace":trace,
        "note":"逐期只使用此前开奖，无前视偏差。历史回放不写入真实方案。缺少官方奖金的期数不计入完整收益；结果属于探索性描述，未证明预测优势。"}),
    )
}

pub fn research(history: &[Draw], request: &BacktestRequest) -> Result<Value> {
    research_with(history, request, &|_, _| Ok(()))
}
pub fn research_with(
    history: &[Draw],
    request: &BacktestRequest,
    progress: Progress<'_>,
) -> Result<Value> {
    ensure!(
        request.mode == Mode::Single,
        "五策略对照仅单式；复式/胆拖请使用完整回测"
    );
    // The same periods, budget and randomization rule are used for all arms.
    // Restrict total work independently of each arm's own validation.
    ensure!(
        request.limit <= 500 && request.bets <= 30,
        "研究对照最多500期、每期30注"
    );
    let mut reports = Vec::new();
    for strategy in [
        Strategy::Random,
        Strategy::Coverage,
        Strategy::Hot,
        Strategy::Cold,
        Strategy::Omission,
    ] {
        let mut config = request.clone();
        config.strategy = strategy;
        let arm = reports.len();
        reports.push(backtest_with(history, &config, &|done, total| {
            progress(arm * total + done, total * 5)
        })?);
    }
    let baseline: Vec<i64> = reports[0]["trace"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["winning_tickets"].as_i64().unwrap())
        .collect();
    for report in &mut reports {
        let differences: Vec<_> = report["trace"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&baseline)
            .map(|(r, b)| r["winning_tickets"].as_i64().unwrap() - b)
            .collect();
        report["paired_mean_winning_ticket_delta"] =
            json!(differences.iter().sum::<i64>() as f64 / differences.len() as f64);
    }
    Ok(
        json!({"status":"exploratory","reports":reports,"note":"固定比较随机、均衡覆盖、热号、冷号和遗漏五组，同期同预算。差值以开奖期配对；未做预注册确认与多重检验，不据此宣称优于随机。"}),
    )
}

pub fn wilson(successes: usize, n: usize) -> [f64; 2] {
    let p = successes as f64 / n as f64;
    let z2 = 1.96 * 1.96;
    let center = (p + z2 / (2. * n as f64)) / (1. + z2 / n as f64);
    let half =
        1.96 * ((p * (1. - p) + z2 / (4. * n as f64)) / n as f64).sqrt() / (1. + z2 / n as f64);
    [(center - half).max(0.), (center + half).min(1.)]
}

pub fn simulate(plan: &crate::plan::Plan, trials: usize) -> Result<Value> {
    simulate_with(plan, trials, 0x444c545f4d435631, &|_, _| Ok(()))
}
pub fn simulate_with(
    plan: &crate::plan::Plan,
    trials: usize,
    seed: u64,
    progress: Progress<'_>,
) -> Result<Value> {
    ensure!(
        (1000..=200_000).contains(&trials) && trials * plan.tickets.len() <= 5_000_000,
        "模拟须为1000～200000期，期数×注数最多500万"
    );
    let mut rng = ChaCha20Rng::seed_from_u64(seed);
    let mut wins = 0;
    let mut low_sum = 0u64;
    let mut high_sum = 0u64;
    let mut recovery_low = 0;
    let mut recovery_high = 0;
    for trial in 0..trials {
        if trial % 100 == 0 {
            progress(trial, trials)?;
        }
        let (old_low, old_high) = (low_sum, high_sum);
        let front: Vec<u8> = (1..=35)
            .collect::<Vec<_>>()
            .choose_multiple(&mut rng, 5)
            .copied()
            .collect();
        let back: Vec<u8> = (1..=12)
            .collect::<Vec<_>>()
            .choose_multiple(&mut rng, 2)
            .copied()
            .collect();
        let mut any = false;
        for ticket in &plan.tickets {
            let f = ticket.front.iter().filter(|n| front.contains(n)).count() as u8;
            let b = ticket.back.iter().filter(|n| back.contains(n)).count() as u8;
            if let Some(grade) = level(f, b, true) {
                any = true;
                low_sum +=
                    fixed_yuan(grade, true, false).unwrap_or(0) * plan.request.multiplier as u64;
                high_sum +=
                    fixed_yuan(grade, true, true).unwrap_or(0) * plan.request.multiplier as u64;
            }
        }
        wins += usize::from(any);
        recovery_low += usize::from((low_sum - old_low) * 100 >= plan.cost_fen);
        recovery_high += usize::from((high_sum - old_high) * 100 >= plan.cost_fen);
    }
    Ok(
        json!({"trials":trials,"plan_id":plan.id,"any_prize_probability":wins as f64/trials as f64,"confidence_interval_95":wilson(wins,trials),
        "fixed_recovery_probability_low":recovery_low as f64/trials as f64,
        "fixed_recovery_probability_high":recovery_high as f64/trials as f64,
        "fixed_recovery_low_ci95":wilson(recovery_low,trials),"fixed_recovery_high_ci95":wilson(recovery_high,trials),
        "simulation_seed":seed,"cost_fen":plan.cost_fen,
        "first_prize_probability_exact":plan.first_prize_probability,
        "mean_fixed_prize_low_yuan":low_sum as f64/trials as f64,"mean_fixed_prize_high_yuan":high_sum as f64/trials as f64,
        "single_ticket_theory":theory(),"note":"按实际票组模拟整组至少一注中奖概率，保留注间相关性。95%区间仅描述蒙特卡洛误差；奖金均值只含标准固定奖，不含浮动奖、派奖及税。"}),
    )
}
