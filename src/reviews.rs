//! Read-only diagnostics over actual archives. Never regenerate historical tickets.
use crate::{digest, store::Store};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, FixedOffset, Utc};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

fn union(tickets: &[crate::model::Ticket], front: bool) -> Vec<u8> {
    tickets
        .iter()
        .flat_map(|t| if front { &t.front } else { &t.back })
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn mean(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<_> = values.collect();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn aggregate(rows: &[&Value]) -> Value {
    let settled: Vec<_> = rows.iter().filter(|r| !r["settlement"].is_null()).collect();
    let cost: u64 = settled
        .iter()
        .map(|r| r["cost_fen"].as_u64().unwrap())
        .sum();
    let pending_cost: u64 = rows
        .iter()
        .filter(|r| r["settlement"].is_null())
        .map(|r| r["cost_fen"].as_u64().unwrap())
        .sum();
    let priced: Vec<_> = settled
        .iter()
        .filter(|r| r["settlement"]["total_prize_fen"].is_u64())
        .collect();
    let priced_cost: u64 = priced.iter().map(|r| r["cost_fen"].as_u64().unwrap()).sum();
    let priced_prize: u64 = priced
        .iter()
        .map(|r| r["settlement"]["total_prize_fen"].as_u64().unwrap())
        .sum();
    let complete = !settled.is_empty() && settled.len() == priced.len();
    let archived_issues: BTreeSet<_> = rows
        .iter()
        .map(|r| &r["issue"])
        .map(Value::to_string)
        .collect();
    let settled_issues: BTreeSet<_> = settled.iter().map(|r| r["issue"].to_string()).collect();
    let pending_issues: BTreeSet<_> = rows
        .iter()
        .filter(|r| r["settlement"].is_null())
        .map(|r| r["issue"].to_string())
        .collect();
    let winning_issues: BTreeSet<_> = settled
        .iter()
        .filter(|r| r["settlement"]["winning_tickets"].as_u64().unwrap() > 0)
        .map(|r| r["issue"].to_string())
        .collect();
    json!({
        "archived_records":rows.len(), "settled_records":settled.len(),
        "pending_records":rows.len()-settled.len(),
        "archived_periods":archived_issues.len(), "settled_periods":settled_issues.len(),
        "pending_periods":pending_issues.len(), "pending_cost_fen":pending_cost,
        "settled_cost_fen":cost, "unpriced_periods":settled.len()-priced.len(),
        "total_prize_fen":complete.then_some(priced_prize),
        "net_fen":complete.then_some(priced_prize as i64-cost as i64),
        "priced_periods":priced.len(), "priced_cost_fen":priced_cost,
        "priced_prize_fen":priced_prize, "priced_net_fen":priced_prize as i64-priced_cost as i64,
        "winning_periods":winning_issues.len(),
        "winning_records":settled.iter().filter(|r| r["settlement"]["winning_tickets"].as_u64().unwrap()>0).count(),
        "winning_tickets":settled.iter().map(|r| r["settlement"]["winning_tickets"].as_u64().unwrap()).sum::<u64>(),
        "issue_win_rate":(!settled_issues.is_empty()).then(||winning_issues.len() as f64/settled_issues.len() as f64),
        "mean_front_union_hits":mean(settled.iter().map(|r| r["front_union_hits"].as_u64().unwrap() as f64)),
        "mean_back_union_hits":mean(settled.iter().map(|r| r["back_union_hits"].as_u64().unwrap() as f64)),
        "issues":rows.iter().map(|r| &r["issue"]).collect::<Vec<_>>()
    })
}

impl Store {
    pub fn live_review(&self, limit: usize) -> Result<Value> {
        ensure!((1..=120).contains(&limit), "复盘窗口须为1～120期");
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let draws: BTreeMap<_, _> = Self::read_draws(&tx)?
            .into_iter()
            .map(|d| (d.issue.clone(), d))
            .collect();
        let records = Self::read_records(&tx)?;
        let batches = Self::read_batches(&tx)?;
        let schedules = Self::read_schedules(&tx)?;
        let mut issues: BTreeSet<String> = batches
            .iter()
            .map(|b| b.issue.clone())
            .chain(schedules.iter().map(|s| s.issue.clone()))
            .chain(
                records
                    .iter()
                    .filter(|r| r.kind == "recommendation")
                    .map(|r| r.plan.request.issue.clone()),
            )
            .collect();
        // Gaps only after actual tracking began; never manufacture old archives.
        if let Some(first) = issues.first().cloned() {
            issues.extend(draws.keys().filter(|issue| **issue >= first).cloned());
        }
        let now = Utc::now();
        let offset = FixedOffset::east_opt(8 * 3600).unwrap();
        let mut periods = Vec::new();
        let mut observations = Vec::new();
        for issue in issues.into_iter().rev().take(limit) {
            let draw = draws.get(&issue);
            let batch = batches.iter().find(|b| b.issue == issue);
            let schedule = schedules.iter().find(|s| s.issue == issue);
            let mut entries = Vec::new();
            if let Some(batch) = batch {
                let created = DateTime::parse_from_rfc3339(&batch.created_at)?;
                let deadline = DateTime::parse_from_rfc3339(&batch.deadline)?;
                let mut labels = BTreeSet::new();
                let mut ids = BTreeSet::new();
                ensure!(!batch.entries.is_empty(), "推荐清单为空");
                for entry in &batch.entries {
                    ensure!(
                        labels.insert(&entry.label) && ids.insert(&entry.plan_id),
                        "推荐清单包含重复身份"
                    );
                    let record = records
                        .iter()
                        .find(|r| r.id == entry.plan_id)
                        .context("推荐清单缺少原始票组")?;
                    ensure!(
                        record.plan.request.issue == issue && record.id == record.plan.id,
                        "推荐清单身份或期号不符"
                    );
                    let saved_at = DateTime::parse_from_rfc3339(&record.created_at)?;
                    let saved_deadline = DateTime::parse_from_rfc3339(&record.deadline)?;
                    let reason = if created >= deadline
                        || saved_at >= saved_deadline
                        || saved_at > created
                    {
                        Some("归档时刻未早于截止或晚于整期清单")
                    } else if record
                        .plan
                        .data_cutoff
                        .as_ref()
                        .is_some_and(|c| c >= &issue)
                    {
                        Some("输入数据截止未早于目标期")
                    } else if draw.is_some_and(|d| {
                        deadline
                            .with_timezone(&offset)
                            .format("%Y-%m-%d")
                            .to_string()
                            != d.date
                    }) {
                        Some("归档截止日期与实际开奖日期不符")
                    } else {
                        None
                    };
                    let front = union(&record.plan.tickets, true);
                    let back = union(&record.plan.tickets, false);
                    let hits = |pool: &[u8], front: bool| -> Option<usize> {
                        draw.map(|d| {
                            pool.iter()
                                .filter(|n| {
                                    if front {
                                        d.front.contains(n)
                                    } else {
                                        d.back.contains(n)
                                    }
                                })
                                .count()
                        })
                    };
                    let front_union_hits = hits(&front, true);
                    let back_union_hits = hits(&back, false);
                    let hit_gap = crate::hitgap::diagnose(
                        &record.plan.tickets,
                        record.settlement.as_ref(),
                        front_union_hits,
                        back_union_hits,
                    )?;
                    let mut settlement = serde_json::to_value(&record.settlement)?;
                    if let Some(object) = settlement.as_object_mut() {
                        object.remove("tickets");
                    }
                    let row = json!({
                        "selection_evidence":Self::evidence_summaries(&tx,std::slice::from_ref(record))?[&record.id],
                        "issue":issue,"label":entry.label,"plan_id":record.id,
                        "batch_hash":digest(batch)?,"created_at":record.created_at,
                        "generator_version":record.plan.generator_version,
                        "data_cutoff":record.plan.data_cutoff,"cost_fen":record.plan.cost_fen,
                        "ticket_count":record.plan.tickets.len(),"additional":record.plan.request.additional,
                        "multiplier":record.plan.request.multiplier,
                        "front_union":front,"back_union":back,
                        "front_union_hits":front_union_hits,"back_union_hits":back_union_hits,
                        "hit_gap":hit_gap,
                        "front_repeated_positions":record.plan.tickets.len()*5-front.len(),
                        "back_repeated_positions":record.plan.tickets.len()*2-back.len(),
                        "capacity":crate::budget::capacity(&record.plan.tickets),
                        "eligible":reason.is_none(),"exclusion_reason":reason,
                        "settlement":settlement,
                        "schedule_evidence_present":record.schedule.is_some()
                    });
                    observations.push(row.clone());
                    entries.push(row);
                }
            }
            let status = if batch.is_none() {
                if draw.is_some()
                    || schedule.is_some_and(|s| {
                        DateTime::parse_from_rfc3339(&s.deadline).is_ok_and(|d| d <= now)
                    })
                {
                    "missed"
                } else {
                    "not_archived"
                }
            } else if entries.iter().any(|e| e["eligible"] == false) {
                "excluded"
            } else if draw.is_none() {
                "pending"
            } else if entries
                .iter()
                .any(|e| e["settlement"]["total_prize_fen"].is_null())
            {
                "prize_pending"
            } else {
                "settled"
            };
            let mut budget_comparisons = Vec::new();
            let mut source_comparisons = Vec::new();
            let included: Vec<_> = entries.iter().filter(|r| r["eligible"] == true).collect();
            for (i, left) in included.iter().enumerate() {
                for right in &included[i + 1..] {
                    let a = records.iter().find(|r| r.id == left["plan_id"]).unwrap();
                    let b = records.iter().find(|r| r.id == right["plan_id"]).unwrap();
                    budget_comparisons.push(crate::budget::compare_named(
                        a,
                        b,
                        left["label"].as_str().unwrap(),
                        right["label"].as_str().unwrap(),
                        draw,
                    ));
                    let mut source = crate::replacements::compare_named(
                        a,
                        b,
                        left["label"].as_str().unwrap(),
                        right["label"].as_str().unwrap(),
                        draw,
                    )?;
                    for side in ["from", "to"] {
                        let id = &source[format!("{side}_plan_id")];
                        let row = included.iter().find(|r| r["plan_id"] == *id).unwrap();
                        source[format!("{side}_selection_evidence")] =
                            row["selection_evidence"].clone();
                    }
                    source["batch_hash"] = json!(batch.map(digest).transpose()?);
                    source_comparisons.push(source);
                }
            }
            periods.push(json!({"issue":issue,"status":status,"draw":draw,"budget_comparisons":budget_comparisons,"source_comparisons":source_comparisons,
                "batch_hash":batch.map(digest).transpose()?,"created_at":batch.map(|b|&b.created_at),
                "deadline":batch.map(|b|&b.deadline).or(schedule.map(|s|&s.deadline)),"entries":entries,
                "unbatched_records":records.iter().filter(|r|r.kind=="recommendation" && r.plan.request.issue==issue && !batch.is_some_and(|b|b.entries.iter().any(|e|e.plan_id==r.id))).count()}));
        }
        let labels: BTreeSet<_> = observations
            .iter()
            .map(|r| r["label"].as_str().unwrap())
            .collect();
        let mut summaries = Vec::new();
        for label in &labels {
            let rows: Vec<_> = observations
                .iter()
                .filter(|r| r["label"] == *label && r["eligible"] == true)
                .collect();
            summaries.push(json!({"label":label,"summary":aggregate(&rows),"stability":crate::stability::summarize(&rows),
                "hit_review":crate::hitgap::summarize(&rows)?,
                "excluded_periods":observations.iter().filter(|r|r["label"]==*label && r["eligible"]==false).count()}));
        }
        let labels: Vec<_> = labels.into_iter().collect();
        let mut comparisons = Vec::new();
        for (i, a) in labels.iter().enumerate() {
            for b in &labels[i + 1..] {
                let mut pairs = Vec::new();
                let mut mismatched = Vec::new();
                for period in &periods {
                    let rows = period["entries"].as_array().unwrap();
                    let find = |name: &str| {
                        rows.iter().find(|r| {
                            r["label"] == name
                                && r["eligible"] == true
                                && !r["settlement"].is_null()
                        })
                    };
                    if let (Some(left), Some(right)) = (find(a), find(b)) {
                        if left["cost_fen"] != right["cost_fen"]
                            || left["ticket_count"] != right["ticket_count"]
                            || left["additional"] != right["additional"]
                            || left["multiplier"] != right["multiplier"]
                        {
                            mismatched.push(period["issue"].clone());
                            continue;
                        }
                        let delta = left["settlement"]["net_fen"]
                            .as_i64()
                            .zip(right["settlement"]["net_fen"].as_i64())
                            .map(|(a, b)| a - b);
                        pairs.push(json!({"issue":period["issue"],"left_plan_id":left["plan_id"],"right_plan_id":right["plan_id"],
                            "cost_fen_each":left["cost_fen"],"winning_ticket_delta":left["settlement"]["winning_tickets"].as_i64().unwrap()-right["settlement"]["winning_tickets"].as_i64().unwrap(),
                            "win_delta":i64::from(left["settlement"]["winning_tickets"].as_u64().unwrap()>0)-i64::from(right["settlement"]["winning_tickets"].as_u64().unwrap()>0),"net_delta_fen":delta}));
                    }
                }
                let net: Option<Vec<_>> =
                    pairs.iter().map(|p| p["net_delta_fen"].as_i64()).collect();
                comparisons.push(json!({"left":a,"right":b,"common_periods":pairs.len(),"budget_mismatch_issues":mismatched,
                    "mean_win_delta":mean(pairs.iter().map(|p|p["win_delta"].as_i64().unwrap() as f64)),
                    "mean_winning_ticket_delta":mean(pairs.iter().map(|p|p["winning_ticket_delta"].as_i64().unwrap() as f64)),
                    "total_net_delta_fen":net.filter(|v|!v.is_empty()).map(|v|v.iter().sum::<i64>()),"pairs":pairs}));
            }
        }
        let eligible: Vec<_> = observations
            .iter()
            .filter(|r| r["eligible"] == true)
            .collect();
        let mut report = json!({"schema_version":2,"requested_limit":limit,"period_count":periods.len(),
            "replacement_review":crate::replacements::summarize(&periods),
            "budget_increments":crate::budget::summarize(&periods),
            "summary":aggregate(&eligible),"strategies":summaries,"comparisons":comparisons,"periods":periods,
            "evidence":"local_archive_integrity_only",
            "note":"只统计实际整期留档，不重生成或补造推荐。缺档、未开奖与排除期不计零分。前后区并集命中不等于单注命中；单注差额另分池外不足和未集中到同一注。浮动奖回本与固定奖补足回本分开统计。共同期比较要求注数、追加、倍数与预算一致，但票组结构可能不同；结果仅描述实际表现，不证明预测优势。本地摘要和系统时钟不构成独立开奖前证明。"});
        report["content_hash"] = json!(digest(&report)?);
        Ok(report)
    }
}
