//! Read-only comparison of a frozen plan with its own score order.
//! The audit is computed when evidence is viewed. It is not stored and cannot change tickets.
use crate::{
    evidence::SelectionEvidence,
    plan::{combinations, Mode, Plan, Strategy},
};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

const SINGLE_ENUMERATION_LIMIT: usize = 20_000;

pub fn audit(plan: &Plan, evidence: Option<&SelectionEvidence>) -> Value {
    let front = zone(plan, evidence, true);
    let back = zone(plan, evidence, false);
    let objective = objective(plan);
    let score_status = score_status(plan, evidence);
    json!({
        "schema_version": 1,
        "objective": objective,
        "score_status": score_status,
        "bankers": {"front": front.public.clone(), "back": back.public.clone()},
        "comparison": compare(plan, &front, &back, objective, score_status),
        "note": note(plan, &front, &back, objective),
    })
}

struct Zone {
    public: Value,
    numbers: Vec<u8>,
    bankers: Vec<u8>,
    weights: Option<BTreeMap<u8, u128>>,
}

fn experimental(strategy: Strategy) -> bool {
    matches!(
        strategy,
        Strategy::Hot | Strategy::Cold | Strategy::Omission
    )
}

fn zone(plan: &Plan, evidence: Option<&SelectionEvidence>, front: bool) -> Zone {
    let numbers = candidate_numbers(plan, evidence, front);
    let bankers = if front {
        plan.front_bankers.clone()
    } else {
        plan.back_bankers.clone()
    };
    let explicit = if front {
        &plan.request.front_bankers
    } else {
        &plan.request.back_bankers
    };
    let manual = if front {
        &plan.request.front_numbers
    } else {
        &plan.request.back_numbers
    };
    let basis = banker_basis(
        plan.request.strategy,
        evidence,
        front,
        explicit,
        manual,
        &bankers,
    );
    Zone {
        public: json!({"basis": basis, "numbers": bankers}),
        weights: weights(evidence, front, &numbers),
        numbers,
        bankers,
    }
}

fn candidate_numbers(plan: &Plan, evidence: Option<&SelectionEvidence>, front: bool) -> Vec<u8> {
    let exposure = if front {
        &plan.front_pool
    } else {
        &plan.back_pool
    };
    let Some(evidence) = evidence else {
        return exposure.clone();
    };
    let recorded = if front {
        &evidence.front
    } else {
        &evidence.back
    };
    let mut numbers: Vec<u8> = recorded
        .numbers
        .iter()
        .filter(|row| row.in_candidate_pool)
        .map(|row| row.number)
        .collect();
    numbers.sort_unstable();
    if numbers.is_empty() {
        exposure.clone()
    } else {
        numbers
    }
}

fn banker_basis(
    strategy: Strategy,
    evidence: Option<&SelectionEvidence>,
    front: bool,
    explicit: &[u8],
    manual: &[u8],
    bankers: &[u8],
) -> &'static str {
    if bankers.is_empty() {
        return "none";
    }
    if !explicit.is_empty() {
        return "explicit";
    }
    if !manual.is_empty() {
        let mut ordered = manual.to_vec();
        ordered.sort_unstable();
        return if bankers.len() <= ordered.len() && bankers == &ordered[..bankers.len()] {
            "numeric_prefix"
        } else {
            "not_prefix"
        };
    }
    if experimental(strategy) {
        if evidence.is_none_or(|item| score_prefix(item, front, bankers)) {
            "score_prefix"
        } else {
            "not_prefix"
        }
    } else {
        "shuffle_prefix"
    }
}

fn score_prefix(evidence: &SelectionEvidence, front: bool, bankers: &[u8]) -> bool {
    let recorded = if front {
        &evidence.front
    } else {
        &evidence.back
    };
    let mut ranked: Vec<u8> = recorded
        .numbers
        .iter()
        .filter(|row| row.in_candidate_pool)
        .map(|row| row.number)
        .take(bankers.len())
        .collect();
    ranked.sort_unstable();
    let mut actual = bankers.to_vec();
    actual.sort_unstable();
    ranked == actual
}

fn weights(
    evidence: Option<&SelectionEvidence>,
    front: bool,
    pool: &[u8],
) -> Option<BTreeMap<u8, u128>> {
    let recorded = if front {
        &evidence?.front
    } else {
        &evidence?.back
    };
    let scores: BTreeMap<u8, i64> = recorded
        .numbers
        .iter()
        .filter_map(|row| row.score.map(|score| (row.number, score)))
        .collect();
    let floor = pool
        .iter()
        .filter_map(|number| scores.get(number))
        .copied()
        .min()?;
    if !pool.iter().all(|number| scores.contains_key(number)) {
        return None;
    }
    let mut weights = BTreeMap::new();
    for number in pool {
        let shifted = scores[number].checked_sub(floor)?.checked_add(1)?;
        weights.insert(*number, u128::try_from(shifted).ok()?);
    }
    Some(weights)
}

