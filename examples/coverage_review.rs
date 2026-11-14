//! Read-only coverage review. Candidate pair rotation is an offline experiment,
//! not a change to the production generator or saved recommendations.
use anyhow::{ensure, Context, Result};
use lottery_dlt::{
    fetch,
    model::{choose, Draw, Ticket, TOTAL_OUTCOMES},
    plan::{generate, GenerateRequest, Mode, Plan, Strategy},
    prizes::{fixed_yuan, level, settle},
};
use rand::{seq::SliceRandom, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};

const SEED: u64 = 20261015;
fn review_seed() -> u64 {
    std::env::var("DLT_REVIEW_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(SEED)
}

#[derive(Clone)]
struct Variant {
    name: &'static str,
    request: GenerateRequest,
    rotate_pairs: bool,
}

fn variants() -> Vec<Variant> {
    let base = GenerateRequest::default();
    let mut result = Vec::new();
    for (bets, random, coverage) in [
        (5, "random_5", "coverage_5"),
        (6, "random_6", "coverage_6"),
        (7, "random_7", "coverage_7"),
        (20, "random_20", "coverage_20"),
    ] {
        for (name, strategy) in [(random, Strategy::Random), (coverage, Strategy::Coverage)] {
            result.push(Variant {
                name,
                request: GenerateRequest {
                    strategy,
                    bets,
                    ..base.clone()
                },
                rotate_pairs: false,
            });
        }
    }
    for (name, bets) in [("structural_7", 7), ("structural_20", 20)] {
        result.push(Variant {
            name,
            request: GenerateRequest {
                strategy: Strategy::Structural,
                bets,
                ..base.clone()
            },
            rotate_pairs: false,
        });
    }
    for (name, front_pool, back_pool) in [("compound_6_2", 6, 2), ("compound_5_4", 5, 4)] {
        result.push(Variant {
            name,
            request: GenerateRequest {
                mode: Mode::Compound,
                front_pool,
                back_pool,
                ..base.clone()
            },
            rotate_pairs: false,
        });
    }
    result.push(Variant {
        name: "dantuo_20",
        request: GenerateRequest {
            mode: Mode::Dantuo,
            front_pool: 7,
            back_pool: 3,
            front_banker_count: 2,
            back_banker_count: 1,
            ..base.clone()
        },
        rotate_pairs: false,
    });
    for (name, strategy) in [
        ("hot_7", Strategy::Hot),
        ("cold_7", Strategy::Cold),
        ("omission_7", Strategy::Omission),
    ] {
        result.push(Variant {
            name,
            request: GenerateRequest {
                bets: 7,
                strategy,
                ..base.clone()
            },
            rotate_pairs: false,
        });
    }
    for (name, bets) in [("pair_rotation_7", 7), ("pair_rotation_20", 20)] {
        result.push(Variant {
            name,
            request: GenerateRequest {
                bets,
                strategy: Strategy::Coverage,
                ..base.clone()
            },
            rotate_pairs: true,
        });
    }
    result
}

/// A fixed one-factorization of K12: eleven rounds of six disjoint pairs.
/// Label permutation, round order and within-round order are seeded without
/// using any target draw. Only the back pairs are changed from the base plan.
fn pair_rotation(plan: &Plan) -> Vec<Ticket> {
    let mut rng = ChaCha20Rng::seed_from_u64(
        review_seed() ^ u64::from_str_radix(&plan.id[..16], 16).unwrap(),
    );
    let mut labels: Vec<u8> = (1..=12).collect();
    labels.shuffle(&mut rng);
    let mut wheel: Vec<usize> = (0..12).collect();
    let mut rounds = Vec::new();
    for _ in 0..11 {
        let mut round = Vec::new();
        for i in 0..6 {
            let mut pair = vec![labels[wheel[i]], labels[wheel[11 - i]]];
            pair.sort_unstable();
            round.push(pair);
        }
        round.shuffle(&mut rng);
        rounds.push(round);
        let last = wheel.pop().unwrap();
        wheel.insert(1, last);
    }
    rounds.shuffle(&mut rng);
    let pairs: Vec<_> = rounds.into_iter().flatten().collect();
    plan.tickets
        .iter()
        .enumerate()
        .map(|(i, t)| Ticket {
            front: t.front.clone(),
            back: pairs[i % pairs.len()].clone(),
        })
        .collect()
}

fn ticket_group(variant: &Variant, draw: &Draw, prior: &[Draw]) -> Result<(Plan, Vec<Ticket>)> {
    let mut request = variant.request.clone();
    request.issue = draw.issue.clone();
    let plan = generate(&request, review_seed(), prior)?;
    ensure!(
        plan.data_cutoff.as_ref().is_none_or(|c| c < &draw.issue),
        "Non-causal input"
    );
    let tickets = if variant.rotate_pairs {
        pair_rotation(&plan)
    } else {
        plan.tickets.clone()
    };
    Ok((plan, tickets))
}

fn historical(history: &[Draw], variant: &Variant, start: usize) -> Result<Value> {
    let mut rows = Vec::new();
    let mut front_hits = 0;
    let mut back_hits = 0;
    let mut front_size = 0;
    let mut back_size = 0;
    let mut both_all = 0;
    let mut front_all = 0;
    let mut back_all = 0;
    let mut win_issues = 0;
    let mut priced = 0;
    let mut recovered = 0;
    let mut profitable = 0;
    let mut prize_sum = 0u64;
    let mut total_cost = 0u64;
    let mut distinct_pairs = 0;
    let mut matching_pair_issues = 0;
    for i in start..history.len() {
        let draw = &history[i];
        let (plan, tickets) = ticket_group(variant, draw, &history[i.saturating_sub(60)..i])?;
        let front: BTreeSet<_> = tickets
            .iter()
            .flat_map(|t| t.front.iter().copied())
            .collect();
        let back: BTreeSet<_> = tickets
            .iter()
            .flat_map(|t| t.back.iter().copied())
            .collect();
        let pairs: BTreeSet<_> = tickets.iter().map(|t| t.back.clone()).collect();
        let f = draw.front.iter().filter(|n| front.contains(n)).count();
        let b = draw.back.iter().filter(|n| back.contains(n)).count();
        front_hits += f;
        back_hits += b;
        front_size += front.len();
        back_size += back.len();
        front_all += usize::from(f == 5);
        back_all += usize::from(b == 2);
        both_all += usize::from(f == 5 && b == 2);
        distinct_pairs += pairs.len();
        matching_pair_issues += usize::from(pairs.contains(&draw.back));
        let result = settle(&tickets, false, 1, draw)?;
        win_issues += usize::from(result.winning_tickets > 0);
        total_cost += plan.cost_fen;
        if let Some(prize) = result.total_prize_fen {
            priced += 1;
            prize_sum += prize;
            recovered += usize::from(prize >= plan.cost_fen);
            profitable += usize::from(prize > plan.cost_fen);
        }
        rows.push(json!({"issue":draw.issue,"data_cutoff":plan.data_cutoff,"base_plan_id":plan.id,
            "experimental_pair_rotation":variant.rotate_pairs,"front_union_hits":f,"back_union_hits":b,
            "winning_tickets":result.winning_tickets,"prize_fen":result.total_prize_fen,"cost_fen":plan.cost_fen}));
    }
    let n = rows.len() as f64;
    Ok(
        json!({"name":variant.name,"periods":rows.len(),"first_issue":history[start].issue,"last_issue":history.last().unwrap().issue,
        "cost_per_issue_yuan":total_cost as f64/n/100.,"mean_front_pool":front_size as f64/n,"mean_back_pool":back_size as f64/n,
        "mean_front_union_hits":front_hits as f64/n,"mean_back_union_hits":back_hits as f64/n,
        "pooled_draw_number_coverage":(front_hits+back_hits) as f64/(7.*n),"front_all_rate":front_all as f64/n,
        "back_all_rate":back_all as f64/n,"both_all_rate":both_all as f64/n,"any_win_rate":win_issues as f64/n,
        "win_issues":win_issues,"priced_issues":priced,"unpriced_issues":rows.len()-priced,
        "recovery_rate_priced_subset":if priced>0 {Some(recovered as f64/priced as f64)} else {None},
        "profit_rate_priced_subset":if priced>0 {Some(profitable as f64/priced as f64)} else {None},
        "cost_fen":total_cost,"total_prize_fen":if priced==rows.len() {Some(prize_sum)} else {None},
        "net_fen":if priced==rows.len() {Some(prize_sum as i64-total_cost as i64)} else {None},
        "mean_distinct_back_pairs":distinct_pairs as f64/n,"actual_back_pair_hit_rate":matching_pair_issues as f64/n,"trace":rows}),
    )
}

fn mask(values: &[u8]) -> u64 {
    values.iter().fold(0, |m, n| m | (1u64 << (n - 1)))
}
fn wilson(success: usize, trials: usize) -> [f64; 2] {
    let p = success as f64 / trials as f64;
    let z2 = 1.96 * 1.96;
    let denom = 1. + z2 / trials as f64;
    let center = (p + z2 / (2. * trials as f64)) / denom;
    let half = 1.96 * ((p * (1. - p) + z2 / (4. * trials as f64)) / trials as f64).sqrt() / denom;
    [center - half, center + half]
}

fn uniform_simulation(history: &[Draw], variants: &[Variant], trials: usize) -> Result<Vec<Value>> {
    let target = Draw {
        issue: format!("{:05}", history.last().unwrap().issue.parse::<u32>()? + 1),
        ..history.last().unwrap().clone()
    };
    let groups = variants
        .iter()
        .map(|v| ticket_group(v, &target, &history[history.len().saturating_sub(60)..]))
        .collect::<Result<Vec<_>>>()?;
    let masks: Vec<_> = groups
        .iter()
        .map(|(_, ts)| {
            ts.iter()
                .map(|t| (mask(&t.front), mask(&t.back)))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut wins = vec![0usize; groups.len()];
    let mut recovered_low = vec![0usize; groups.len()];
    let mut recovered_high = vec![0usize; groups.len()];
    let mut paired_counts = vec![[0usize; 2]; variants.len()];
    let baseline = variants
        .iter()
        .position(|v| v.name == "coverage_7")
        .unwrap();
    let mut current_wins = vec![false; variants.len()];
    let mut rng = ChaCha20Rng::seed_from_u64(0x44544c5f434f5631);
    let fronts: Vec<u8> = (1..=35).collect();
    let backs: Vec<u8> = (1..=12).collect();
    for _ in 0..trials {
        let fm = fronts
            .choose_multiple(&mut rng, 5)
            .fold(0u64, |m, n| m | (1u64 << (n - 1)));
        let bm = backs
            .choose_multiple(&mut rng, 2)
            .fold(0u64, |m, n| m | (1u64 << (n - 1)));
        for (i, group) in masks.iter().enumerate() {
            let mut any = false;
            let mut low = 0;
            let mut high = 0;
            for (f, b) in group {
                if let Some(grade) = level(
                    (f & fm).count_ones() as u8,
                    (b & bm).count_ones() as u8,
                    true,
                ) {
                    any = true;
                    low += fixed_yuan(grade, true, false).unwrap_or(0);
                    high += fixed_yuan(grade, true, true).unwrap_or(0);
                }
            }
            current_wins[i] = any;
            wins[i] += usize::from(any);
            let cost = groups[i].0.cost_fen / 100;
            recovered_low[i] += usize::from(low >= cost);
            recovered_high[i] += usize::from(high >= cost);
        }
        for i in 0..variants.len() {
            paired_counts[i][0] += usize::from(current_wins[i] && !current_wins[baseline]);
            paired_counts[i][1] += usize::from(!current_wins[i] && current_wins[baseline]);
        }
    }
    Ok(groups.iter().zip(variants).enumerate().map(|(i,((plan,ts),variant))| {
        let fp=ts.iter().flat_map(|t|t.front.iter().copied()).collect::<BTreeSet<_>>().len();
        let bp=ts.iter().flat_map(|t|t.back.iter().copied()).collect::<BTreeSet<_>>().len();
        let pairs=ts.iter().map(|t|t.back.clone()).collect::<BTreeSet<_>>().len();
        let delta=(paired_counts[i][0] as f64-paired_counts[i][1] as f64)/trials as f64;
        let se=(((paired_counts[i][0]+paired_counts[i][1]) as f64/trials as f64-delta*delta)/trials as f64).max(0.).sqrt();
        json!({"paired_vs_coverage_7":{"win_rate_delta":delta,"monte_carlo_ci95":[delta-1.96*se,delta+1.96*se]},"name":variant.name,"trials":trials,"cost_yuan":plan.cost_fen as f64/100.,"front_pool":fp,"back_pool":bp,"distinct_back_pairs":pairs,
            "expected_front_union_hits":5.*fp as f64/35.,"expected_back_union_hits":2.*bp as f64/12.,
            "both_all_probability_exact":choose(fp,5) as f64/choose(35,5) as f64*choose(bp,2) as f64/choose(12,2) as f64,
            "first_prize_probability_exact":ts.len() as f64/TOTAL_OUTCOMES as f64,
            "any_win_rate_mc":wins[i] as f64/trials as f64,"any_win_interval_95":wilson(wins[i],trials),
            "recovery_low_fixed_only_lower_bound_mc":recovered_low[i] as f64/trials as f64,
            "recovery_high_fixed_only_lower_bound_mc":recovered_high[i] as f64/trials as f64,
            "base_plan_id":plan.id,"tickets":ts})
    }).collect())
}

fn main() -> Result<()> {
    let path = std::env::args()
        .nth(1)
        .context("usage: coverage_review HISTORY_JSON")?;
    let history = fetch::load(Path::new(&path))?;
    ensure!(history.len() > 360, "Need more history");
    let variants = variants();
    let recent_start = history.len() - 300;
    let modern_start = history
        .iter()
        .position(|d| d.issue.as_str() >= "26014")
        .context("Need current rules draws")?;
    let recent = variants
        .iter()
        .map(|v| historical(&history, v, recent_start))
        .collect::<Result<Vec<_>>>()?;
    let modern = variants
        .iter()
        .map(|v| historical(&history, v, modern_start))
        .collect::<Result<Vec<_>>>()?;
    let simulated = uniform_simulation(&history, &variants, 500_000)?;
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({"data_hash":lottery_dlt::digest(&history)?,"history_rows":history.len(),
        "generator_version":lottery_dlt::plan::GENERATOR_VERSION,"seed":review_seed().to_string(),"window":60,"recent_300":recent,"modern_rules":modern,"uniform_current_plan_mc":simulated,
        "note":"Read-only exploratory review, no real pre-draw records. Pair rotation is an offline structural candidate. Shared uniform draws evaluate fixed current ticket groups; MC intervals are simulation uncertainty only. Historical outcomes are not proof of predictive advantage. Recovery simulations exclude floating awards, promotions and tax."})
        )?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experimental_rotation_covers_all_66_pairs_and_keeps_number_exposure_balanced() {
        for bets in [7, 20, 66] {
            let request = GenerateRequest {
                issue: "26118".into(),
                strategy: Strategy::Coverage,
                bets,
                ..Default::default()
            };
            let plan = generate(&request, SEED, &[]).unwrap();
            let tickets = pair_rotation(&plan);
            assert_eq!(tickets.len(), bets);
            assert_eq!(
                tickets
                    .iter()
                    .map(|t| &t.back)
                    .collect::<BTreeSet<_>>()
                    .len(),
                bets
            );
            assert!(tickets
                .iter()
                .zip(&plan.tickets)
                .all(|(new, old)| new.front == old.front));
            let mut counts = [0usize; 12];
            for t in &tickets {
                for n in &t.back {
                    counts[*n as usize - 1] += 1;
                }
            }
            assert!(counts.iter().max().unwrap() - counts.iter().min().unwrap() <= 1);
        }
    }
}
