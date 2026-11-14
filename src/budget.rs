//! Descriptive comparisons of already archived plans, ordered without draw labels.
use crate::{
    model::{Draw, Ticket},
    store::SavedPlan,
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn capacity(tickets: &[Ticket]) -> Value {
    let zone = |front: bool, upper: usize, size: usize| {
        let pool: BTreeSet<_> = tickets
            .iter()
            .flat_map(|t| if front { &t.front } else { &t.back })
            .copied()
            .collect();
        let positions = tickets.len() * size;
        json!({"positions":positions,"unique_numbers":pool.len(),"repeated_positions":positions-pool.len(),
            "union_upper_bound":upper.min(positions),"unused_union_capacity":upper.min(positions)-pool.len(),
            "at_unconstrained_union_bound":pool.len()==upper.min(positions)})
    };
    json!({"front":zone(true,35,5),"back":zone(false,12,2),
        "note":"位置数给出不额外限制复式、胆拖或自选池的并集上界；达到上界不代表中奖或收益最优。"})
}

fn zone_change(a: &[u8], b: &[u8], draw: Option<&[u8]>) -> Value {
    let added: Vec<_> = b.iter().copied().filter(|n| !a.contains(n)).collect();
    let removed: Vec<_> = a.iter().copied().filter(|n| !b.contains(n)).collect();
    let hits = |values: &[u8]| {
        draw.map(|d| {
            values
                .iter()
                .copied()
                .filter(|n| d.contains(n))
                .collect::<Vec<_>>()
        })
    };
    let added_hits = hits(&added);
    let removed_hits = hits(&removed);
    json!({"added":added,"removed":removed,"preserves_original_union":removed.is_empty(),
        "added_hits":added_hits,"removed_hits":removed_hits,
        "hit_delta":added_hits.as_ref().zip(removed_hits.as_ref()).map(|(a,b)|a.len() as i64-b.len() as i64)})
}

pub fn compare(a: &SavedPlan, b: &SavedPlan, draw: Option<&Draw>) -> Value {
    compare_named(a, b, &a.label, &b.label, draw)
}

pub fn compare_named(
    a: &SavedPlan,
    b: &SavedPlan,
    a_label: &str,
    b_label: &str,
    draw: Option<&Draw>,
) -> Value {
    let key = |r: &SavedPlan, label: &str| {
        (
            r.plan.cost_fen,
            r.plan.tickets.len(),
            label.to_string(),
            r.id.clone(),
        )
    };
    let (a, b, a_label, b_label) = if key(a, a_label) <= key(b, b_label) {
        (a, b, a_label, b_label)
    } else {
        (b, a, b_label, a_label)
    };
    let pool = |r: &SavedPlan, front: bool| {
        r.plan
            .tickets
            .iter()
            .flat_map(|t| if front { &t.front } else { &t.back })
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>()
    };
    let front = zone_change(
        &pool(a, true),
        &pool(b, true),
        draw.map(|d| d.front.as_slice()),
    );
    let back = zone_change(
        &pool(a, false),
        &pool(b, false),
        draw.map(|d| d.back.as_slice()),
    );
    let left: BTreeSet<_> = a.plan.tickets.iter().collect();
    let right: BTreeSet<_> = b.plan.tickets.iter().collect();
    let delta = b.plan.cost_fen as i64 - a.plan.cost_fen as i64;
    let prize =
        |r: &SavedPlan| draw.and_then(|_| r.settlement.as_ref().and_then(|s| s.total_prize_fen));
    let prize_delta = prize(a).zip(prize(b)).map(|(a, b)| b as i64 - a as i64);
    let per_yuan = |value: &Value| {
        value["hit_delta"]
            .as_i64()
            .filter(|_| delta > 0)
            .map(|n| n as f64 * 100. / delta as f64)
    };
    json!({"issue":a.plan.request.issue,"from_label":a_label,"to_label":b_label,
        "from_plan_id":a.id,"to_plan_id":b.id,"from_cost_fen":a.plan.cost_fen,"to_cost_fen":b.plan.cost_fen,
        "cost_delta_fen":delta,"drawn":draw.is_some(),"front":front,"back":back,
        "preserves_both_unions":front["preserves_original_union"]==true && back["preserves_original_union"]==true,
        "removed_complete_tickets":left.difference(&right).count(),"added_complete_tickets":right.difference(&left).count(),
        "preserves_complete_tickets":left.is_subset(&right),"prize_delta_fen":prize_delta,
        "net_delta_fen":prize_delta.map(|p|p-delta),
        "front_hits_per_extra_yuan":per_yuan(&front),"back_hits_per_extra_yuan":per_yuan(&back)})
}

pub fn summarize(periods: &[Value]) -> Value {
    let mut groups: BTreeMap<(String, String), Vec<&Value>> = BTreeMap::new();
    for period in periods {
        if let Some(pairs) = period["budget_comparisons"].as_array() {
            for pair in pairs {
                groups
                    .entry((
                        pair["from_label"].as_str().unwrap().into(),
                        pair["to_label"].as_str().unwrap().into(),
                    ))
                    .or_default()
                    .push(pair);
            }
        }
    }
    let comparisons: Vec<_> = groups.into_iter().map(|((from,to),all)| {
        let rows: Vec<_> = all.iter().copied().filter(|r|r["drawn"]==true).collect();
        let cost: i64 = rows.iter().map(|r|r["cost_delta_fen"].as_i64().unwrap()).sum();
        let hits = |zone: &str|rows.iter().map(|r|r[zone]["hit_delta"].as_i64().unwrap()).sum::<i64>();
        let mean = |zone: &str| (!rows.is_empty()).then(||hits(zone) as f64/rows.len() as f64);
        let priced: Vec<_> = rows.iter().filter(|r|r["net_delta_fen"].is_i64()).collect();
        let full_net = (!rows.is_empty() && priced.len()==rows.len()).then(||priced.iter().map(|r|r["net_delta_fen"].as_i64().unwrap()).sum::<i64>());
        json!({"from_label":from,"to_label":to,"common_periods":rows.len(),"pending_periods":all.len()-rows.len(),
            "cost_delta_fen":cost,"mean_front_hit_delta":mean("front"),"mean_back_hit_delta":mean("back"),
            "front_hits_per_extra_yuan":(cost>0).then(||hits("front") as f64*100./cost as f64),
            "back_hits_per_extra_yuan":(cost>0).then(||hits("back") as f64*100./cost as f64),
            "both_unions_preserved_periods":rows.iter().filter(|r|r["preserves_both_unions"]==true).count(),
            "complete_tickets_preserved_periods":rows.iter().filter(|r|r["preserves_complete_tickets"]==true).count(),
            "priced_common_periods":priced.len(),"net_delta_fen":full_net,
            "priced_net_delta_fen":(!priced.is_empty()).then(||priced.iter().map(|r|r["net_delta_fen"].as_i64().unwrap()).sum::<i64>()),
            "priced_cost_delta_fen":priced.iter().map(|r|r["cost_delta_fen"].as_i64().unwrap()).sum::<i64>(),
            "issues":rows.iter().map(|r|&r["issue"]).collect::<Vec<_>>(),"pairs":all})
    }).collect();
    json!({"comparisons":comparisons,"ordering":"cost_then_ticket_count_then_label_then_plan_id",
        "note":"只比较同一期实际整期留档中合格的方案，按成本、注数和原始身份固定方向，新增减去原方案；不按开奖结果选优。两区并集保留不等于完整5+2单注保留。未开奖不计零分，奖额缺失时完整金额差为未知，同成本的每新增元指标留空。"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        plan::{generate, GenerateRequest},
        prizes::settle,
    };
    fn record(label: &str, tickets: Vec<Ticket>, draw: Option<&Draw>) -> SavedPlan {
        let mut plan = generate(
            &GenerateRequest {
                issue: "26030".into(),
                ..Default::default()
            },
            42,
            &[],
        )
        .unwrap();
        plan.id = label.into();
        plan.cost_fen = tickets.len() as u64 * 200;
        plan.tickets = tickets;
        let settlement = draw.map(|d| settle(&plan.tickets, false, 1, d).unwrap());
        SavedPlan {
            id: label.into(),
            label: label.into(),
            kind: "recommendation".into(),
            created_at: "2026-03-01T10:00:00Z".into(),
            deadline: "2026-03-01T13:00:00Z".into(),
            plan,
            settlement,
            integrity_valid: true,
            schedule: None,
        }
    }
    fn ticket(front: &[u8], back: &[u8]) -> Ticket {
        Ticket {
            front: front.to_vec(),
            back: back.to_vec(),
        }
    }
    fn draw() -> Draw {
        Draw {
            issue: "26030".into(),
            date: "2026-03-01".into(),
            front: vec![7, 8, 9, 10, 11],
            back: vec![4, 5],
            awards: Default::default(),
            pool_before_fen: Some(0),
            source: "synthetic-test".into(),
        }
    }
    #[test]
    fn same_unions_do_not_imply_preserved_complete_tickets() {
        let a = record(
            "a",
            vec![
                ticket(&[1, 2, 3, 4, 5], &[1, 2]),
                ticket(&[2, 3, 4, 5, 6], &[2, 3]),
            ],
            None,
        );
        let b = record(
            "b",
            vec![
                ticket(&[1, 2, 3, 4, 5], &[2, 3]),
                ticket(&[2, 3, 4, 5, 6], &[1, 2]),
            ],
            None,
        );
        let pair = compare(&a, &b, None);
        assert_eq!(pair["preserves_both_unions"], true);
        assert_eq!(pair["preserves_complete_tickets"], false);
        assert_eq!(pair["removed_complete_tickets"], 2);
        assert_eq!(pair["added_complete_tickets"], 2);
        assert!(pair["front_hits_per_extra_yuan"].is_null());
        assert!(pair["front"]["hit_delta"].is_null());
        assert_eq!(pair, compare(&b, &a, None));
    }
    #[test]
    fn added_and_removed_draw_hits_account_for_actual_coverage_delta() {
        let draw = draw();
        let a = record(
            "original",
            vec![ticket(&[1, 2, 3, 4, 5], &[1, 2])],
            Some(&draw),
        );
        let mut tickets = a.plan.tickets.clone();
        tickets.push(ticket(&draw.front, &draw.back));
        let b = record("expanded", tickets, Some(&draw));
        let pair = compare(&a, &b, Some(&draw));
        assert_eq!(pair["preserves_complete_tickets"], true);
        assert_eq!(pair["cost_delta_fen"], 200);
        assert_eq!(pair["front"]["added_hits"], json!([7, 8, 9, 10, 11]));
        assert_eq!(pair["front"]["removed_hits"], json!([]));
        assert_eq!(pair["front"]["hit_delta"], 5);
        assert_eq!(pair["back"]["hit_delta"], 2);
        assert_eq!(pair["front_hits_per_extra_yuan"], 2.5);
        assert!(pair["net_delta_fen"].is_null());
        let reverse = compare_named(&b, &a, "other", "first", Some(&draw));
        assert_eq!(reverse["from_plan_id"], "original");
    }
    #[test]
    fn non_nested_larger_plans_report_lost_hits_and_capacity_bounds() {
        let draw = draw();
        let a = record("a", vec![ticket(&draw.front, &draw.back)], Some(&draw));
        let b = record(
            "b",
            vec![
                ticket(&[1, 2, 3, 4, 5], &[1, 2]),
                ticket(&[1, 2, 3, 4, 6], &[2, 3]),
            ],
            Some(&draw),
        );
        let pair = compare(&a, &b, Some(&draw));
        assert_eq!(pair["preserves_both_unions"], false);
        assert_eq!(pair["front"]["hit_delta"], -5);
        assert_eq!(pair["back"]["hit_delta"], -2);
        let capacity = capacity(&b.plan.tickets);
        assert_eq!(capacity["front"]["positions"], 10);
        assert_eq!(capacity["front"]["unique_numbers"], 6);
        assert_eq!(capacity["front"]["repeated_positions"], 4);
        assert_eq!(capacity["front"]["union_upper_bound"], 10);
    }
}
