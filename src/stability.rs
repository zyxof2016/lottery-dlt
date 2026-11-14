//! Descriptive monitoring over one strategy's actual eligible issue archives.
use serde_json::{json, Value};
pub fn summarize(rows: &[&Value]) -> Value {
    let mut settled: Vec<_> = rows
        .iter()
        .copied()
        .filter(|r| !r["settlement"].is_null())
        .collect();
    settled.sort_by(|a, b| a["issue"].as_str().cmp(&b["issue"].as_str()));
    let priced: Vec<_> = settled
        .iter()
        .copied()
        .filter(|r| r["settlement"]["total_prize_fen"].is_u64())
        .collect();
    let total: u64 = priced
        .iter()
        .map(|r| r["settlement"]["total_prize_fen"].as_u64().unwrap())
        .sum();
    let complete = !settled.is_empty() && priced.len() == settled.len();
    let largest = priced
        .iter()
        .map(|r| r["settlement"]["total_prize_fen"].as_u64().unwrap())
        .max();
    let max_issues: Vec<_> = if complete && total > 0 {
        settled
            .iter()
            .filter(|r| r["settlement"]["total_prize_fen"].as_u64() == largest)
            .map(|r| &r["issue"])
            .collect()
    } else {
        vec![]
    };
    let range = |key: &str| {
        let values: Vec<_> = settled.iter().filter_map(|r| r[key].as_u64()).collect();
        if values.is_empty() {
            Value::Null
        } else {
            json!([values.iter().min().unwrap(), values.iter().max().unwrap()])
        }
    };
    let mut run = 0;
    let mut longest = 0;
    let mut previous: Option<&str> = None;
    for row in &settled {
        let issue = row["issue"].as_str().unwrap();
        if !previous.is_some_and(|p| {
            p[..2] == issue[..2]
                && p.parse::<u32>()
                    .ok()
                    .zip(issue.parse::<u32>().ok())
                    .is_some_and(|(a, b)| a + 1 == b)
        }) {
            run = 0;
        }
        if row["settlement"]["winning_tickets"].as_u64() == Some(0) {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
        previous = Some(issue);
    }
    json!({"settled_periods":settled.len(),"pending_periods":rows.len()-settled.len(),"priced_periods":priced.len(),
        "front_union_hit_range":range("front_union_hits"),"back_union_hit_range":range("back_union_hits"),
        "winning_periods":settled.iter().filter(|r|r["settlement"]["winning_tickets"].as_u64().unwrap()>0).count(),
        "profitable_periods":priced.iter().filter(|r|r["settlement"]["net_fen"].as_i64().unwrap()>0).count(),
        "recovered_periods":priced.iter().filter(|r|r["settlement"]["net_fen"].as_i64().unwrap()>=0).count(),
        "longest_consecutive_nonwinning_run":longest,
        "max_single_prize_fen":if complete {largest}else{None},"max_single_prize_issues":max_issues,
        "max_single_prize_share":if complete&&total>0 {largest.map(|n|n as f64/total as f64)}else{None},
        "concentration_status":if settled.is_empty(){"no_settled_periods"}else if !complete{"missing_awards"}else if total==0{"no_prize"}else{"complete"},
        "front_flat_score_periods":settled.iter().filter(|r|r["selection_evidence"]["front_all_scores_equal"]==true).count(),
        "back_flat_score_periods":settled.iter().filter(|r|r["selection_evidence"]["back_all_scores_equal"]==true).count(),
        "missing_evidence_periods":settled.iter().filter(|r|r["selection_evidence"]["status"]!="recorded").count(),
        "trace":settled.iter().map(|r|json!({"issue":r["issue"],"plan_id":r["plan_id"],"batch_hash":r["batch_hash"],"data_cutoff":r["data_cutoff"],
            "front_union_hits":r["front_union_hits"],"back_union_hits":r["back_union_hits"],"winning_tickets":r["settlement"]["winning_tickets"],
            "cost_fen":r["cost_fen"],"prize_fen":r["settlement"]["total_prize_fen"],"net_fen":r["settlement"]["net_fen"],"selection_evidence":r["selection_evidence"]})).collect::<Vec<_>>(),
        "note":"仅描述本窗口合格实际留档；待开奖和缺档不计零分。同年期序不连续时中断未见奖连期；奖金不完整或总奖金为零时不计算集中度。短期覆盖、单期大奖与等分统计不构成未来预测优势。"})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(issue: &str, prize: Option<u64>, wins: u64) -> Value {
        json!({"issue":issue,"front_union_hits":2,"back_union_hits":1,"cost_fen":200,
        "settlement":{"winning_tickets":wins,"total_prize_fen":prize,"net_fen":prize.map(|p|p as i64-200)},"selection_evidence":{"status":"not_recorded"}})
    }
    #[test]
    fn large_prize_concentration_is_unknown_with_missing_money_or_zero_total() {
        let rows = [row("26030", Some(8000), 1), row("26031", Some(300), 1)];
        let refs: Vec<_> = rows.iter().collect();
        let r = summarize(&refs);
        assert_eq!(r["max_single_prize_share"], 8000. / 8300.);
        assert_eq!(r["max_single_prize_issues"], json!(["26030"]));
        let missing = [row("26030", Some(8000), 1), row("26031", None, 1)];
        assert!(summarize(&missing.iter().collect::<Vec<_>>())["max_single_prize_share"].is_null());
        assert!(summarize(&[&row("26030", Some(0), 0)])["max_single_prize_share"].is_null());
        assert_eq!(summarize(&[])["concentration_status"], "no_settled_periods");
    }
    #[test]
    fn missing_issue_breaks_the_losing_run_and_pending_is_not_a_zero_result() {
        let mut rows = vec![
            row("26030", Some(0), 0),
            row("26031", Some(0), 0),
            row("26033", Some(0), 0),
            row("26034", Some(0), 0),
            row("26035", Some(0), 0),
        ];
        rows.push(json!({"issue":"26036","settlement":null}));
        let r = summarize(&rows.iter().collect::<Vec<_>>());
        assert_eq!(r["longest_consecutive_nonwinning_run"], 3);
        assert_eq!(r["settled_periods"], 5);
        assert_eq!(r["pending_periods"], 1);
        assert_eq!(r["missing_evidence_periods"], 5);
    }
}
