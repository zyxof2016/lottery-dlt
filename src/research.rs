use crate::{
    analysis::{backtest_with, BacktestRequest, Progress},
    digest,
    model::{validate_history, Draw},
    plan::{Mode, Strategy},
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResearchRequest {
    pub config: BacktestRequest,
    pub seeds: Vec<u64>,
    pub validation_periods: usize,
}
impl Default for ResearchRequest {
    fn default() -> Self {
        Self {
            config: BacktestRequest::default(),
            seeds: vec![20261015, 20261016, 20261017],
            validation_periods: 100,
        }
    }
}
fn paired(rows: &[f64]) -> Value {
    let n = rows.len();
    if n < 2 {
        return json!({"n":n,"mean":null,"ci95":null,"p_approx":null});
    }
    let mean = rows.iter().sum::<f64>() / n as f64;
    let variance = rows.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / (n - 1) as f64;
    let se = (variance / n as f64).sqrt();
    let z = if se > 0. { mean.abs() / se } else { 0. };
    let t = 1. / (1. + 0.2316419 * z);
    let density = (-z * z / 2.).exp() / std::f64::consts::TAU.sqrt();
    let p = if se == 0. {
        1.
    } else {
        (2. * density
            * t
            * (0.319381530
                + t * (-0.356563782 + t * (1.781477937 + t * (-1.821255978 + t * 1.330274429)))))
            .clamp(0., 1.)
    };
    json!({"n":n,"mean":mean,"ci95":[mean-1.96*se,mean+1.96*se],"p_approx":p,"method":"按期聚合多个种子后的配对差，正态近似；小样本和时间相关性限制仍存在"})
}
pub fn evaluate(
    history: &[Draw],
    request: &ResearchRequest,
    progress: Progress<'_>,
) -> Result<Value> {
    ensure!(
        request.config.mode == Mode::Single,
        "研究对照支持单式；其余形态使用完整回测"
    );
    ensure!(
        (1..=5).contains(&request.seeds.len())
            && request
                .seeds
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == request.seeds.len(),
        "须提供1～5个不同种子"
    );
    ensure!(
        (10..=300).contains(&request.validation_periods)
            && (10..=500).contains(&request.config.limit),
        "训练期10～500，验证期10～300"
    );
    ensure!(
        request.config.bets
            * request.seeds.len()
            * (request.config.limit + request.validation_periods)
            * 5
            <= 200_000,
        "研究总注次超过200000"
    );
    let history = validate_history(history)?;
    ensure!(
        history.len() >= request.config.warmup + request.config.limit + request.validation_periods,
        "历史不足以分离训练和验证窗口"
    );
    let split = history.len() - request.validation_periods;
    let strategies = [
        Strategy::Random,
        Strategy::Coverage,
        Strategy::Hot,
        Strategy::Cold,
        Strategy::Omission,
    ];
    let mut reports = Vec::new();
    let mut arm = 0;
    let total_arms = request.seeds.len() * strategies.len() * 2;
    for phase in ["selection", "validation"] {
        for &seed in &request.seeds {
            for strategy in strategies {
                let mut config = request.config.clone();
                config.strategy = strategy;
                config.seed = seed;
                let data = if phase == "selection" {
                    &history[..split]
                } else {
                    config.limit = request.validation_periods;
                    &history[..]
                };
                let report = backtest_with(data, &config, &|done, total| {
                    progress(arm * total + done, total_arms * total)
                })?;
                reports
                    .push(json!({"phase":phase,"seed":seed,"strategy":strategy,"report":report}));
                arm += 1;
            }
        }
    }
    let baseline_reports = reports.clone();
    for row in &mut reports {
        let baseline = baseline_reports
            .iter()
            .find(|b| {
                b["phase"] == row["phase"] && b["seed"] == row["seed"] && b["strategy"] == "random"
            })
            .unwrap();
        let trace = row["report"]["trace"].as_array().unwrap();
        let base = baseline["report"]["trace"].as_array().unwrap();
        let delta = trace
            .iter()
            .zip(base)
            .map(|(a, b)| {
                a["winning_tickets"].as_i64().unwrap() - b["winning_tickets"].as_i64().unwrap()
            })
            .sum::<i64>() as f64
            / trace.len() as f64;
        row["report"]["paired_mean_winning_ticket_delta"] = json!(delta);
    }
    let mut train: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for r in reports.iter().filter(|r| r["phase"] == "selection") {
        train
            .entry(r["strategy"].as_str().unwrap().into())
            .or_default()
            .push(r["report"]["issue_win_rate"].as_f64().unwrap());
    }
    let selected = train
        .iter()
        .max_by(|a, b| {
            (a.1.iter().sum::<f64>() / a.1.len() as f64)
                .total_cmp(&(b.1.iter().sum::<f64>() / b.1.len() as f64))
        })
        .unwrap()
        .0
        .clone();
    let mut outcomes: BTreeMap<(String, String), Vec<f64>> = BTreeMap::new();
    let mut regimes: BTreeMap<(String, String), (usize, usize)> = BTreeMap::new();
    for r in reports.iter().filter(|r| r["phase"] == "validation") {
        for t in r["report"]["trace"].as_array().unwrap() {
            let strategy = r["strategy"].as_str().unwrap().to_string();
            let win = usize::from(t["winning_tickets"].as_u64().unwrap() > 0) as f64;
            outcomes
                .entry((strategy.clone(), t["issue"].as_str().unwrap().into()))
                .or_default()
                .push(win);
            let segment = regimes
                .entry((strategy, t["rule_version"].as_str().unwrap().into()))
                .or_default();
            segment.0 += 1;
            segment.1 += win as usize;
        }
    }
    let mut comparisons = Vec::new();
    for strategy in ["coverage", "hot", "cold", "omission"] {
        let mut deltas = Vec::new();
        for ((s, issue), wins) in &outcomes {
            if s == strategy {
                let base = &outcomes[&("random".into(), issue.clone())];
                deltas.push(
                    wins.iter().sum::<f64>() / wins.len() as f64
                        - base.iter().sum::<f64>() / base.len() as f64,
                );
            }
        }
        comparisons.push(json!({"strategy":strategy,"paired":paired(&deltas)}));
    }
    let mut order: Vec<usize> = (0..comparisons.len()).collect();
    order.sort_by(|&a, &b| {
        comparisons[a]["paired"]["p_approx"]
            .as_f64()
            .unwrap_or(1.)
            .total_cmp(&comparisons[b]["paired"]["p_approx"].as_f64().unwrap_or(1.))
    });
    let mut previous = 0f64;
    for (rank, &index) in order.iter().enumerate() {
        previous = previous.max(
            (comparisons[index]["paired"]["p_approx"]
                .as_f64()
                .unwrap_or(1.)
                * (order.len() - rank) as f64)
                .min(1.),
        );
        comparisons[index]["holm_adjusted_p_approx"] = json!(previous);
    }
    let segments:Vec<_>=regimes.into_iter().map(|((strategy,rule),(n,w))|json!({"strategy":strategy,"rule_version":rule,"seed_period_observations":n,"win_rate":w as f64/n as f64})).collect();
    progress(1, 1)?;
    Ok(
        json!({"config":request,"history_hash":digest(&history)?,"generator_version":crate::plan::GENERATOR_VERSION,"selected_on_training":selected,"selection_end":history[split-1].issue,"validation_start":history[split].issue,"validation_end":history.last().unwrap().issue,"reports":reports,"validation_comparisons":comparisons,"rule_segments":segments,"status":"historical_holdout","note":"多个种子共享相同训练/留出期；按期聚合后配对，避免把同期开奖重复当独立样本。选优仅用训练窗口；验证给出近似区间和Holm校正。此为历史留出分析，不是未来预注册确认，也不能证明预测优势。"}),
    )
}