fn score_status(plan: &Plan, evidence: Option<&SelectionEvidence>) -> &'static str {
    if !experimental(plan.request.strategy) {
        return "not_scored";
    }
    match evidence {
        Some(evidence)
            if evidence
                .front
                .numbers
                .iter()
                .chain(&evidence.back.numbers)
                .any(|row| row.score.is_some()) =>
        {
            "recorded_scores"
        }
        Some(_) => "not_scored",
        None => "not_recorded",
    }
}

fn objective(plan: &Plan) -> &'static str {
    match (plan.request.mode, plan.request.strategy) {
        (Mode::Compound, _) => "full_pool",
        (_, Strategy::Coverage | Strategy::Structural) => "exposure_balance",
        (_, Strategy::Random) => "uniform_sample",
        _ => "independent_product",
    }
}

fn compare(plan: &Plan, front: &Zone, back: &Zone, objective: &str, score_status: &str) -> Value {
    if objective == "full_pool" {
        return json!({"status":"full_pool","chosen_mass":null,"best_mass":null,"best_split_count":1,"chosen_is_best":true,"generated_ticket_count":plan.tickets.len(),"tickets_eligible_for_best_set":null});
    }
    if objective == "exposure_balance" {
        return unknown("not_applicable", plan.tickets.len());
    }
    if objective == "uniform_sample" || zones_flat(front, back) {
        return analytic(plan, front, back);
    }
    if score_status != "recorded_scores" || front.weights.is_none() || back.weights.is_none() {
        return unknown("unknown_scores", plan.tickets.len());
    }
    match plan.request.mode {
        Mode::Dantuo => match dantuo(front, back) {
            Ok((chosen, best, ties)) => compared(chosen, best, ties, plan.tickets.len(), None),
            Err(Fail::Overflow | Fail::Missing) => unknown("not_compared", plan.tickets.len()),
        },
        Mode::Single => singles(plan, front, back),
        Mode::Compound => {
            json!({"status":"full_pool","chosen_is_best":true,"generated_ticket_count":plan.tickets.len()})
        }
    }
}

fn zones_flat(front: &Zone, back: &Zone) -> bool {
    flat(&front.weights) && flat(&back.weights)
}

fn flat(weights: &Option<BTreeMap<u8, u128>>) -> bool {
    weights.as_ref().is_some_and(|weights| {
        let Some(first) = weights.values().next() else {
            return false;
        };
        weights.values().all(|weight| weight == first)
    })
}

fn analytic(plan: &Plan, front: &Zone, back: &Zone) -> Value {
    let count = (plan.request.mode == Mode::Dantuo)
        .then(|| split_count(front, back))
        .flatten();
    json!({
        "status": "analytic_tie",
        "chosen_mass": null,
        "best_mass": null,
        "best_split_count": count,
        "chosen_is_best": true,
        "generated_ticket_count": plan.tickets.len(),
        "tickets_eligible_for_best_set": null,
    })
}

fn split_count(front: &Zone, back: &Zone) -> Option<u64> {
    let front_count =
        u64::try_from(combinations(&front.numbers, front.bankers.len()).len()).ok()?;
    let back_count = u64::try_from(combinations(&back.numbers, back.bankers.len()).len()).ok()?;
    let count = front_count.checked_mul(back_count)?;
    (count > 0).then_some(count)
}

#[derive(Debug)]
enum Fail {
    Missing,
    Overflow,
}

fn dantuo(front: &Zone, back: &Zone) -> Result<(u128, u128, u64), Fail> {
    let (front_chosen, front_best, front_ties) = zone_product(front, 5)?;
    let (back_chosen, back_best, back_ties) = zone_product(back, 2)?;
    Ok((
        front_chosen
            .checked_mul(back_chosen)
            .ok_or(Fail::Overflow)?,
        front_best.checked_mul(back_best).ok_or(Fail::Overflow)?,
        front_ties.checked_mul(back_ties).ok_or(Fail::Overflow)?,
    ))
}

