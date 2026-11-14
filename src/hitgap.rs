//! Read-only split of jackpot distance and prize composition.
//! Reuses the settlement already computed for the original tickets.
use crate::{model::Ticket, prizes::Settlement};
use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn same_numbers(left: &[u8], right: &[u8]) -> bool {
    let mut left = left.to_vec();
    let mut right = right.to_vec();
    left.sort_unstable();
    right.sort_unstable();
    left == right
}

fn zone(best: usize, union_hits: usize, target: usize) -> Result<Value> {
    ensure!(
        best <= union_hits && union_hits <= target && best <= target,
        "单注命中不能超过号码池命中或开奖个数"
    );
    let outside = target - union_hits;
    let scatter = union_hits - best;
    let gap = target - best;
    ensure!(
        outside + scatter == gap,
        "池外与未集中差额之和必须等于距满额的差距"
    );
    Ok(json!({
        "union_hits": union_hits,
        "best_ticket_hits": best,
        "gap": gap,
        "outside_pool": outside,
        "in_pool_not_on_best_ticket": scatter
    }))
}

fn flag_pair(
    settlement: &Settlement,
    floating_known: u64,
    floating_complete: bool,
    fixed_known: u64,
    fixed_complete: bool,
) -> (Option<bool>, Option<bool>) {
    let cost = settlement.cost_fen;
    let floating_covers = if floating_known >= cost {
        Some(true)
    } else if floating_complete {
        Some(false)
    } else {
        None
    };
    let fixed_assisted = match floating_covers {
        Some(true) => Some(false),
        None => None,
        Some(false) => {
            let lower = floating_known + fixed_known;
            if fixed_complete || lower >= cost {
                Some(lower >= cost)
            } else {
                None
            }
        }
    };
    (floating_covers, fixed_assisted)
}

