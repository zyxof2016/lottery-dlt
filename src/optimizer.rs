use crate::{
    analysis::{simulate_with, Progress},
    digest,
    model::Draw,
    plan::{generate, GenerateRequest, Mode, Strategy},
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OptimizeRequest {
    pub request: GenerateRequest,
    pub objective: String,
    pub trials: usize,
}
impl Default for OptimizeRequest {
    fn default() -> Self {
        Self {
            request: GenerateRequest::default(),
            objective: "seen".into(),
            trials: 5000,
        }
    }
}
pub fn metrics(tickets: &[crate::model::Ticket]) -> Value {
    let fronts: BTreeSet<_> = tickets.iter().flat_map(|t| t.front.iter()).collect();
    let backs: BTreeSet<_> = tickets.iter().flat_map(|t| t.back.iter()).collect();
    let pairs: BTreeSet<_> = tickets.iter().map(|t| &t.back).collect();
    let mut overlap = 0;
    let mut count = 0;
    for (i, a) in tickets.iter().enumerate() {
        for b in &tickets[i + 1..] {
            overlap += a.front.iter().filter(|n| b.front.contains(n)).count()
                + a.back.iter().filter(|n| b.back.contains(n)).count();
            count += 1;
        }
    }
    json!({"front_union":fronts.len(),"back_union":backs.len(),"unique_back_pairs":pairs.len(),"back_pair_total":66,"mean_pairwise_number_overlap":if count>0 {overlap as f64/count as f64}else{0.},"coverage_score":fronts.len() as f64/35.+backs.len() as f64/12.+pairs.len() as f64/66.})
}
pub fn optimize(
    history: &[Draw],
    seed: u64,
    input: &OptimizeRequest,
    progress: Progress<'_>,
) -> Result<Value> {
    ensure!(
        input.request.mode == Mode::Single && (1..=30).contains(&input.request.bets),
        "结构优化支持1～30注单式，同预算比较"
    );
    ensure!(
        (1000..=20000).contains(&input.trials),
        "优化模拟次数须1000～20000"
    );
    ensure!(
        ["coverage", "seen", "recovery"].contains(&input.objective.as_str()),
        "目标须为coverage、seen或recovery"
    );
    let mut rows = Vec::new();
    for (arm, strategy) in [Strategy::Random, Strategy::Coverage, Strategy::Structural]
        .into_iter()
        .enumerate()
    {
        let mut request = input.request.clone();
        request.strategy = strategy;
        let plan = generate(&request, seed, history)?;
        let train = simulate_with(&plan, input.trials, 0x545241494e, &|done, total| {
            progress(arm * total + done, total * 6)
        })?;
        let structure = metrics(&plan.tickets);
        let score = match input.objective.as_str() {
            "coverage" => structure["coverage_score"].as_f64().unwrap(),
            "recovery" => train["fixed_recovery_probability_low"].as_f64().unwrap(),
            _ => train["any_prize_probability"].as_f64().unwrap(),
        };
        rows.push(json!({"plan":plan,"structure":structure,"selection_simulation":train,"selection_score":score}));
    }
    let best = (0..rows.len())
        .max_by(|&a, &b| {
            rows[a]["selection_score"]
                .as_f64()
                .unwrap()
                .total_cmp(&rows[b]["selection_score"].as_f64().unwrap())
        })
        .unwrap();
    for (arm, row) in rows.iter_mut().enumerate() {
        let plan = serde_json::from_value(row["plan"].clone())?;
        row["validation_simulation"] =
            simulate_with(&plan, input.trials, 0x56414c4944415445, &|done, total| {
                progress((arm + 3) * total + done, total * 6)
            })?;
    }
    let plan: crate::plan::Plan = serde_json::from_value(rows[best]["plan"].clone())?;
    let validation = rows[best]["validation_simulation"].clone();
    progress(1, 1)?;
    Ok(
        json!({"objective":input.objective,"budget_fen":rows[0]["plan"]["cost_fen"],"candidates":rows,"selected_index":best,"selected_plan":plan,"independent_validation":validation,"history_hash":digest(&history)?,"note":"固定三种单式结构，同预算选择；验证模拟使用独立随机样本，不参与选优。回本仅含标准固定奖，是完整回本概率的下界；覆盖评分=前区并集/35+后区并集/12+不同后区组合/66，不是中奖概率；选中后才验证所有候选，不再按验证样本改选。"}),
    )
}