fn zone_product(zone: &Zone, take: usize) -> Result<(u128, u128, u64), Fail> {
    let weights = zone.weights.as_ref().ok_or(Fail::Missing)?;
    if zone.bankers.len() > take
        || zone.numbers.len() < take
        || zone.numbers.len() - zone.bankers.len() < take - zone.bankers.len()
    {
        return Err(Fail::Missing);
    }
    let drag = take - zone.bankers.len();
    let chosen_set: BTreeSet<u8> = zone.bankers.iter().copied().collect();
    if !chosen_set.iter().all(|number| weights.contains_key(number)) {
        return Err(Fail::Missing);
    }
    let mut best = None;
    let mut ties = 0u64;
    let mut chosen = None;
    for split in combinations(&zone.numbers, zone.bankers.len()) {
        let banker_weight = split
            .iter()
            .try_fold(1u128, |acc, number| acc.checked_mul(weights[number]))
            .ok_or(Fail::Overflow)?;
        let drag_weights: Vec<u128> = zone
            .numbers
            .iter()
            .filter(|number| !split.contains(number))
            .map(|number| weights[number])
            .collect();
        let mass = banker_weight
            .checked_mul(symmetric(&drag_weights, drag).ok_or(Fail::Overflow)?)
            .ok_or(Fail::Overflow)?;
        match best {
            Some(current) if mass == current => ties += 1,
            Some(current) if mass > current => {
                best = Some(mass);
                ties = 1;
            }
            None => {
                best = Some(mass);
                ties = 1;
            }
            Some(_) => {}
        }
        if split.into_iter().collect::<BTreeSet<_>>() == chosen_set {
            chosen = Some(mass);
        }
    }
    Ok((
        chosen.ok_or(Fail::Missing)?,
        best.ok_or(Fail::Missing)?,
        ties,
    ))
}

fn symmetric(weights: &[u128], k: usize) -> Option<u128> {
    if k == 0 {
        return Some(1);
    }
    if weights.len() < k {
        return Some(0);
    }
    let mut sums = vec![0u128; k + 1];
    sums[0] = 1;
    for &weight in weights {
        for index in (1..=k).rev() {
            sums[index] = sums[index].checked_add(sums[index - 1].checked_mul(weight)?)?;
        }
    }
    Some(sums[k])
}

fn singles(plan: &Plan, front: &Zone, back: &Zone) -> Value {
    let front_sets = combinations(&front.numbers, 5);
    let back_sets = combinations(&back.numbers, 2);
    if front_sets.len().saturating_mul(back_sets.len()) > SINGLE_ENUMERATION_LIMIT {
        return json!({
            "status": "skipped_pool_too_large",
            "chosen_mass": null,
            "best_mass": null,
            "best_split_count": null,
            "chosen_is_best": null,
            "generated_ticket_count": plan.tickets.len(),
            "tickets_eligible_for_best_set": null,
            "enumeration_limit": SINGLE_ENUMERATION_LIMIT,
        });
    }
    let Some(front_weights) = front.weights.as_ref() else {
        return unknown("unknown_scores", plan.tickets.len());
    };
    let Some(back_weights) = back.weights.as_ref() else {
        return unknown("unknown_scores", plan.tickets.len());
    };
    let mut masses = Vec::new();
    for front_set in &front_sets {
        for back_set in &back_sets {
            let Some(mass) = ticket_mass(front_set, front_weights, back_set, back_weights) else {
                return unknown("not_compared", plan.tickets.len());
            };
            masses.push(mass);
        }
    }
    masses.sort_unstable_by(|left, right| right.cmp(left));
    let bets = plan.request.bets.min(masses.len());
    let Some(best) = masses
        .iter()
        .take(bets)
        .try_fold(0u128, |acc, mass| acc.checked_add(*mass))
    else {
        return unknown("not_compared", plan.tickets.len());
    };
    let cutoff = masses.get(bets.saturating_sub(1)).copied().unwrap_or(0);
    let mut chosen = 0u128;
    let mut eligible = 0u64;
    for ticket in &plan.tickets {
        let Some(mass) = ticket_mass(&ticket.front, front_weights, &ticket.back, back_weights)
        else {
            return unknown("not_compared", plan.tickets.len());
        };
        chosen = match chosen.checked_add(mass) {
            Some(value) => value,
            None => return unknown("not_compared", plan.tickets.len()),
        };
        if mass >= cutoff {
            eligible += 1;
        }
    }
    let ties = masses.iter().filter(|mass| **mass == cutoff).count() as u64;
    compared(chosen, best, ties, plan.tickets.len(), Some(eligible))
}

fn ticket_mass(
    front: &[u8],
    front_weights: &BTreeMap<u8, u128>,
    back: &[u8],
    back_weights: &BTreeMap<u8, u128>,
) -> Option<u128> {
    product(front, front_weights)?.checked_mul(product(back, back_weights)?)
}

