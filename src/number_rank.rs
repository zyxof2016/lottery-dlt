//! Pre-draw rankings and the frozen number pools for one issue.
//! Scores only order numbers. They are not calibrated win probabilities.
use crate::model::Draw;
use anyhow::{ensure, Result};
use rand::{seq::SliceRandom, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

const BASE: &[&str] = &["hot", "cold", "omission", "follow", "avoid", "band"];
const ALGORITHMS: &[(&str, &str)] = &[
    ("hot", "热号"),
    ("cold", "冷号"),
    ("omission", "遗漏"),
    ("follow", "跟随上期"),
    ("avoid", "避开上期"),
    ("band", "分区轮动"),
    ("cover_bottleneck", "覆盖瓶颈"),
    ("cover_partner", "同出补齐"),
    ("cover_blend", "全覆盖融合"),
    ("cover_recent", "近期全覆盖"),
    ("uniform", "均匀随机对照"),
];
const SIGNAL_CAP: i64 = 1_000;
const COVER_BUMP: i64 = 1_000;

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
        "note": "排序只用目标期之前的开奖。原有排序仍按各自方向，但会把曾经拖长全覆盖前缀的开奖号提前。覆盖瓶颈、同出补齐、全覆盖融合和近期全覆盖直接追求更短的号码池。均匀随机对照不参与这项调整。名次不是中奖概率，也不替换已经冻结的号码池。",
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
    let refs: Vec<&Draw> = draws.iter().collect();
    History {
        front: replay(&refs, seed, &FRONT).means,
        back: replay(&refs, seed, &BACK).means,
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
    let mut fitted = replay(prior, seed, zone);
    fitted.memory.decay();
    let algorithms: Vec<Value> = ALGORITHMS
        .iter()
        .enumerate()
        .map(|(index, (name, label))| {
            let ranking = rank_one(&fitted.signals, &fitted.memory, seed, issue, zone, name);
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

struct Scale {
    max_frequency: i64,
    max_omission: i64,
    max_band: i64,
}

struct Memory {
    pull: Vec<Vec<i64>>,
    bottleneck: Vec<i64>,
    partner: Vec<i64>,
    recent: Vec<i64>,
    blend: Vec<i64>,
}

impl Memory {
    fn new(upper: usize) -> Self {
        Self {
            pull: (0..BASE.len()).map(|_| vec![0; upper]).collect(),
            bottleneck: vec![0; upper],
            partner: vec![0; upper],
            recent: vec![0; upper],
            blend: vec![SIGNAL_CAP; BASE.len()],
        }
    }

    fn decay(&mut self) {
        let soften = |values: &mut [i64]| {
            for value in values {
                *value = *value * 9 / 10;
            }
        };
        for pull in &mut self.pull {
            soften(pull);
        }
        soften(&mut self.bottleneck);
        soften(&mut self.partner);
        soften(&mut self.blend);
        for value in &mut self.recent {
            *value /= 2;
        }
    }

    fn learn(&mut self, rankings: &[Vec<u8>], winners: &[u8], zone: &Zone) {
        for (index, ranking) in rankings.iter().take(BASE.len()).enumerate() {
            if let Some(worst) = worst_winner(ranking, winners) {
                let at = worst as usize - 1;
                self.pull[index][at] += COVER_BUMP;
                self.bottleneck[at] += COVER_BUMP;
                self.recent[at] += COVER_BUMP;
            }
            if let Some(prefix) = prefix_need(ranking, winners, zone.drawn) {
                self.blend[index] += zone.upper as i64 * SIGNAL_CAP / prefix as i64;
            }
        }
        let Some(ranking) = rankings.first() else {
            return;
        };
        let Some(worst) = worst_winner(ranking, winners) else {
            return;
        };
        self.partner[worst as usize - 1] += COVER_BUMP;
        for number in winners {
            if *number != worst {
                self.partner[*number as usize - 1] += COVER_BUMP / 4;
            }
        }
    }
}

struct Fitted {
    memory: Memory,
    signals: Signals,
    means: Vec<Option<f64>>,
}

fn replay(draws: &[&Draw], seed: u64, zone: &Zone) -> Fitted {
    let mut running = Running::new(zone.upper as usize);
    let mut memory = Memory::new(zone.upper as usize);
    let mut sums = vec![0.0; ALGORITHMS.len()];
    let mut counts = vec![0.0; ALGORITHMS.len()];
    let front = zone.name == "front";
    for (index, draw) in draws.iter().enumerate() {
        let winners = if front { &draw.front } else { &draw.back };
        if index > 0 {
            memory.decay();
            let signals = running.signals(front);
            let rankings = rank_all(&signals, &memory, seed, &draw.issue, zone);
            for (algo_index, ranking) in rankings.iter().enumerate() {
                if let Some(need) = prefix_need(ranking, winners, zone.drawn) {
                    sums[algo_index] += need as f64;
                    counts[algo_index] += 1.0;
                }
            }
            memory.learn(&rankings, winners, zone);
        }
        running.observe(winners);
    }
    Fitted {
        memory,
        signals: running.signals(front),
        means: means(&sums, &counts),
    }
}

fn rank_all(
    signals: &Signals,
    memory: &Memory,
    seed: u64,
    issue: &str,
    zone: &Zone,
) -> Vec<Vec<u8>> {
    ALGORITHMS
        .iter()
        .map(|(name, _)| rank_one(signals, memory, seed, issue, zone, name))
        .collect()
}

fn rank_one(
    signals: &Signals,
    memory: &Memory,
    seed: u64,
    issue: &str,
    zone: &Zone,
    algorithm: &str,
) -> Vec<u8> {
    let scale = Scale {
        max_frequency: signals.frequency.iter().copied().max().unwrap_or(0),
        max_omission: signals.omission.iter().copied().max().unwrap_or(0),
        max_band: signals.bands.iter().copied().max().unwrap_or(0),
    };
    let mut numbers: Vec<u8> = (1..=zone.upper).collect();
    let mut rng = ChaCha20Rng::from_seed(seed_bytes(seed, issue, zone.name, algorithm));
    numbers.shuffle(&mut rng);
    numbers.sort_by(|&left, &right| {
        score(signals, memory, &scale, zone, algorithm, right)
            .cmp(&score(signals, memory, &scale, zone, algorithm, left))
    });
    numbers
}

fn score(
    signals: &Signals,
    memory: &Memory,
    scale: &Scale,
    zone: &Zone,
    algorithm: &str,
    number: u8,
) -> i64 {
    let index = number as usize - 1;
    let directed = signal_score(signals, scale, zone, algorithm, number);
    match algorithm {
        "cover_bottleneck" => memory.bottleneck[index],
        "cover_partner" => memory.partner[index],
        "cover_recent" => memory.recent[index],
        "cover_blend" => BASE
            .iter()
            .enumerate()
            .map(|(slot, name)| {
                memory.blend[slot]
                    * (signal_score(signals, scale, zone, name, number) + memory.pull[slot][index])
                    / SIGNAL_CAP
            })
            .sum(),
        "uniform" => 0,
        _ => {
            let slot = BASE.iter().position(|name| *name == algorithm).unwrap_or(0);
            directed + memory.pull[slot][index]
        }
    }
}

fn signal_score(signals: &Signals, scale: &Scale, zone: &Zone, algorithm: &str, number: u8) -> i64 {
    let index = number as usize - 1;
    let frequency = signals.frequency[index];
    let front = zone.name == "front";
    match algorithm {
        "hot" => scaled(frequency, scale.max_frequency),
        "cold" => scaled(scale.max_frequency - frequency, scale.max_frequency),
        "omission" => scaled(signals.omission[index], scale.max_omission),
        "follow" => i64::from(signals.previous.contains(&number)) * SIGNAL_CAP,
        "avoid" => i64::from(!signals.previous.contains(&number)) * SIGNAL_CAP,
        "band" => {
            let total = signals.bands[band_index(number, front)];
            scaled(scale.max_band - total, scale.max_band)
        }
        _ => 0,
    }
}

fn scaled(value: i64, max: i64) -> i64 {
    if max <= 0 {
        0
    } else {
        value * SIGNAL_CAP / max
    }
}

fn worst_winner(ranking: &[u8], winners: &[u8]) -> Option<u8> {
    winners.iter().copied().max_by_key(|number| {
        ranking
            .iter()
            .position(|item| item == number)
            .unwrap_or(usize::MAX)
    })
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

    fn algorithm<'a>(report: &'a Value, name: &str) -> &'a Value {
        report["zones"]["front"]["algorithms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["name"] == name)
            .unwrap()
    }

    fn position(report: &Value, name: &str, number: u8) -> usize {
        algorithm(report, name)["ranking"]
            .as_array()
            .unwrap()
            .iter()
            .position(|item| item.as_u64() == Some(u64::from(number)))
            .unwrap()
    }

    #[test]
    fn hot_without_cover_feedback_keeps_the_seen_numbers_ahead_of_the_target() {
        let draws = vec![
            draw("26001", [1, 2, 3, 4, 35], [1, 12]),
            draw("26002", [9, 10, 11, 12, 13], [3, 4]),
        ];
        let report = report(&draws, Some("26002"), None, 7, &[]).unwrap();
        let hot = algorithm(&report, "hot")["ranking"].as_array().unwrap();
        let seen: std::collections::BTreeSet<u64> = [1, 2, 3, 4, 35].into_iter().collect();
        let top: std::collections::BTreeSet<u64> = hot
            .iter()
            .take(5)
            .map(|item| item.as_u64().unwrap())
            .collect();
        assert_eq!(top, seen);
        assert_eq!(report["data_cutoff"], "26001");
        assert_ne!(algorithm(&report, "cold")["ranking"][0], 35);
    }

    #[test]
    fn cover_adjustment_pulls_forward_the_number_that_lengthened_the_pool() {
        let draws = vec![
            draw("26001", [1, 2, 3, 4, 5], [1, 2]),
            draw("26002", [1, 2, 3, 4, 35], [1, 12]),
            draw("26003", [6, 7, 8, 9, 10], [3, 4]),
        ];
        let report = report(&draws, Some("26003"), None, 7, &[]).unwrap();
        assert_eq!(report["data_cutoff"], "26002");
        assert_eq!(algorithm(&report, "hot")["ranking"][0], 35);
        assert_eq!(algorithm(&report, "cover_partner")["ranking"][0], 35);
        assert!(
            position(&report, "cover_bottleneck", 35) < position(&report, "cover_bottleneck", 6)
        );
        assert!(position(&report, "cover_blend", 35) < position(&report, "cover_blend", 6));
        assert!(position(&report, "cover_recent", 35) < position(&report, "cover_recent", 6));
        assert_ne!(algorithm(&report, "cold")["ranking"][0], 35);
        assert_eq!(algorithm(&report, "uniform")["name"], "uniform");
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