pub fn diagnose(
    tickets: &[Ticket],
    settlement: Option<&Settlement>,
    front_union_hits: Option<usize>,
    back_union_hits: Option<usize>,
) -> Result<Value> {
    if front_union_hits.is_none() && back_union_hits.is_none() {
        return Ok(json!({
            "status": "pending",
            "ticket_count": tickets.len(),
            "jackpot_tickets": Value::Null,
            "union_covers_both_zones": Value::Null,
            "front": Value::Null,
            "back": Value::Null,
            "joint": Value::Null,
            "prize_split": Value::Null,
            "floating_covers_cost": Value::Null,
            "fixed_assisted_recovery": Value::Null
        }));
    }
    let (Some(settlement), Some(front_union_hits), Some(back_union_hits)) =
        (settlement, front_union_hits, back_union_hits)
    else {
        bail!("开奖命中与结算状态不一致");
    };
    ensure!(
        settlement.tickets.len() == tickets.len() && !tickets.is_empty(),
        "结算票数与原始票组不符"
    );
    for (saved, settled) in tickets.iter().zip(&settlement.tickets) {
        ensure!(
            same_numbers(&saved.front, &settled.ticket.front)
                && same_numbers(&saved.back, &settled.ticket.back),
            "结算票面与原始票组不符"
        );
    }
    let best_front = settlement
        .tickets
        .iter()
        .map(|ticket| ticket.front_hits as usize)
        .max()
        .unwrap();
    let best_back = settlement
        .tickets
        .iter()
        .map(|ticket| ticket.back_hits as usize)
        .max()
        .unwrap();
    let front = zone(best_front, front_union_hits, 5)?;
    let back = zone(best_back, back_union_hits, 2)?;
    let mut groups: BTreeMap<(u8, u8), Vec<usize>> = BTreeMap::new();
    for (index, ticket) in settlement.tickets.iter().enumerate() {
        groups
            .entry((ticket.front_hits, ticket.back_hits))
            .or_default()
            .push(index);
    }
    let best_gap = groups
        .keys()
        .map(|(front_hits, back_hits)| (5 - *front_hits as usize) + (2 - *back_hits as usize))
        .min()
        .unwrap();
    let mut patterns = Vec::new();
    let mut outside_explains = true;
    for ((front_hits, back_hits), indexes) in &groups {
        let gap = (5 - *front_hits as usize) + (2 - *back_hits as usize);
        if gap != best_gap {
            continue;
        }
        let level = settlement.tickets[indexes[0]].level;
        ensure!(
            indexes
                .iter()
                .all(|index| settlement.tickets[*index].level == level),
            "相同命中的奖级不一致"
        );
        let front_scatter = front_union_hits.min(5) - *front_hits as usize;
        let back_scatter = back_union_hits.min(2) - *back_hits as usize;
        let front_outside = 5 - front_union_hits.min(5);
        let back_outside = 2 - back_union_hits.min(2);
        ensure!(front_outside + front_scatter + back_outside + back_scatter == gap);
        outside_explains &= front_scatter == 0 && back_scatter == 0;
        let examples: Vec<_> = indexes
            .iter()
            .take(3)
            .map(|index| {
                let ticket = &settlement.tickets[*index].ticket;
                json!({"front": ticket.front, "back": ticket.back})
            })
            .collect();
        patterns.push(json!({
            "front_hits": front_hits,
            "back_hits": back_hits,
            "gap": gap,
            "ticket_count": indexes.len(),
            "level": level,
            "front_outside_pool": front_outside,
            "front_scatter": front_scatter,
            "back_outside_pool": back_outside,
            "back_scatter": back_scatter,
            "examples": examples
        }));
    }
    let jackpot_tickets = settlement
        .tickets
        .iter()
        .filter(|ticket| ticket.front_hits == 5 && ticket.back_hits == 2)
        .count();
    ensure!(
        (jackpot_tickets > 0) == (best_gap == 0),
        "头奖注数与最近单注差距不一致"
    );
    let mut floating_tickets = 0;
    let mut fixed_tickets = 0;
    let mut no_prize_tickets = 0;
    let mut unpriced_floating = 0;
    let mut unpriced_fixed = 0;
    let mut floating_known = 0u64;
    let mut fixed_known = 0u64;
    for ticket in &settlement.tickets {
        match ticket.level {
            None => {
                ensure!(ticket.amount_fen == Some(0), "未中奖注的金额必须为0");
                no_prize_tickets += 1;
            }
            Some(level) if (1..=2).contains(&level) => {
                floating_tickets += 1;
                match ticket.amount_fen {
                    Some(amount) => floating_known += amount,
                    None => unpriced_floating += 1,
                }
            }
            Some(level) if (3..=9).contains(&level) => {
                fixed_tickets += 1;
                match ticket.amount_fen {
                    Some(amount) => fixed_known += amount,
                    None => unpriced_fixed += 1,
                }
            }
            Some(level) => bail!("未知奖级 {level}"),
        }
    }
    ensure!(floating_tickets + fixed_tickets == settlement.winning_tickets);
    ensure!(no_prize_tickets + settlement.winning_tickets == settlement.tickets.len());
    ensure!(settlement.unpriced_tickets == unpriced_floating + unpriced_fixed);
    let floating_complete = unpriced_floating == 0;
    let fixed_complete = unpriced_fixed == 0;
    if floating_complete && fixed_complete {
        ensure!(
            settlement.total_prize_fen == Some(floating_known + fixed_known),
            "浮动奖与固定奖之和不等于原结算奖金"
        );
    }
    let (floating_covers, fixed_assisted) = flag_pair(
        settlement,
        floating_known,
        floating_complete,
        fixed_known,
        fixed_complete,
    );
    Ok(json!({
        "status": "drawn",
        "ticket_count": tickets.len(),
        "jackpot_tickets": jackpot_tickets,
        "union_covers_both_zones": front_union_hits >= 5 && back_union_hits >= 2,
        "front": front,
        "back": back,
        "joint": {
            "best_gap": best_gap,
            "outside_explains_entire_gap": outside_explains,
            "patterns": patterns
        },
        "prize_split": {
            "floating_tickets": floating_tickets,
            "fixed_tickets": fixed_tickets,
            "no_prize_tickets": no_prize_tickets,
            "unpriced_floating_tickets": unpriced_floating,
            "unpriced_fixed_tickets": unpriced_fixed,
            "floating_prize_fen": floating_complete.then_some(floating_known),
            "fixed_prize_fen": fixed_complete.then_some(fixed_known)
        },
        "floating_covers_cost": floating_covers,
        "fixed_assisted_recovery": fixed_assisted
    }))
}

fn adjacent(previous: Option<&str>, issue: &str) -> bool {
    previous.is_some_and(|previous| {
        previous.len() == 5
            && issue.len() == 5
            && previous[..2] == issue[..2]
            && previous
                .parse::<u32>()
                .ok()
                .zip(issue.parse::<u32>().ok())
                .is_some_and(|(left, right)| left + 1 == right)
    })
}

