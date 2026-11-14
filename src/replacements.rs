//! Read-only replacement and overlap diagnostics over original archived tickets.
use crate::{model::Draw, store::SavedPlan};
use anyhow::{ensure, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn compare_named(
    a: &SavedPlan,
    b: &SavedPlan,
    a_label: &str,
    b_label: &str,
    draw: Option<&Draw>,
) -> Result<Value> {
    ensure!(
        a.plan.request.issue == b.plan.request.issue
            && draw.is_none_or(|d| d.issue == a.plan.request.issue),
        "替换对照期号不一致"
    );
    let mut pair = crate::budget::compare_named(a, b, a_label, b_label, draw);
    let (a, b) = if pair["from_plan_id"] == a.id {
        (a, b)
    } else {
        (b, a)
    };
    let mut reasons = Vec::new();
    for (same, reason) in [
        (a.plan.cost_fen == b.plan.cost_fen, "投入金额不同"),
        (a.plan.tickets.len() == b.plan.tickets.len(), "注数不同"),
        (
            a.plan.request.additional == b.plan.request.additional,
            "追加设置不同",
        ),
        (
            a.plan.request.multiplier == b.plan.request.multiplier,
            "倍数不同",
        ),
    ] {
        if !same {
            reasons.push(reason);
        }
    }
    pair["same_budget"] = json!(reasons.is_empty());
    pair["budget_mismatch_reasons"] = json!(reasons);
    pair["ticket_count_each"] = json!(if reasons.is_empty() {
        Some(a.plan.tickets.len())
    } else {
        None
    });
    pair["additional"] = json!(a.plan.request.additional);
    pair["multiplier"] = json!(a.plan.request.multiplier);
    pair["from_data_cutoff"] = json!(a.plan.data_cutoff);
    pair["to_data_cutoff"] = json!(b.plan.data_cutoff);
    for (name, front, upper) in [("front", true, 35), ("back", false, 12)] {
        let pool = |r: &SavedPlan| {
            r.plan
                .tickets
                .iter()
                .flat_map(|t| if front { &t.front } else { &t.back })
                .copied()
                .collect::<BTreeSet<_>>()
        };
        let (left, right) = (pool(a), pool(b));
        let common: Vec<_> = left.intersection(&right).copied().collect();
        let union: Vec<_> = left.union(&right).copied().collect();
        let (k, l, n, i) = (
            left.len() as f64,
            right.len() as f64,
            upper as f64,
            common.len() as f64,
        );
        let denominator = (k * (n - k) * l * (n - l)).sqrt();
        let zone = pair[name].as_object_mut().unwrap();
        zone.insert("from_size".into(), json!(left.len()));
        zone.insert("to_size".into(), json!(right.len()));
        zone.insert("same_size".into(), json!(left.len() == right.len()));
        zone.insert("common".into(), json!(common));
        zone.insert("union".into(), json!(union));
        zone.insert(
            "jaccard".into(),
            json!((!union.is_empty()).then(|| i / union.len() as f64)),
        );
        zone.insert("uniform_expected_overlap".into(), json!(k * l / n));
        zone.insert(
            "selection_indicator_correlation".into(),
            json!((denominator > 0.).then(|| (n * i - k * l) / denominator)),
        );
        zone.insert(
            "correlation_status".into(),
            json!(if denominator > 0. {
                "defined"
            } else {
                "constant_selection_indicator"
            }),
        );
    }
    pair["same_zone_sizes"] =
        json!(pair["front"]["same_size"] == true && pair["back"]["same_size"] == true);
    pair["identical_zone_pools"] = json!(
        pair["front"]["added"].as_array().unwrap().is_empty()
            && pair["front"]["removed"].as_array().unwrap().is_empty()
            && pair["back"]["added"].as_array().unwrap().is_empty()
            && pair["back"]["removed"].as_array().unwrap().is_empty()
    );
    let left: BTreeSet<_> = a.plan.tickets.iter().collect();
    let right: BTreeSet<_> = b.plan.tickets.iter().collect();
    pair["identical_complete_tickets"] = json!(left == right);
    Ok(pair)
}

pub fn summarize(periods: &[Value]) -> Value {
    // Keep budget and the two pairs of pool sizes fixed within each summary.
    let mut groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    let mut mismatches = Vec::new();
    for period in periods {
        for pair in period["source_comparisons"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if pair["same_budget"] != true {
                mismatches.push(json!({"issue":pair["issue"],"from_label":pair["from_label"],"to_label":pair["to_label"],"from_plan_id":pair["from_plan_id"],"to_plan_id":pair["to_plan_id"],"reasons":pair["budget_mismatch_reasons"]}));
                continue;
            }
            let key = json!([
                pair["from_label"],
                pair["to_label"],
                pair["from_cost_fen"],
                pair["ticket_count_each"],
                pair["additional"],
                pair["multiplier"],
                pair["front"]["from_size"],
                pair["front"]["to_size"],
                pair["back"]["from_size"],
                pair["back"]["to_size"]
            ])
            .to_string();
            groups.entry(key).or_default().push(pair);
        }
    }
    let comparisons:Vec<_>=groups.into_values().map(|mut all|{
        all.sort_by(|a,b|a["issue"].as_str().cmp(&b["issue"].as_str()));
        let settled:Vec<_>=all.iter().copied().filter(|p|p["drawn"]==true).collect();
        let priced:Vec<_>=settled.iter().copied().filter(|p|p["net_delta_fen"].is_i64()).collect();
        let zone=|name:&str|{
            let sum_hits=|key:&str|settled.iter().map(|p|p[name][key].as_array().unwrap().len() as i64).sum::<i64>();
            let added=sum_hits("added_hits");let removed=sum_hits("removed_hits");
            json!({"from_size":all[0][name]["from_size"],"to_size":all[0][name]["to_size"],
                "added_hit_count":(!settled.is_empty()).then_some(added),"removed_hit_count":(!settled.is_empty()).then_some(removed),
                "net_hit_delta":(!settled.is_empty()).then_some(added-removed),"mean_hit_delta":(!settled.is_empty()).then(||(added-removed) as f64/settled.len() as f64),
                "better_periods":settled.iter().filter(|p|p[name]["hit_delta"].as_i64().unwrap()>0).count(),
                "tied_periods":settled.iter().filter(|p|p[name]["hit_delta"]==0).count(),
                "worse_periods":settled.iter().filter(|p|p[name]["hit_delta"].as_i64().unwrap()<0).count()})
        };
        let net=priced.iter().map(|p|p["net_delta_fen"].as_i64().unwrap()).sum::<i64>();
        json!({"from_label":all[0]["from_label"],"to_label":all[0]["to_label"],"cost_fen_each":all[0]["from_cost_fen"],
            "ticket_count_each":all[0]["ticket_count_each"],"additional":all[0]["additional"],"multiplier":all[0]["multiplier"],
            "same_zone_sizes":all[0]["same_zone_sizes"],"common_periods":settled.len(),"pending_periods":all.len()-settled.len(),
            "front":zone("front"),"back":zone("back"),"priced_common_periods":priced.len(),
            "net_delta_fen":(!settled.is_empty()&&priced.len()==settled.len()).then_some(net),"priced_net_delta_fen":(!priced.is_empty()).then_some(net),"pairs":all})
    }).collect();
    json!({"comparisons":comparisons,"budget_mismatches":mismatches,
        "note":"只对照同一期合格原始留档；共同期要求金额、注数、追加和倍数一致，并按两区池大小分别分组。差值均为对照方案减原方案。方向在开奖前可由成本、注数、名称和原始编号确定，不按结果选优。补回命中减去丢失命中才是净变化；两区大小均相同时才是等容量换号。待开奖不计零分，金额缺失保持未知。入选指示相关性只描述集合相似度，不能推断独立信息源或预测优势；均匀期望交集仅为固定池大小的参照。"})
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::Ticket,
        plan::{generate, GenerateRequest},
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
        let settlement = draw.map(|d| crate::prizes::settle(&plan.tickets, false, 1, d).unwrap());
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
            front: vec![1, 2, 3, 4, 9],
            back: vec![1, 2],
            pool_before_fen: Some(0),
            awards: Default::default(),
            source: "synthetic-replacement-tests".into(),
        }
    }
    #[test]
    fn replacement_subtracts_lost_hits_and_overlap_uses_the_correct_zone_population() {
        let d = draw();
        let a = record("a", vec![ticket(&[1, 2, 3, 4, 5], &[1, 2])], Some(&d));
        let b = record("b", vec![ticket(&[1, 2, 6, 7, 8], &[2, 3])], Some(&d));
        let pair = compare_named(&a, &b, "a", "b", Some(&d)).unwrap();
        assert_eq!(pair["same_budget"], true);
        assert_eq!(pair["same_zone_sizes"], true);
        assert_eq!(pair["front"]["added_hits"], json!([]));
        assert_eq!(pair["front"]["removed_hits"], json!([3, 4]));
        assert_eq!(pair["front"]["hit_delta"], -2);
        assert_eq!(pair["back"]["hit_delta"], -1);
        assert_eq!(pair["front"]["common"], json!([1, 2]));
        assert_eq!(pair["front"]["jaccard"], 0.25);
        assert_eq!(pair["front"]["selection_indicator_correlation"], 0.3);
        assert_eq!(pair["back"]["selection_indicator_correlation"], 0.4);
        assert_eq!(pair["front"]["uniform_expected_overlap"], 25. / 35.);
        assert_eq!(pair, compare_named(&b, &a, "b", "a", Some(&d)).unwrap());
        let pending = compare_named(&a, &b, "a", "b", None).unwrap();
        for zone in ["front", "back"] {
            for key in [
                "added",
                "removed",
                "common",
                "union",
                "selection_indicator_correlation",
            ] {
                assert_eq!(pair[zone][key], pending[zone][key]);
            }
            assert!(pending[zone]["hit_delta"].is_null());
        }
    }
    #[test]
    fn full_zone_overlap_has_no_indicator_variance_and_identical_pools_do_not_imply_identical_tickets(
    ) {
        let front = [1, 2, 3, 4, 5];
        let a = record(
            "a",
            (0..6)
                .map(|i| ticket(&front, &[i * 2 + 1, i * 2 + 2]))
                .collect(),
            None,
        );
        let b = record(
            "b",
            (0..6)
                .map(|i| {
                    let mut back = vec![(i * 2 + 1) % 12 + 1, (i * 2 + 2) % 12 + 1];
                    back.sort();
                    ticket(&front, &back)
                })
                .collect(),
            None,
        );
        let pair = compare_named(&a, &b, "a", "b", None).unwrap();
        assert_eq!(pair["identical_zone_pools"], true);
        assert_eq!(pair["identical_complete_tickets"], false);
        assert_eq!(pair["back"]["jaccard"], 1.);
        assert!(pair["back"]["selection_indicator_correlation"].is_null());
        assert_eq!(
            pair["back"]["correlation_status"],
            "constant_selection_indicator"
        );
        assert_eq!(pair["front"]["selection_indicator_correlation"], 1.);
    }
    #[test]
    fn equal_cost_does_not_override_multiplier_or_additional_or_issue_mismatches() {
        let a = record("a", vec![ticket(&[1, 2, 3, 4, 5], &[1, 2])], None);
        let mut b = a.clone();
        b.id = "b".into();
        b.plan.id = "b".into();
        b.plan.request.multiplier = 2;
        b.plan.request.additional = true;
        let pair = compare_named(&a, &b, "a", "b", None).unwrap();
        assert_eq!(pair["same_budget"], false);
        assert_eq!(
            pair["budget_mismatch_reasons"],
            json!(["追加设置不同", "倍数不同"])
        );
        let summary = summarize(&[json!({"source_comparisons":[pair]})]);
        assert_eq!(summary["comparisons"], json!([]));
        assert_eq!(summary["budget_mismatches"].as_array().unwrap().len(), 1);
        b.plan.request.issue = "26031".into();
        assert!(compare_named(&a, &b, "a", "b", None).is_err());
        let mut d = draw();
        d.issue = "26032".into();
        assert!(compare_named(&a, &a, "a", "a", Some(&d)).is_err());
    }
    #[test]
    fn summaries_keep_negative_gains_and_pending_and_unknown_money_separate() {
        let d = draw();
        let a = record("a", vec![ticket(&d.front, &d.back)], Some(&d));
        let b = record("b", vec![ticket(&[10, 11, 12, 13, 14], &[3, 4])], Some(&d));
        let settled = compare_named(&a, &b, "a", "b", Some(&d)).unwrap();
        let pending = compare_named(&a, &b, "a", "b", None).unwrap();
        let r = summarize(&[json!({"source_comparisons":[settled,pending.clone()]})]);
        let s = &r["comparisons"][0];
        assert_eq!(s["common_periods"], 1);
        assert_eq!(s["pending_periods"], 1);
        assert_eq!(s["front"]["net_hit_delta"], -5);
        assert_eq!(s["front"]["worse_periods"], 1);
        assert_eq!(s["front"]["better_periods"], 0);
        assert!(s["net_delta_fen"].is_null());
        assert_eq!(s["priced_common_periods"], 0);
        let r = summarize(&[json!({"source_comparisons":[pending]})]);
        assert!(r["comparisons"][0]["front"]["net_hit_delta"].is_null());
        assert!(r["comparisons"][0]["net_delta_fen"].is_null());
    }
    #[test]
    fn different_pool_sizes_are_not_pooled_across_the_review_window() {
        let tickets = vec![
            ticket(&[1, 2, 3, 4, 5], &[1, 2]),
            ticket(&[1, 2, 3, 4, 6], &[2, 3]),
        ];
        let a = record("a", tickets.clone(), None);
        let b = record("b", tickets, None);
        let first = compare_named(&a, &b, "a", "b", None).unwrap();
        let mut changed = b.clone();
        changed.plan.tickets[1] = ticket(&[6, 7, 8, 9, 10], &[3, 4]);
        let second = compare_named(&a, &changed, "a", "b", None).unwrap();
        assert_eq!(second["same_zone_sizes"], false);
        let r = summarize(&[json!({"source_comparisons":[first,second]})]);
        assert_eq!(r["comparisons"].as_array().unwrap().len(), 2);
    }
}
