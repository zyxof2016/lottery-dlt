//! Pre-draw rankings and the frozen number pools for one issue.
//! Scores only order numbers. They are not calibrated win probabilities.
use crate::model::Draw;
use anyhow::{ensure, Result};
use rand::{seq::SliceRandom, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const ALGORITHMS: &[(&str, &str)] = &[
    ("hot", "热号"),
    ("cold", "冷号"),
    ("omission", "遗漏"),
    ("follow", "跟随上期"),
    ("avoid", "避开上期"),
    ("band", "分区轮动"),
    ("uniform", "均匀随机对照"),
];

struct Zone {
    name: &'static str,
    upper: u8,
    drawn: usize,
    targets: &'static [usize],
}

const FRONT: Zone = Zone {
    name: "front",
    upper: 35,
    drawn: 5,
    targets: &[3, 4, 5],
};
const BACK: Zone = Zone {
    name: "back",
    upper: 12,
    drawn: 2,
    targets: &[1, 2],
};

pub struct FrozenPool {
    pub id: String,
    pub issue: String,
    pub label: String,
    pub kind: String,
    pub mode: String,
    pub strategy: String,
    pub bets: usize,
    pub cost_fen: u64,
    pub front: Vec<u8>,
    pub back: Vec<u8>,
}

pub fn report(
    draws: &[Draw],
    requested: Option<&str>,
    pending: Option<&str>,
    seed: u64,
    pools: &[FrozenPool],
) -> Result<Value> {
    let issue = choose_issue(draws, pending, requested)?;
    let settled = draws.iter().find(|draw| draw.issue == issue);
    let prior: Vec<&Draw> = draws.iter().filter(|draw| draw.issue < issue).collect();
    let history = history(draws, seed);
    let zones = json!({
        "front": zone_report(&FRONT, &prior, settled.map(|d| d.front.as_slice()), seed, &issue, &history.front),
        "back": zone_report(&BACK, &prior, settled.map(|d| d.back.as_slice()), seed, &issue, &history.back),
    });
    let pool_rows: Vec<Value> = pools
        .iter()
        .filter(|pool| pool.issue == issue)
        .map(|pool| pool_row(pool, settled))
        .collect();
    let mut issues: Vec<Value> = Vec::new();
    if let Some(next) = pending.filter(|item| draws.iter().all(|draw| draw.issue != *item)) {
        issues.push(json!({"issue": next, "settled": false}));
    }
    for draw in draws.iter().rev() {
        issues.push(json!({"issue": draw.issue, "settled": true}));
    }
    Ok(json!({
        "issue": issue,
        "status": if settled.is_some() { "settled" } else { "pending" },
        "data_cutoff": prior.last().map(|draw| &draw.issue),
        "draw": settled.map(|draw| json!({"front": draw.front, "back": draw.back})),
        "zones": zones,
        "pools": pool_rows,
        "issues": issues,
        "note": "排序只用目标期之前的开奖。热号和冷号是同一出现次数的两个方向。名次不是中奖概率，也不替换已经冻结的号码池。",
    }))
}

fn pool_row(pool: &FrozenPool, draw: Option<&Draw>) -> Value {
    let covered =
        |numbers: &[u8], winners: &[u8]| numbers.iter().filter(|n| winners.contains(n)).count();
    json!({
        "id": pool.id,
        "label": pool.label,
        "kind": pool.kind,
        "mode": pool.mode,
        "strategy": pool.strategy,
        "bets": pool.bets,
        "cost_fen": pool.cost_fen,
        "front": pool.front,
        "back": pool.back,
        "front_covered": draw.map(|item| covered(&pool.front, &item.front)),
        "back_covered": draw.map(|item| covered(&pool.back, &item.back)),
    })
}

fn choose_issue(draws: &[Draw], pending: Option<&str>, requested: Option<&str>) -> Result<String> {
    if let Some(issue) = requested.filter(|item| !item.is_empty()) {
        crate::model::issue(issue)?;
        let known = draws.iter().any(|draw| draw.issue == issue) || pending == Some(issue);
        ensure!(known, "没有这一期的开奖或待开奖日程");
        return Ok(issue.to_string());
    }
    if let Some(issue) = pending {
        return Ok(issue.to_string());
    }
    draws
        .last()
        .map(|draw| draw.issue.clone())
        .ok_or_else(|| anyhow::anyhow!("还没有开奖，无法排序"))
}

struct History {
    front: Vec<Option<f64>>,
    back: Vec<Option<f64>>,
}

fn history(draws: &[Draw], seed: u64) -> History {
    let mut front_run = Running::new(FRONT.upper as usize);
    let mut back_run = Running::new(BACK.upper as usize);
    let mut front_sum = vec![0.0; ALGORITHMS.len()];
    let mut back_sum = vec![0.0; ALGORITHMS.len()];
    let mut front_count = vec![0.0; ALGORITHMS.len()];
    let mut back_count = vec![0.0; ALGORITHMS.len()];
    for (index, draw) in draws.iter().enumerate() {
        if index > 0 {
            let front_signals = front_run.signals(true);
            let back_signals = back_run.signals(false);
            for (algo_index, (name, _)) in ALGORITHMS.iter().enumerate() {
                if let Some(need) = prefix_need(
                    &order(&front_signals, seed, &draw.issue, &FRONT, name),
                    &draw.front,
                    FRONT.drawn,
                ) {
                    front_sum[algo_index] += need as f64;
                    front_count[algo_index] += 1.0;
                }
                if let Some(need) = prefix_need(
                    &order(&back_signals, seed, &draw.issue, &BACK, name),
                    &draw.back,
                    BACK.drawn,
                ) {
                    back_sum[algo_index] += need as f64;
                    back_count[algo_index] += 1.0;
                }
            }
        }
        front_run.observe(&draw.front);
        back_run.observe(&draw.back);
    }
    History {
        front: means(&front_sum, &front_count),
        back: means(&back_sum, &back_count),
    }
}

fn means(sum: &[f64], count: &[f64]) -> Vec<Option<f64>> {
    sum.iter()
        .zip(count)
        .map(|(total, seen)| (seen > &0.0).then_some(total / seen))
        .collect()
}

fn zone_report(
    zone: &Zone,
    prior: &[&Draw],
    drawn: Option<&[u8]>,
    seed: u64,
    issue: &str,
    means: &[Option<f64>],
) -> Value {
    let mut running = Running::new(zone.upper as usize);
    let front = zone.name == "front";
    for draw in prior {
        running.observe(if front { &draw.front } else { &draw.back });
    }
    let signals = running.signals(front);
    let algorithms: Vec<Value> = ALGORITHMS
        .iter()
        .enumerate()
        .map(|(index, (name, label))| {
            let ranking = order(&signals, seed, issue, zone, name);
            let need = drawn.map(|numbers| {
                zone.targets
                    .iter()
                    .map(|target| json!([target, prefix_need(&ranking, numbers, *target)]))
                    .collect::<Vec<_>>()
            });
            json!({
                "name": name,
                "label": label,
                "ranking": ranking,
                "need": need,
                "top_hits": drawn.map(|numbers| ranking.iter().take(zone.drawn).filter(|n| numbers.contains(n)).count()),
                "history_mean_full": means[index],
            })
        })
        .collect();
    json!({
        "size": zone.upper,
        "drawn": zone.drawn,
        "targets": zone.targets,
        "uniform_expected_prefix": zone.targets.iter().map(|target| json!([target, *target as f64 * (zone.upper as f64 + 1.0) / (zone.drawn as f64 + 1.0)])).collect::<Vec<_>>(),
        "uniform_top_hits": zone.drawn as f64 * zone.drawn as f64 / zone.upper as f64,
        "algorithms": algorithms,
    })
}

struct Signals {
    frequency: Vec<i64>,
    omission: Vec<i64>,
    previous: BTreeSet<u8>,
    bands: Vec<i64>,
}

struct Running {
    frequency: Vec<i64>,
    seen: Vec<Option<usize>>,
    last: Vec<u8>,
    len: usize,
}

impl Running {
    fn new(upper: usize) -> Self {
        Self {
            frequency: vec![0; upper],
            seen: vec![None; upper],
            last: Vec::new(),
            len: 0,
        }
    }

    fn signals(&self, front: bool) -> Signals {
        let end = self.len as i64;
        let omission = self
            .seen
            .iter()
            .map(|item| match item {
                Some(index) => end - 1 - *index as i64,
                None => end,
            })
            .collect();
        Signals {
            frequency: self.frequency.clone(),
            omission,
            previous: self.last.iter().copied().collect(),
            bands: band_frequency(&self.frequency, front),
        }
    }

    fn observe(&mut self, numbers: &[u8]) {
        for &number in numbers {
            let index = number as usize - 1;
            self.frequency[index] += 1;
            self.seen[index] = Some(self.len);
        }
        self.last = numbers.to_vec();
        self.len += 1;
    }
}

fn order(signals: &Signals, seed: u64, issue: &str, zone: &Zone, algorithm: &str) -> Vec<u8> {
    let mut numbers: Vec<u8> = (1..=zone.upper).collect();
    let mut rng = ChaCha20Rng::from_seed(seed_bytes(seed, issue, zone.name, algorithm));
    numbers.shuffle(&mut rng);
    let front = zone.name == "front";
    numbers.sort_by_key(|&number| {
        let index = number as usize - 1;
        match algorithm {
            "hot" => -signals.frequency[index],
            "cold" => signals.frequency[index],
            "omission" => -signals.omission[index],
            "follow" => i64::from(!signals.previous.contains(&number)),
            "avoid" => i64::from(signals.previous.contains(&number)),
            "band" => signals.bands[band_index(number, front)],
            _ => 0,
        }
    });
    numbers
}

fn band_index(number: u8, front: bool) -> usize {
    if front {
        if number <= 12 {
            0
        } else if number <= 24 {
            1
        } else {
            2
        }
    } else if number <= 6 {
        0
    } else {
        1
    }
}

fn band_frequency(frequency: &[i64], front: bool) -> Vec<i64> {
    let mut totals = vec![0; if front { 3 } else { 2 }];
    for (index, count) in frequency.iter().enumerate() {
        totals[band_index(index as u8 + 1, front)] += count;
    }
    totals
}

fn prefix_need(ranking: &[u8], drawn: &[u8], count: usize) -> Option<usize> {
    if drawn.len() < count {
        return None;
    }
    let mut ranks: Vec<usize> = drawn
        .iter()
        .map(|number| {
            ranking
                .iter()
                .position(|item| item == number)
                .map(|index| index + 1)
        })
        .collect::<Option<_>>()?;
    ranks.sort_unstable();
    ranks.get(count - 1).copied()
}

fn seed_bytes(seed: u64, issue: &str, zone: &str, algorithm: &str) -> [u8; 32] {
    Sha256::digest(format!("{seed}:{issue}:{zone}:{algorithm}").as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn draw(issue: &str, front: [u8; 5], back: [u8; 2]) -> Draw {
        Draw {
            issue: issue.into(),
            date: "2026-03-01".into(),
            front: front.to_vec(),
            back: back.to_vec(),
            awards: BTreeMap::new(),
            pool_before_fen: None,
            source: "synthetic".into(),
        }
    }

    #[test]
    fn hot_rank_uses_only_earlier_draws_and_names_the_repeated_number_first() {
        let draws = vec![
            draw("26001", [1, 2, 3, 4, 35], [1, 12]),
            draw("26002", [5, 6, 7, 8, 35], [2, 12]),
            draw("26003", [9, 10, 11, 12, 13], [3, 4]),
        ];
        let report = report(&draws, Some("26003"), None, 7, &[]).unwrap();
        let hot = &report["zones"]["front"]["algorithms"][0];
        assert_eq!(hot["name"], "hot");
        assert_eq!(hot["ranking"][0], 35);
        assert_eq!(report["data_cutoff"], "26002");
        assert!(hot["need"][2][1].as_u64().unwrap() >= 1);
        assert_ne!(report["zones"]["front"]["algorithms"][1]["ranking"][0], 35);
    }

    #[test]
    fn pending_issue_has_no_cover_count_and_frozen_pool_keeps_its_numbers() {
        let draws = vec![draw("26001", [1, 2, 3, 4, 5], [1, 2])];
        let pools = [
            FrozenPool {
                id: "older".into(),
                issue: "26001".into(),
                label: "上一期".into(),
                kind: "recommendation".into(),
                mode: "single".into(),
                strategy: "random".into(),
                bets: 1,
                cost_fen: 200,
                front: vec![9],
                back: vec![9],
            },
            FrozenPool {
                id: "one".into(),
                issue: "26002".into(),
                label: "双区胆拖".into(),
                kind: "recommendation".into(),
                mode: "dantuo".into(),
                strategy: "random".into(),
                bets: 20,
                cost_fen: 4000,
                front: vec![1, 2, 3],
                back: vec![1, 12],
            },
        ];
        let report = report(&draws, Some("26002"), Some("26002"), 7, &pools).unwrap();
        assert_eq!(report["status"], "pending");
        assert!(report["zones"]["front"]["algorithms"][0]["top_hits"].is_null());
        assert!(report["zones"]["front"]["algorithms"][0]["need"].is_null());
        assert!(report["zones"]["front"]["algorithms"][0]["history_mean_full"].is_null());
        assert_eq!(report["pools"].as_array().unwrap().len(), 1);
        assert_eq!(report["pools"][0]["front"], json!([1, 2, 3]));
        assert!(report["pools"][0]["front_covered"].is_null());
    }

    #[test]
    fn uniform_expectation_matches_the_order_statistic() {
        let draws = vec![
            draw("26001", [1, 2, 3, 4, 5], [1, 2]),
            draw("26002", [6, 7, 8, 9, 10], [3, 4]),
        ];
        let report = report(&draws, Some("26002"), None, 3, &[]).unwrap();
        assert_eq!(
            report["zones"]["front"]["uniform_expected_prefix"][2][1],
            30.0
        );
        assert_eq!(
            report["zones"]["back"]["uniform_expected_prefix"][1][1],
            2.0 * 13.0 / 3.0
        );
        assert!(report["zones"]["front"]["algorithms"][0]["history_mean_full"].is_number());
    }

    #[test]
    fn prefix_need_is_the_position_of_the_last_required_winner() {
        let ranking: Vec<u8> = (1..=35).collect();
        assert_eq!(prefix_need(&ranking, &[3, 1, 4], 3), Some(4));
        assert_eq!(prefix_need(&ranking, &[3], 2), None);
    }
}