fn check_drawn(gap: &Value) -> Result<()> {
    ensure!(gap["status"] == "drawn", "已开奖档案缺少单注差额");
    for (zone, target) in [("front", 5u64), ("back", 2)] {
        let item = &gap[zone];
        let outside = item["outside_pool"].as_u64().context("缺少池外差额")?;
        let scatter = item["in_pool_not_on_best_ticket"]
            .as_u64()
            .context("缺少未集中差额")?;
        let zone_gap = item["gap"].as_u64().context("缺少分区差距")?;
        ensure!(outside + scatter == zone_gap, "分区差额拆分不一致");
        ensure!(item["best_ticket_hits"].as_u64().unwrap() + zone_gap == target);
    }
    let joint = &gap["joint"];
    let best_gap = joint["best_gap"].as_u64().context("缺少最近单注差距")?;
    let mut explains = true;
    let patterns = joint["patterns"].as_array().context("缺少最近单注")?;
    ensure!(!patterns.is_empty(), "最近单注不能为空");
    for pattern in patterns {
        let gap = pattern["gap"].as_u64().unwrap();
        ensure!(gap == best_gap, "最近单注差距不一致");
        let parts = pattern["front_outside_pool"].as_u64().unwrap()
            + pattern["front_scatter"].as_u64().unwrap()
            + pattern["back_outside_pool"].as_u64().unwrap()
            + pattern["back_scatter"].as_u64().unwrap();
        ensure!(parts == gap, "最近单注的池外与未集中之和不一致");
        explains &= pattern["front_scatter"].as_u64() == Some(0)
            && pattern["back_scatter"].as_u64() == Some(0);
    }
    ensure!(joint["outside_explains_entire_gap"] == explains);
    Ok(())
}

fn component_sum(rows: &[&Value], key: &str) -> Option<u64> {
    if rows.is_empty() {
        return None;
    }
    let mut total = 0u64;
    for row in rows {
        total += row["hit_gap"]["prize_split"][key].as_u64()?;
    }
    Some(total)
}

fn count_flag(rows: &[&Value], key: &str, expected: bool) -> usize {
    rows.iter()
        .filter(|row| row["hit_gap"][key] == expected)
        .count()
}

