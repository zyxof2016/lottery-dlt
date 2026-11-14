use crate::model::{choose, Draw, Ticket, TOTAL_OUTCOMES};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

pub fn rule_version(issue: &str) -> Result<&'static str> {
    crate::model::issue(issue)?;
    ensure!(
        issue >= "19019",
        "19019 期之前的旧奖表暂不核算，可用于号码统计"
    );
    Ok(if issue >= "26014" {
        "dlt-2026-7"
    } else {
        "dlt-2019-9"
    })
}

pub fn level(front: u8, back: u8, modern: bool) -> Option<u8> {
    match (front, back, modern) {
        (5, 2, _) => Some(1),
        (5, 1, _) => Some(2),
        (5, 0, _) => Some(3),
        (4, 2, true) => Some(3),
        (4, 1, true) => Some(4),
        (4, 0, true) | (3, 2, true) => Some(5),
        (3, 1, true) | (2, 2, true) => Some(6),
        (3, 0, true) | (2, 1, true) | (1, 2, true) | (0, 2, true) => Some(7),
        (4, 2, false) => Some(4),
        (4, 1, false) => Some(5),
        (3, 2, false) => Some(6),
        (4, 0, false) => Some(7),
        (3, 1, false) | (2, 2, false) => Some(8),
        (3, 0, false) | (2, 1, false) | (1, 2, false) | (0, 2, false) => Some(9),
        _ => None,
    }
}

pub fn fixed_yuan(level: u8, modern: bool, high_pool: bool) -> Option<u64> {
    match (modern, high_pool, level) {
        (true, false, 3..=7) => Some([5000, 300, 150, 15, 5][(level - 3) as usize]),
        (true, true, 3..=7) => Some([6666, 380, 200, 18, 7][(level - 3) as usize]),
        (false, _, 3..=9) => Some([10000, 3000, 300, 200, 100, 15, 5][(level - 3) as usize]),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketResult {
    pub ticket: Ticket,
    pub front_hits: u8,
    pub back_hits: u8,
    pub level: Option<u8>,
    pub amount_fen: Option<u64>,
    pub amount_source: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settlement {
    pub issue: String,
    pub rule_version: String,
    pub cost_fen: u64,
    pub known_prize_fen: u64,
    pub total_prize_fen: Option<u64>,
    pub net_fen: Option<i64>,
    pub winning_tickets: usize,
    pub unpriced_tickets: usize,
    pub tickets: Vec<TicketResult>,
}

pub fn settle(
    tickets: &[Ticket],
    additional: bool,
    multiplier: u8,
    draw: &Draw,
) -> Result<Settlement> {
    let draw = draw.validate()?;
    let version = rule_version(&draw.issue)?;
    ensure!(
        !tickets.is_empty() && tickets.len() <= crate::model::MAX_TICKETS,
        "无效票组数量"
    );
    ensure!((1..=99).contains(&multiplier), "倍数须为 1～99");
    let modern = draw.issue.as_str() >= "26014";
    let mut results = Vec::with_capacity(tickets.len());
    for ticket in tickets {
        let ticket = ticket.validate()?;
        let (front_hits, back_hits) = ticket.hits(&draw);
        let prize_level = level(front_hits, back_hits, modern);
        let (amount, source) = match prize_level {
            None => (Some(0), "no_prize"),
            Some(n) => {
                let award = draw.awards.get(&n);
                let official = award.and_then(|a| a.basic_fen);
                let fixed = if !modern || draw.pool_before_fen.is_some() {
                    fixed_yuan(
                        n,
                        modern,
                        draw.pool_before_fen.unwrap_or(0) >= 80_000_000_000,
                    )
                    .map(|v| v * 100)
                } else {
                    None
                };
                let basic = official.or(fixed);
                let amount = if additional && n <= 2 {
                    // Official additional awards are rounded down to whole yuan;
                    // require the published amount instead of fabricating it.
                    basic
                        .zip(award.and_then(|a| a.additional_fen))
                        .map(|(b, a)| b + a)
                } else {
                    basic
                };
                (
                    amount,
                    if amount.is_none() {
                        "pending_official"
                    } else if official.is_some() {
                        "published"
                    } else {
                        "standard_rules"
                    },
                )
            }
        };
        results.push(TicketResult {
            ticket,
            front_hits,
            back_hits,
            level: prize_level,
            amount_fen: amount.map(|v| v * multiplier as u64),
            amount_source: source.into(),
        });
    }
    let cost = tickets.len() as u64 * if additional { 300 } else { 200 } * multiplier as u64;
    let known = results.iter().filter_map(|r| r.amount_fen).sum::<u64>();
    let unpriced = results.iter().filter(|r| r.amount_fen.is_none()).count();
    Ok(Settlement {
        issue: draw.issue,
        rule_version: version.into(),
        cost_fen: cost,
        known_prize_fen: known,
        total_prize_fen: (unpriced == 0).then_some(known),
        net_fen: (unpriced == 0).then_some(known as i64 - cost as i64),
        winning_tickets: results.iter().filter(|r| r.level.is_some()).count(),
        unpriced_tickets: unpriced,
        tickets: results,
    })
}

pub fn theory() -> serde_json::Value {
    let mut rows = Vec::new();
    let mut any_count = 0;
    let mut low_expectation = 0.;
    let mut high_expectation = 0.;
    for front in 0..=5 {
        for back in 0..=2 {
            let outcomes =
                choose(5, front) * choose(30, 5 - front) * choose(2, back) * choose(10, 2 - back);
            let probability = outcomes as f64 / TOTAL_OUTCOMES as f64;
            let grade = level(front as u8, back as u8, true);
            if let Some(n) = grade {
                any_count += outcomes;
                low_expectation += probability * fixed_yuan(n, true, false).unwrap_or(0) as f64;
                high_expectation += probability * fixed_yuan(n, true, true).unwrap_or(0) as f64;
            }
            rows.push(serde_json::json!({"front_hits":front,"back_hits":back,"level":grade,"outcomes":outcomes,"probability":probability,
                "low_pool_yuan": grade.and_then(|n| fixed_yuan(n,true,false)),"high_pool_yuan":grade.and_then(|n|fixed_yuan(n,true,true))}));
        }
    }
    serde_json::json!({"rule_version":"dlt-2026-7","total_outcomes":TOTAL_OUTCOMES,"first_prize_probability":1. / TOTAL_OUTCOMES as f64,
        "any_prize_probability":any_count as f64 / TOTAL_OUTCOMES as f64,"basic_cost_yuan":2,"additional_cost_yuan":3,
        "fixed_prize_expectation_low_yuan":low_expectation,"fixed_prize_expectation_high_yuan":high_expectation,
        "note":"概率按公平独立开奖计算；一二等奖为浮动奖，固定奖期望不等于总奖金期望。追加只参与一二等奖。","rows":rows})
}