fn product(numbers: &[u8], weights: &BTreeMap<u8, u128>) -> Option<u128> {
    numbers
        .iter()
        .try_fold(1u128, |acc, number| acc.checked_mul(*weights.get(number)?))
}

fn compared(chosen: u128, best: u128, ties: u64, tickets: usize, eligible: Option<u64>) -> Value {
    json!({
        "status": "compared",
        "chosen_mass": chosen.to_string(),
        "best_mass": best.to_string(),
        "best_split_count": ties,
        "chosen_is_best": chosen == best,
        "generated_ticket_count": tickets,
        "tickets_eligible_for_best_set": eligible,
    })
}

fn unknown(status: &str, tickets: usize) -> Value {
    json!({
        "status": status,
        "chosen_mass": null,
        "best_mass": null,
        "best_split_count": null,
        "chosen_is_best": null,
        "generated_ticket_count": tickets,
        "tickets_eligible_for_best_set": null,
    })
}

fn note(plan: &Plan, front: &Zone, back: &Zone, objective: &str) -> String {
    let mut parts = vec![
        "只核对原票的组票目标，不替换、不重写这组号码。".to_string(),
        "正整数权重只保留原评分的高低顺序，不是概率。".to_string(),
    ];
    if matches!(plan.request.strategy, Strategy::Hot | Strategy::Cold) {
        parts.push("热号和冷号是同一频率的两个方向。".to_string());
    }
    parts.push(match objective {
        "full_pool" => "复式展开全部候选组合，没有另一组子集。".to_string(),
        "exposure_balance" => {
            "覆盖和结构策略平衡号码或后区对的出现次数，不按完整注的连乘质量选胆。".to_string()
        }
        "uniform_sample" => "权重相同，固定注数的每一种胆码分法质量都一样。".to_string(),
        _ => "连乘质量只说明该实验评分内部的高低，不改变均匀头奖概率。".to_string(),
    });
    for (name, item) in [("前区", front), ("后区", back)] {
        let sentence = match item.public["basis"].as_str() {
            Some("numeric_prefix") => Some(format!("{name}留空胆码取的是自选池里最小的号码。")),
            Some("shuffle_prefix") => Some(format!("{name}留空胆码取的是种子顺序的前几个号码。")),
            Some("score_prefix") => Some(format!("{name}留空胆码取的是评分较高的前几个号码。")),
            Some("explicit") => Some(format!("{name}胆码是手工指定的。")),
            _ => None,
        };
        if let Some(sentence) = sentence {
            parts.push(sentence);
        }
    }
    parts.join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::Draw,
        plan::{
            generate, generate_with_evidence, presets, GenerateRequest, Mode, Strategy,
            GENERATOR_VERSION,
        },
    };
    use chrono::{Duration, NaiveDate};
    use std::collections::BTreeMap;

    fn history() -> Vec<Draw> {
        (0..12)
            .map(|index| Draw {
                issue: format!("26{:03}", index + 1),
                date: (NaiveDate::from_ymd_opt(2026, 3, 1).unwrap() + Duration::days(index))
                    .to_string(),
                front: vec![35, 34, 33, 32, 31],
                back: vec![12, 11],
                awards: BTreeMap::new(),
                pool_before_fen: None,
                source: "synthetic".into(),
            })
            .collect()
    }

    fn request(mode: Mode, strategy: Strategy) -> GenerateRequest {
        GenerateRequest {
            issue: "26030".into(),
            mode,
            strategy,
            bets: 1,
            front_pool: 7,
            back_pool: 3,
            front_banker_count: 2,
            back_banker_count: 1,
            ..GenerateRequest::default()
        }
    }

    #[test]
    fn manual_blank_bankers_are_the_smallest_numbers_and_can_lose_the_score_product() {
        let mut input = request(Mode::Dantuo, Strategy::Hot);
        input.front_numbers = vec![1, 2, 3, 4, 5, 6, 35];
        input.back_numbers = vec![1, 2, 12];
        let (plan, evidence) = generate_with_evidence(&input, 7, &history()).unwrap();
        assert_eq!(plan.front_bankers, vec![1, 2]);
        assert!(plan
            .tickets
            .iter()
            .all(|ticket| ticket.front.starts_with(&[1, 2])));
        let id = plan.id.clone();
        let tickets = plan.tickets.clone();
        let report = audit(&plan, Some(&evidence));
        assert_eq!(plan.id, id);
        assert_eq!(plan.tickets, tickets);
        assert_eq!(plan.generator_version, GENERATOR_VERSION);
        assert_eq!(report["bankers"]["front"]["basis"], "numeric_prefix");
        assert_eq!(report["comparison"]["status"], "compared");
        assert_eq!(report["comparison"]["chosen_is_best"], false);
        assert!(report["comparison"]["chosen_mass"].is_string());
        assert!(report["comparison"]["best_mass"].is_string());
        assert!(report["note"].as_str().unwrap().contains("最小的号码"));
        assert!(!report["note"].as_str().unwrap().contains("建议号码"));
    }

    #[test]
    fn score_prefix_bankers_match_the_enumerated_product() {
        let (plan, evidence) =
            generate_with_evidence(&request(Mode::Dantuo, Strategy::Hot), 7, &history()).unwrap();
        let report = audit(&plan, Some(&evidence));
        assert_eq!(report["bankers"]["front"]["basis"], "score_prefix");
        assert_eq!(report["bankers"]["back"]["basis"], "score_prefix");
        assert_eq!(report["comparison"]["chosen_is_best"], true);
    }

    #[test]
    fn random_blank_bankers_are_a_uniform_tie() {
        let (_, input) = presets("26030")
            .into_iter()
            .find(|(name, _)| *name == "双区胆拖")
            .unwrap();
        let plan = generate(&input, 7, &[]).unwrap();
        let report = audit(&plan, None);
        assert_eq!(report["bankers"]["front"]["basis"], "shuffle_prefix");
        assert_eq!(report["comparison"]["status"], "analytic_tie");
        assert_eq!(report["comparison"]["chosen_is_best"], true);
        assert!(report["comparison"]["best_split_count"].as_u64().unwrap() > 1);
        assert!(report["comparison"]["chosen_mass"].is_null());
        assert!(report["comparison"]["best_mass"].is_null());
    }

    #[test]
    fn equal_scores_tie_and_large_single_pools_are_not_invented() {
        let (plan, mut evidence) =
            generate_with_evidence(&request(Mode::Dantuo, Strategy::Hot), 7, &history()).unwrap();
        for row in evidence
            .front
            .numbers
            .iter_mut()
            .chain(&mut evidence.back.numbers)
        {
            row.score = Some(0);
        }
        let report = audit(&plan, Some(&evidence));
        assert_eq!(report["comparison"]["status"], "analytic_tie");
        assert!(report["comparison"]["chosen_mass"].is_null());

        let mut wide = request(Mode::Single, Strategy::Hot);
        wide.front_pool = 12;
        wide.back_pool = 8;
        wide.bets = 1;
        let (plan, evidence) = generate_with_evidence(&wide, 7, &history()).unwrap();
        let report = audit(&plan, Some(&evidence));
        assert_eq!(report["comparison"]["status"], "skipped_pool_too_large");
        assert!(report["comparison"]["chosen_mass"].is_null());
        assert!(report["comparison"]["best_mass"].is_null());
    }

    #[test]
    fn missing_scores_and_coverage_do_not_receive_a_product_winner() {
        let (plan, _) =
            generate_with_evidence(&request(Mode::Dantuo, Strategy::Hot), 7, &history()).unwrap();
        let report = audit(&plan, None);
        assert_eq!(report["score_status"], "not_recorded");
        assert_eq!(report["bankers"]["front"]["basis"], "score_prefix");
        assert_eq!(report["comparison"]["status"], "unknown_scores");
        assert!(report["comparison"]["chosen_mass"].is_null());

        let (_, input) = presets("26030")
            .into_iter()
            .find(|(name, _)| *name == "均衡覆盖7注")
            .unwrap();
        let coverage = generate(&input, 7, &[]).unwrap();
        let report = audit(&coverage, None);
        assert_eq!(report["objective"], "exposure_balance");
        assert_eq!(report["comparison"]["status"], "not_applicable");
        assert!(report["comparison"]["chosen_mass"].is_null());
        assert!(report["note"]
            .as_str()
            .unwrap()
            .contains("不按完整注的连乘质量"));
    }

    #[test]
    fn symmetric_mass_matches_direct_enumeration() {
        assert_eq!(symmetric(&[3, 1, 1], 2), Some(7));
        assert_eq!(symmetric(&[u128::MAX, 2], 2), None);
        let zone = Zone {
            public: json!({}),
            numbers: vec![1, 2, 3],
            bankers: vec![1],
            weights: Some(BTreeMap::from([(1, 3u128), (2, 1), (3, 1)])),
        };
        let (chosen, best, ties) = zone_product(&zone, 2).unwrap();
        assert_eq!((chosen, best, ties), (6, 6, 1));
    }
}