pub fn summarize(rows: &[&Value]) -> Result<Value> {
    let mut ordered = rows.to_vec();
    ordered.sort_by(|left, right| left["issue"].as_str().cmp(&right["issue"].as_str()));
    let settled: Vec<_> = ordered
        .iter()
        .copied()
        .filter(|row| !row["settlement"].is_null())
        .collect();
    let pending = ordered.len() - settled.len();
    for row in &settled {
        check_drawn(&row["hit_gap"])?;
        ensure!(
            row["cost_fen"].as_u64() == row["settlement"]["cost_fen"].as_u64(),
            "原票成本与结算成本不一致"
        );
        if let (Some(floating), Some(fixed), Some(total)) = (
            row["hit_gap"]["prize_split"]["floating_prize_fen"].as_u64(),
            row["hit_gap"]["prize_split"]["fixed_prize_fen"].as_u64(),
            row["settlement"]["total_prize_fen"].as_u64(),
        ) {
            ensure!(
                floating + fixed == total,
                "浮动奖与固定奖之和不等于原结算奖金"
            );
        }
    }
    for row in ordered.iter().filter(|row| row["settlement"].is_null()) {
        ensure!(
            row["hit_gap"]["status"] == "pending",
            "待开奖被写成了已开奖差额"
        );
        ensure!(row["hit_gap"]["joint"].is_null() && row["hit_gap"]["jackpot_tickets"].is_null());
    }
    let drawn_gaps: Vec<f64> = settled
        .iter()
        .map(|row| row["hit_gap"]["joint"]["best_gap"].as_u64().unwrap() as f64)
        .collect();
    let floating_prize = component_sum(&settled, "floating_prize_fen");
    let fixed_prize = component_sum(&settled, "fixed_prize_fen");
    if let (Some(floating), Some(fixed)) = (floating_prize, fixed_prize) {
        let prizes: Option<Vec<u64>> = settled
            .iter()
            .map(|row| row["settlement"]["total_prize_fen"].as_u64())
            .collect();
        if let Some(prizes) = prizes {
            ensure!(floating + fixed == prizes.iter().sum::<u64>());
        }
    }
    let fully_priced = !settled.is_empty()
        && settled
            .iter()
            .all(|row| row["settlement"]["net_fen"].is_i64());
    let max_drawdown = if fully_priced {
        let mut cumulative = 0i64;
        let mut peak = 0i64;
        let mut drawdown = 0i64;
        for row in &settled {
            cumulative += row["settlement"]["net_fen"].as_i64().unwrap();
            peak = peak.max(cumulative);
            drawdown = drawdown.max(peak - cumulative);
        }
        Some(drawdown)
    } else {
        None
    };
    let mut run = 0usize;
    let mut longest = 0usize;
    let mut previous = None;
    let mut interrupted = false;
    let mut priced_periods = 0usize;
    for row in &settled {
        let issue = row["issue"].as_str().context("缺少期号")?;
        if !adjacent(previous, issue) {
            run = 0;
        }
        match row["settlement"]["net_fen"].as_i64() {
            None => {
                interrupted = true;
                run = 0;
            }
            Some(net) => {
                priced_periods += 1;
                if net < 0 {
                    run += 1;
                    longest = longest.max(run);
                } else {
                    run = 0;
                }
            }
        }
        previous = Some(issue);
    }
    let recovery_unknown = settled
        .iter()
        .filter(|row| {
            row["hit_gap"]["floating_covers_cost"].is_null()
                || row["hit_gap"]["fixed_assisted_recovery"].is_null()
        })
        .count();
    let trace = ordered
        .iter()
        .map(|row| {
            let gap = &row["hit_gap"];
            let drawn = gap["status"] == "drawn";
            json!({
                "issue": row["issue"],
                "plan_id": row["plan_id"],
                "batch_hash": row["batch_hash"],
                "status": gap["status"],
                "joint_gap": drawn.then(|| gap["joint"]["best_gap"].clone()),
                "outside_explains_entire_gap": drawn.then(|| gap["joint"]["outside_explains_entire_gap"].clone()),
                "patterns": drawn.then(|| gap["joint"]["patterns"].clone()),
                "jackpot_tickets": gap["jackpot_tickets"],
                "union_covers_both_zones": gap["union_covers_both_zones"],
                "front_outside_pool": drawn.then(|| gap["front"]["outside_pool"].clone()),
                "front_scatter": drawn.then(|| gap["front"]["in_pool_not_on_best_ticket"].clone()),
                "back_outside_pool": drawn.then(|| gap["back"]["outside_pool"].clone()),
                "back_scatter": drawn.then(|| gap["back"]["in_pool_not_on_best_ticket"].clone()),
                "floating_tickets": drawn.then(|| gap["prize_split"]["floating_tickets"].clone()),
                "fixed_tickets": drawn.then(|| gap["prize_split"]["fixed_tickets"].clone()),
                "floating_prize_fen": drawn.then(|| gap["prize_split"]["floating_prize_fen"].clone()),
                "fixed_prize_fen": drawn.then(|| gap["prize_split"]["fixed_prize_fen"].clone()),
                "floating_covers_cost": gap["floating_covers_cost"],
                "fixed_assisted_recovery": gap["fixed_assisted_recovery"],
                "net_fen": row["settlement"]["net_fen"]
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "settled_periods": settled.len(),
        "pending_periods": pending,
        "priced_periods": priced_periods,
        "jackpot_periods": settled.iter().filter(|row| row["hit_gap"]["jackpot_tickets"].as_u64().unwrap_or(0) > 0).count(),
        "union_covered_without_jackpot_periods": settled.iter().filter(|row| row["hit_gap"]["union_covers_both_zones"] == true && row["hit_gap"]["jackpot_tickets"].as_u64() == Some(0)).count(),
        "front_outside_periods": settled.iter().filter(|row| row["hit_gap"]["front"]["outside_pool"].as_u64().unwrap_or(0) > 0).count(),
        "front_scatter_periods": settled.iter().filter(|row| row["hit_gap"]["front"]["in_pool_not_on_best_ticket"].as_u64().unwrap_or(0) > 0).count(),
        "back_outside_periods": settled.iter().filter(|row| row["hit_gap"]["back"]["outside_pool"].as_u64().unwrap_or(0) > 0).count(),
        "back_scatter_periods": settled.iter().filter(|row| row["hit_gap"]["back"]["in_pool_not_on_best_ticket"].as_u64().unwrap_or(0) > 0).count(),
        "mean_joint_gap": (!drawn_gaps.is_empty()).then(|| drawn_gaps.iter().sum::<f64>() / drawn_gaps.len() as f64),
        "floating_ticket_periods": settled.iter().filter(|row| row["hit_gap"]["prize_split"]["floating_tickets"].as_u64().unwrap_or(0) > 0).count(),
        "fixed_ticket_periods": settled.iter().filter(|row| row["hit_gap"]["prize_split"]["fixed_tickets"].as_u64().unwrap_or(0) > 0).count(),
        "floating_prize_fen": floating_prize,
        "fixed_prize_fen": fixed_prize,
        "floating_recovered_periods": count_flag(&settled, "floating_covers_cost", true),
        "fixed_assisted_recovered_periods": count_flag(&settled, "fixed_assisted_recovery", true),
        "recovery_unknown_periods": recovery_unknown,
        "max_drawdown_fen": max_drawdown,
        "drawdown_status": if settled.is_empty() {"no_settled_periods"} else if fully_priced {"complete"} else {"missing_awards"},
        "longest_loss_run": (priced_periods > 0).then_some(longest),
        "loss_run_status": if priced_periods == 0 {"no_priced_periods"} else if interrupted {"interrupted_by_unknown"} else {"complete"},
        "trace": trace,
        "note": "只读取原票结算。距头奖差拆成池外不足和已进池但未出现在该区最高命中注上；两区最高命中不一定属于同一注。一二等奖为浮动奖，三等奖及以后为固定奖。浮动奖回本指浮动奖本身不少于本期成本；总奖金回到成本但浮动奖不够时，记为固定奖补足，不算浮动奖成绩。金额未知时回本分类、累计奖金和回撤保持未知，不把未知记为0。连亏只计净额小于0的同年连续期，金额未知或期号不连续会打断。回撤沿实际留档的已开奖净额计算，缺档不补零。这些描述不用于更换选号。"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{Award, Draw, Ticket},
        prizes::settle,
    };
    use std::collections::BTreeMap;

    fn ticket(front: [u8; 5], back: [u8; 2]) -> Ticket {
        Ticket {
            front: front.to_vec(),
            back: back.to_vec(),
        }
    }
    fn draw(
        issue: &str,
        date: &str,
        front: [u8; 5],
        back: [u8; 2],
        level: Option<u8>,
        amount: Option<u64>,
    ) -> Draw {
        let mut awards = BTreeMap::new();
        if let Some(level) = level {
            awards.insert(
                level,
                Award {
                    basic_fen: amount,
                    additional_fen: None,
                },
            );
        }
        Draw {
            issue: issue.into(),
            date: date.into(),
            front: front.to_vec(),
            back: back.to_vec(),
            awards,
            pool_before_fen: Some(0),
            source: "unit-test".into(),
        }
    }
    fn union_hits(tickets: &[Ticket], draw: &Draw, front: bool) -> usize {
        tickets
            .iter()
            .flat_map(|ticket| if front { &ticket.front } else { &ticket.back })
            .filter(|number| {
                if front {
                    draw.front.contains(number)
                } else {
                    draw.back.contains(number)
                }
            })
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    }
    fn observation(
        issue: &str,
        date: &str,
        tickets: &[Ticket],
        drawn: Option<([u8; 5], [u8; 2])>,
    ) -> Value {
        let draw = drawn.map(|(front, back)| draw(issue, date, front, back, None, None));
        let settlement = draw
            .as_ref()
            .map(|draw| settle(tickets, false, 1, draw).unwrap());
        let gap = diagnose(
            tickets,
            settlement.as_ref(),
            draw.as_ref().map(|draw| union_hits(tickets, draw, true)),
            draw.as_ref().map(|draw| union_hits(tickets, draw, false)),
        )
        .unwrap();
        json!({
            "issue": issue,
            "plan_id": format!("plan-{issue}"),
            "batch_hash": "abc",
            "cost_fen": settlement.as_ref().map(|item| item.cost_fen).unwrap_or(200),
            "settlement": settlement,
            "hit_gap": gap
        })
    }

    #[test]
    fn scatter_on_the_closest_ticket_is_not_called_an_outside_miss() {
        let tickets = vec![
            ticket([1, 2, 3, 4, 6], [1, 3]),
            ticket([1, 2, 7, 8, 9], [1, 2]),
        ];
        let draw = draw("26030", "2026-03-02", [1, 2, 3, 4, 5], [1, 2], None, None);
        let gap = diagnose(
            &tickets,
            Some(&settle(&tickets, false, 1, &draw).unwrap()),
            Some(union_hits(&tickets, &draw, true)),
            Some(union_hits(&tickets, &draw, false)),
        )
        .unwrap();
        assert_eq!(gap["jackpot_tickets"], 0);
        assert_eq!(gap["union_covers_both_zones"], false);
        assert_eq!(gap["front"]["outside_pool"], 1);
        assert_eq!(gap["front"]["in_pool_not_on_best_ticket"], 0);
        assert_eq!(gap["back"]["outside_pool"], 0);
        assert_eq!(gap["back"]["in_pool_not_on_best_ticket"], 0);
        assert_eq!(gap["joint"]["best_gap"], 2);
        assert_eq!(gap["joint"]["outside_explains_entire_gap"], false);
        assert_eq!(gap["joint"]["patterns"][0]["front_hits"], 4);
        assert_eq!(gap["joint"]["patterns"][0]["back_hits"], 1);
        assert_eq!(gap["joint"]["patterns"][0]["back_scatter"], 1);
        assert_eq!(gap["prize_split"]["floating_tickets"], 0);
        assert!(gap["prize_split"]["fixed_prize_fen"].as_u64().unwrap() > 0);
    }

    #[test]
    fn outside_numbers_explain_the_whole_gap_when_nothing_in_the_pool_was_left_off_the_ticket() {
        let tickets = vec![ticket([1, 2, 3, 4, 6], [1, 3])];
        let draw = draw("26030", "2026-03-02", [1, 2, 3, 4, 5], [1, 2], None, None);
        let gap = diagnose(
            &tickets,
            Some(&settle(&tickets, false, 1, &draw).unwrap()),
            Some(4),
            Some(1),
        )
        .unwrap();
        assert_eq!(gap["front"]["gap"], 1);
        assert_eq!(gap["back"]["gap"], 1);
        assert_eq!(gap["joint"]["best_gap"], 2);
        assert_eq!(gap["joint"]["outside_explains_entire_gap"], true);
        assert_eq!(gap["union_covers_both_zones"], false);
    }

    #[test]
    fn union_cover_without_a_jackpot_ticket_stays_a_scatter_gap() {
        let tickets = vec![
            ticket([1, 2, 3, 4, 5], [1, 3]),
            ticket([6, 7, 8, 9, 10], [1, 2]),
        ];
        let draw = draw("26030", "2026-03-02", [1, 2, 3, 4, 5], [1, 2], None, None);
        let gap = diagnose(
            &tickets,
            Some(&settle(&tickets, false, 1, &draw).unwrap()),
            Some(5),
            Some(2),
        )
        .unwrap();
        assert_eq!(gap["union_covers_both_zones"], true);
        assert_eq!(gap["jackpot_tickets"], 0);
        assert_eq!(gap["joint"]["best_gap"], 1);
        assert_eq!(gap["joint"]["outside_explains_entire_gap"], false);
        assert_eq!(gap["front"]["in_pool_not_on_best_ticket"], 0);
        assert_eq!(gap["back"]["in_pool_not_on_best_ticket"], 0);
    }

    #[test]
    fn fixed_prize_recovery_is_not_a_floating_prize_recovery() {
        let tickets = vec![ticket([6, 7, 8, 9, 10], [1, 2])];
        let draw = draw("26030", "2026-03-02", [1, 2, 3, 4, 5], [1, 2], None, None);
        let settlement = settle(&tickets, false, 1, &draw).unwrap();
        assert_eq!(settlement.tickets[0].level, Some(7));
        assert_eq!(settlement.tickets[0].amount_fen, Some(500));
        let gap = diagnose(&tickets, Some(&settlement), Some(0), Some(2)).unwrap();
        assert_eq!(gap["prize_split"]["floating_prize_fen"], 0);
        assert_eq!(gap["prize_split"]["fixed_prize_fen"], 500);
        assert_eq!(gap["floating_covers_cost"], false);
        assert_eq!(gap["fixed_assisted_recovery"], true);
        let row = observation(
            "26030",
            "2026-03-02",
            &tickets,
            Some(([1, 2, 3, 4, 5], [1, 2])),
        );
        let summary = summarize(&[&row]).unwrap();
        assert_eq!(summary["floating_recovered_periods"], 0);
        assert_eq!(summary["fixed_assisted_recovered_periods"], 1);
        assert_eq!(summary["recovery_unknown_periods"], 0);
        assert_eq!(summary["max_drawdown_fen"], 0);
        assert_eq!(summary["drawdown_status"], "complete");
        assert_eq!(summary["longest_loss_run"], 0);
    }

    #[test]
    fn missing_floating_prize_keeps_drawdown_and_recovery_unknown() {
        let tickets = vec![ticket([1, 2, 3, 4, 5], [1, 2])];
        let draw = draw("26030", "2026-03-02", [1, 2, 3, 4, 5], [1, 2], None, None);
        let settlement = settle(&tickets, false, 1, &draw).unwrap();
        assert_eq!(settlement.tickets[0].level, Some(1));
        assert!(settlement.total_prize_fen.is_none());
        let gap = diagnose(&tickets, Some(&settlement), Some(5), Some(2)).unwrap();
        assert_eq!(gap["jackpot_tickets"], 1);
        assert!(gap["prize_split"]["floating_prize_fen"].is_null());
        assert!(gap["floating_covers_cost"].is_null());
        assert!(gap["fixed_assisted_recovery"].is_null());
        let row = json!({
            "issue": "26030", "plan_id": "p", "batch_hash": "abc", "cost_fen": settlement.cost_fen,
            "settlement": settlement, "hit_gap": gap
        });
        let summary = summarize(&[&row]).unwrap();
        assert_eq!(summary["jackpot_periods"], 1);
        assert!(summary["floating_prize_fen"].is_null());
        assert!(summary["max_drawdown_fen"].is_null());
        assert_eq!(summary["drawdown_status"], "missing_awards");
        assert_eq!(summary["recovery_unknown_periods"], 1);
        assert!(summary["longest_loss_run"].is_null());
        assert_eq!(summary["loss_run_status"], "no_priced_periods");
    }

    #[test]
    fn known_floating_prize_can_cover_cost_before_a_fixed_prize_is_published() {
        let tickets = vec![
            ticket([1, 2, 3, 4, 5], [1, 2]),
            ticket([1, 2, 3, 4, 6], [1, 3]),
        ];
        let mut draw = draw(
            "26030",
            "2026-03-02",
            [1, 2, 3, 4, 5],
            [1, 2],
            Some(1),
            Some(50_000),
        );
        draw.pool_before_fen = None;
        let settlement = settle(&tickets, false, 1, &draw).unwrap();
        assert!(settlement
            .tickets
            .iter()
            .any(|ticket| ticket.level == Some(4) && ticket.amount_fen.is_none()));
        assert!(settlement.total_prize_fen.is_none());
        let gap = diagnose(&tickets, Some(&settlement), Some(5), Some(2)).unwrap();
        assert_eq!(gap["floating_covers_cost"], true);
        assert_eq!(gap["fixed_assisted_recovery"], false);
        assert!(gap["prize_split"]["fixed_prize_fen"].is_null());
        assert_eq!(gap["prize_split"]["floating_prize_fen"], 50_000);
    }

    #[test]
    fn old_fourth_prize_stays_in_the_fixed_component() {
        let tickets = vec![ticket([1, 2, 3, 4, 6], [1, 2])];
        let draw = draw("19019", "2019-02-20", [1, 2, 3, 4, 5], [1, 2], None, None);
        let settlement = settle(&tickets, false, 1, &draw).unwrap();
        assert_eq!(settlement.tickets[0].level, Some(4));
        let gap = diagnose(&tickets, Some(&settlement), Some(4), Some(2)).unwrap();
        assert_eq!(gap["prize_split"]["floating_tickets"], 0);
        assert_eq!(gap["prize_split"]["fixed_tickets"], 1);
        assert_eq!(gap["fixed_assisted_recovery"], true);
    }

    #[test]
    fn loss_run_breaks_on_a_missing_issue_or_unknown_money_and_drawdown_uses_the_archived_path() {
        let miss = vec![ticket([11, 12, 13, 14, 15], [3, 4])];
        let first = observation(
            "26030",
            "2026-03-02",
            &miss,
            Some(([1, 2, 3, 4, 5], [1, 2])),
        );
        let second = observation(
            "26031",
            "2026-03-04",
            &miss,
            Some(([1, 2, 3, 4, 5], [1, 2])),
        );
        let summary = summarize(&[&second, &first]).unwrap();
        assert_eq!(summary["max_drawdown_fen"], 400);
        assert_eq!(summary["longest_loss_run"], 2);
        assert_eq!(summary["loss_run_status"], "complete");
        let later = observation(
            "26032",
            "2026-03-06",
            &miss,
            Some(([1, 2, 3, 4, 5], [1, 2])),
        );
        let gapped = summarize(&[&first, &later]).unwrap();
        assert_eq!(gapped["longest_loss_run"], 1);
        assert_eq!(gapped["max_drawdown_fen"], 400);
        let mut unknown = first.clone();
        unknown["settlement"]["total_prize_fen"] = Value::Null;
        unknown["settlement"]["net_fen"] = Value::Null;
        unknown["hit_gap"]["prize_split"]["floating_prize_fen"] = Value::Null;
        unknown["hit_gap"]["floating_covers_cost"] = Value::Null;
        unknown["hit_gap"]["fixed_assisted_recovery"] = Value::Null;
        let interrupted = summarize(&[&unknown, &second]).unwrap();
        assert!(interrupted["max_drawdown_fen"].is_null());
        assert_eq!(interrupted["drawdown_status"], "missing_awards");
        assert_eq!(interrupted["longest_loss_run"], 1);
        assert_eq!(interrupted["loss_run_status"], "interrupted_by_unknown");
        assert_eq!(interrupted["trace"][0]["issue"], "26030");
    }

    #[test]
    fn pending_and_empty_samples_do_not_become_zero_results() {
        let tickets = vec![ticket([1, 2, 3, 4, 5], [1, 2])];
        let pending = diagnose(&tickets, None, None, None).unwrap();
        assert_eq!(pending["status"], "pending");
        assert!(pending["jackpot_tickets"].is_null());
        assert!(pending["joint"].is_null());
        let row = json!({
            "issue": "26032", "plan_id": "p", "batch_hash": "abc", "cost_fen": 200,
            "settlement": Value::Null, "hit_gap": pending
        });
        let summary = summarize(&[&row]).unwrap();
        assert_eq!(summary["settled_periods"], 0);
        assert_eq!(summary["pending_periods"], 1);
        assert!(summary["mean_joint_gap"].is_null());
        assert!(summary["max_drawdown_fen"].is_null());
        assert_eq!(summary["drawdown_status"], "no_settled_periods");
        assert!(summary["longest_loss_run"].is_null());
        assert!(summary["floating_prize_fen"].is_null());
        let empty = summarize(&[]).unwrap();
        assert_eq!(empty["jackpot_periods"], 0);
        assert_eq!(empty["drawdown_status"], "no_settled_periods");
        assert!(empty["max_drawdown_fen"].is_null());
    }

    #[test]
    fn inconsistent_union_or_component_sums_refuse_the_report() {
        let tickets = vec![ticket([1, 2, 3, 4, 5], [1, 2])];
        let draw = draw(
            "26030",
            "2026-03-02",
            [1, 2, 3, 4, 5],
            [1, 2],
            Some(1),
            Some(50_000),
        );
        let settlement = settle(&tickets, false, 1, &draw).unwrap();
        assert!(diagnose(&tickets, Some(&settlement), Some(0), Some(2)).is_err());
        assert!(diagnose(&tickets, None, Some(5), None).is_err());
        let gap = diagnose(&tickets, Some(&settlement), Some(5), Some(2)).unwrap();
        let mut row = json!({
            "issue": "26030", "plan_id": "p", "batch_hash": "abc", "cost_fen": 200,
            "settlement": settlement, "hit_gap": gap
        });
        row["hit_gap"]["prize_split"]["floating_prize_fen"] = json!(1);
        assert!(summarize(&[&row]).is_err());
        row["hit_gap"]["prize_split"]["floating_prize_fen"] = json!(50_000);
        row["hit_gap"]["front"]["gap"] = json!(9);
        assert!(summarize(&[&row]).is_err());
    }

    #[test]
    fn a_gain_then_a_loss_measures_drawdown_from_the_peak() {
        let win = vec![ticket([6, 7, 8, 9, 10], [1, 2])];
        let loss = vec![ticket([11, 12, 13, 14, 15], [3, 4])];
        let first = observation("26030", "2026-03-02", &win, Some(([1, 2, 3, 4, 5], [1, 2])));
        let second = observation(
            "26031",
            "2026-03-04",
            &loss,
            Some(([1, 2, 3, 4, 5], [1, 2])),
        );
        let summary = summarize(&[&first, &second]).unwrap();
        assert_eq!(first["settlement"]["net_fen"], 300);
        assert_eq!(second["settlement"]["net_fen"], -200);
        assert_eq!(summary["max_drawdown_fen"], 200);
        assert_eq!(summary["longest_loss_run"], 1);
        assert_eq!(summary["fixed_assisted_recovered_periods"], 1);
    }
}
