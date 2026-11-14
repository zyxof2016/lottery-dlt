use crate::{
    digest,
    model::{choose, issue, numbers, validate_history, Draw, Ticket, MAX_TICKETS, TOTAL_OUTCOMES},
};
use anyhow::{ensure, Result};
use rand::{seq::SliceRandom, SeedableRng};
use rand_chacha::ChaCha20Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const GENERATOR_VERSION: &str = "dlt-plan-v3";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    #[default]
    Single,
    Compound,
    Dantuo,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    #[default]
    Random,
    Coverage,
    Structural,
    Hot,
    Cold,
    Omission,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct GenerateRequest {
    pub issue: String,
    pub mode: Mode,
    pub strategy: Strategy,
    pub bets: usize,
    pub front_pool: usize,
    pub back_pool: usize,
    pub front_banker_count: usize,
    pub back_banker_count: usize,
    pub front_numbers: Vec<u8>,
    pub back_numbers: Vec<u8>,
    pub front_bankers: Vec<u8>,
    pub back_bankers: Vec<u8>,
    pub additional: bool,
    pub multiplier: u8,
    pub window: usize,
}
impl Default for GenerateRequest {
    fn default() -> Self {
        Self {
            issue: String::new(),
            mode: Mode::Single,
            strategy: Strategy::Random,
            bets: 5,
            front_pool: 7,
            back_pool: 3,
            front_banker_count: 1,
            back_banker_count: 0,
            front_numbers: vec![],
            back_numbers: vec![],
            front_bankers: vec![],
            back_bankers: vec![],
            additional: false,
            multiplier: 1,
            window: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub generator_version: String,
    pub request: GenerateRequest,
    pub data_cutoff: Option<String>,
    pub history_hash: String,
    pub seed_fingerprint: String,
    pub tickets: Vec<Ticket>,
    pub front_pool: Vec<u8>,
    pub back_pool: Vec<u8>,
    pub front_bankers: Vec<u8>,
    pub back_bankers: Vec<u8>,
    pub cost_fen: u64,
    pub first_prize_probability: f64,
    pub front_exposure: Vec<usize>,
    pub back_exposure: Vec<usize>,
    pub print_text: String,
    pub note: String,
}

pub fn combinations(pool: &[u8], k: usize) -> Vec<Vec<u8>> {
    fn walk(pool: &[u8], k: usize, start: usize, current: &mut Vec<u8>, out: &mut Vec<Vec<u8>>) {
        if current.len() == k {
            out.push(current.clone());
            return;
        }
        let left = k - current.len();
        if pool.len().saturating_sub(start) < left {
            return;
        }
        for i in start..=pool.len() - left {
            current.push(pool[i]);
            walk(pool, k, i + 1, current, out);
            current.pop();
        }
    }
    let mut out = Vec::new();
    if k <= pool.len() {
        walk(pool, k, 0, &mut Vec::new(), &mut out);
    }
    out
}

pub fn ranking(
    history: &[Draw],
    upper: u8,
    front: bool,
    strategy: Strategy,
    rng: &mut ChaCha20Rng,
) -> Vec<u8> {
    let mut pool: Vec<u8> = (1..=upper).collect();
    pool.shuffle(rng);
    let mut frequency = vec![0i64; upper as usize];
    let mut omission = vec![history.len() as i64; upper as usize];
    for (age, draw) in history.iter().rev().enumerate() {
        for &n in if front { &draw.front } else { &draw.back } {
            frequency[n as usize - 1] += 1;
            if omission[n as usize - 1] == history.len() as i64 {
                omission[n as usize - 1] = age as i64;
            }
        }
    }
    pool.sort_by_key(|&n| match strategy {
        Strategy::Hot => -frequency[n as usize - 1],
        Strategy::Cold => frequency[n as usize - 1],
        Strategy::Omission => -omission[n as usize - 1],
        _ => 0,
    });
    pool
}

fn sample(pool: &[u8], size: usize, rng: &mut ChaCha20Rng) -> Vec<u8> {
    let mut values: Vec<u8> = pool.choose_multiple(rng, size).copied().collect();
    values.sort_unstable();
    values
}
fn balanced(upper: u8, k: usize, exposure: &[usize], rng: &mut ChaCha20Rng) -> Vec<u8> {
    let mut values: Vec<u8> = (1..=upper).collect();
    values.shuffle(rng);
    values.sort_by_key(|n| exposure[*n as usize - 1]);
    values.truncate(k);
    values.sort_unstable();
    values
}

pub fn generate(request: &GenerateRequest, seed: u64, history: &[Draw]) -> Result<Plan> {
    Ok(generate_internal(request, seed, history, false)?.0)
}
pub fn generate_with_evidence(
    request: &GenerateRequest,
    seed: u64,
    history: &[Draw],
) -> Result<(Plan, crate::evidence::SelectionEvidence)> {
    let (plan, evidence) = generate_internal(request, seed, history, true)?;
    Ok((plan, evidence.expect("evidence requested")))
}
fn generate_internal(
    request: &GenerateRequest,
    seed: u64,
    history: &[Draw],
    capture: bool,
) -> Result<(Plan, Option<crate::evidence::SelectionEvidence>)> {
    issue(&request.issue)?;
    ensure!((1..=99).contains(&request.multiplier), "倍数须为 1～99");
    let experimental = matches!(
        request.strategy,
        Strategy::Hot | Strategy::Cold | Strategy::Omission
    );
    ensure!(
        request.mode == Mode::Single
            || !matches!(request.strategy, Strategy::Coverage | Strategy::Structural),
        "均衡覆盖仅支持单式；复式和胆拖请选择随机或历史实验策略"
    );
    if request.mode != Mode::Single || experimental {
        ensure!(
            (5..=35).contains(&request.front_pool) && (2..=12).contains(&request.back_pool),
            "前区池须为 5～35，后区池须为 2～12"
        );
    }
    if experimental {
        ensure!((1..=500).contains(&request.window), "历史窗口须为 1～500");
    }
    if request.mode == Mode::Single {
        ensure!(
            (1..=MAX_TICKETS).contains(&request.bets),
            "单式注数须为 1～{MAX_TICKETS}"
        );
    }
    let mut req = request.clone();
    // Inputs that do not apply to this mode cannot secretly change its seed.
    if !experimental {
        req.window = 60;
    }
    if req.mode != Mode::Single {
        req.bets = 1;
    }
    if req.mode == Mode::Single && !experimental {
        req.front_pool = 35;
        req.back_pool = 12;
    }
    let validated = validate_history(history)?;
    let prior: Vec<_> = validated
        .into_iter()
        .filter(|d| d.issue < request.issue)
        .collect();
    let relevant = &prior[prior.len().saturating_sub(req.window)..];
    ensure!(
        !experimental || !relevant.is_empty(),
        "实验策略需要目标期之前的真实开奖数据，请先同步或导入"
    );
    req.front_numbers.sort_unstable();
    req.back_numbers.sort_unstable();
    req.front_bankers.sort_unstable();
    req.back_bankers.sort_unstable();
    if req.mode != Mode::Dantuo {
        ensure!(
            req.front_bankers.is_empty() && req.back_bankers.is_empty(),
            "仅胆拖模式可指定胆码"
        );
        req.front_banker_count = 0;
        req.back_banker_count = 0;
    }
    if req.mode == Mode::Single {
        ensure!(
            req.front_numbers.is_empty() && req.back_numbers.is_empty(),
            "自选号码池请使用复式或胆拖模式"
        );
    }
    let history_hash = digest(
        &relevant
            .iter()
            .filter(|_| experimental)
            .map(|d| (&d.issue, &d.front, &d.back))
            .collect::<Vec<_>>(),
    )?;
    let seed_fingerprint = digest(&seed)?;
    let mut selection = req.clone();
    selection.additional = false;
    selection.multiplier = 1;
    let seed_bytes: [u8; 32] = Sha256::digest(serde_json::to_vec(&(
        GENERATOR_VERSION,
        seed,
        &selection,
        &history_hash,
    ))?)
    .into();
    let mut rng = ChaCha20Rng::from_seed(seed_bytes);
    let front_rank = ranking(relevant, 35, true, req.strategy, &mut rng);
    let back_rank = ranking(relevant, 12, false, req.strategy, &mut rng);
    let mut front_bankers = Vec::new();
    let mut back_bankers = Vec::new();
    let mut tickets = Vec::new();
    let mut seen = BTreeSet::new();
    let mut front_exposure = vec![0; 35];
    let mut back_exposure = vec![0; 12];
    if req.mode == Mode::Single {
        let fp = if experimental {
            &front_rank[..req.front_pool]
        } else {
            &front_rank
        };
        let bp = if experimental {
            &back_rank[..req.back_pool]
        } else {
            &back_rank
        };
        ensure!(
            choose(fp.len(), 5) * choose(bp.len(), 2) >= req.bets as u64,
            "候选池不足以生成 {} 个不同单注，请扩大号码池或减少注数",
            req.bets
        );
        let mut attempts = 0;
        while tickets.len() < req.bets {
            attempts += 1;
            ensure!(
                attempts <= req.bets * 100 + 100,
                "候选池无法生成足够的不同单注，请减少注数"
            );
            let ticket = if matches!(req.strategy, Strategy::Coverage | Strategy::Structural) {
                Ticket {
                    front: balanced(35, 5, &front_exposure, &mut rng),
                    back: if req.strategy == Strategy::Structural {
                        let mut pairs = combinations(&(1..=12).collect::<Vec<_>>(), 2);
                        pairs.shuffle(&mut rng);
                        pairs.sort_by_key(|pair| {
                            (
                                tickets.iter().filter(|t: &&Ticket| t.back == *pair).count(),
                                pair.iter()
                                    .map(|n| back_exposure[*n as usize - 1])
                                    .sum::<usize>(),
                            )
                        });
                        pairs.remove(0)
                    } else {
                        balanced(12, 2, &back_exposure, &mut rng)
                    },
                }
            } else {
                Ticket {
                    front: sample(fp, 5, &mut rng),
                    back: sample(bp, 2, &mut rng),
                }
            };
            if seen.insert(ticket.clone()) {
                for &n in &ticket.front {
                    front_exposure[n as usize - 1] += 1;
                }
                for &n in &ticket.back {
                    back_exposure[n as usize - 1] += 1;
                }
                tickets.push(ticket);
            }
        }
    } else {
        let front_pool = if req.front_numbers.is_empty() {
            front_rank[..req.front_pool].to_vec()
        } else {
            numbers(&req.front_numbers, req.front_pool, req.front_pool, 35)?
        };
        let back_pool = if req.back_numbers.is_empty() {
            back_rank[..req.back_pool].to_vec()
        } else {
            numbers(&req.back_numbers, req.back_pool, req.back_pool, 12)?
        };
        if req.mode == Mode::Dantuo {
            ensure!(
                req.front_banker_count <= 4 && req.back_banker_count <= 1,
                "前区胆码最多 4 个，后区最多 1 个"
            );
            front_bankers = if req.front_bankers.is_empty() {
                front_pool[..req.front_banker_count].to_vec()
            } else {
                numbers(
                    &req.front_bankers,
                    req.front_banker_count,
                    req.front_banker_count,
                    35,
                )?
            };
            back_bankers = if req.back_bankers.is_empty() {
                back_pool[..req.back_banker_count].to_vec()
            } else {
                numbers(
                    &req.back_bankers,
                    req.back_banker_count,
                    req.back_banker_count,
                    12,
                )?
            };
            ensure!(
                !front_bankers.is_empty() || !back_bankers.is_empty(),
                "胆拖至少一个区需要胆码"
            );
            ensure!(
                front_bankers.iter().all(|n| front_pool.contains(n))
                    && back_bankers.iter().all(|n| back_pool.contains(n)),
                "胆码必须包含在对应号码池内"
            );
            ensure!(
                front_bankers.is_empty() || front_pool.len() >= 6,
                "前区胆拖至少需要 6 个号码（含胆码）"
            );
            ensure!(
                back_bankers.is_empty() || back_pool.len() >= 3,
                "后区胆拖至少需要 3 个号码（含胆码）"
            );
        }
        let count = choose(
            front_pool.len() - front_bankers.len(),
            5 - front_bankers.len(),
        ) * choose(back_pool.len() - back_bankers.len(), 2 - back_bankers.len());
        ensure!(
            count > 0 && count <= MAX_TICKETS as u64,
            "展开为 {count} 注，单方案上限 {MAX_TICKETS} 注，请缩小号码池"
        );
        let fronts = combinations(
            &front_pool
                .iter()
                .filter(|n| !front_bankers.contains(n))
                .copied()
                .collect::<Vec<_>>(),
            5 - front_bankers.len(),
        );
        let backs = combinations(
            &back_pool
                .iter()
                .filter(|n| !back_bankers.contains(n))
                .copied()
                .collect::<Vec<_>>(),
            2 - back_bankers.len(),
        );
        for f in fronts {
            for b in &backs {
                let mut front = f.clone();
                front.extend(&front_bankers);
                front.sort_unstable();
                let mut back = b.clone();
                back.extend(&back_bankers);
                back.sort_unstable();
                tickets.push(Ticket { front, back });
            }
        }
        tickets.sort();
        for ticket in &tickets {
            for &n in &ticket.front {
                front_exposure[n as usize - 1] += 1;
            }
            for &n in &ticket.back {
                back_exposure[n as usize - 1] += 1;
            }
        }
    }
    front_bankers.sort_unstable();
    back_bankers.sort_unstable();
    let front_pool: Vec<u8> = (1..=35)
        .filter(|&n| front_exposure[n as usize - 1] > 0)
        .collect();
    let back_pool: Vec<u8> = (1..=12)
        .filter(|&n| back_exposure[n as usize - 1] > 0)
        .collect();
    let cost_fen =
        tickets.len() as u64 * if req.additional { 300 } else { 200 } * req.multiplier as u64;
    ensure!(
        cost_fen <= if req.additional { 3_000_000 } else { 2_000_000 },
        "金额超过单张彩票限额，请减少注数或倍数"
    );
    let id = digest(&(
        GENERATOR_VERSION,
        &req,
        &history_hash,
        &seed_fingerprint,
        &tickets,
    ))?;
    let format = |v: &[u8]| {
        v.iter()
            .map(|n| format!("{n:02}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut print_text = format!(
        "体彩大乐透 {}期 · {}注 · {}倍 · {} · {:.2}元\n",
        req.issue,
        tickets.len(),
        req.multiplier,
        if req.additional { "追加" } else { "基本" },
        cost_fen as f64 / 100.
    );
    for (i, t) in tickets.iter().enumerate() {
        print_text.push_str(&format!(
            "{:03}. {} + {}\n",
            i + 1,
            format(&t.front),
            format(&t.back)
        ));
    }
    let plan=Plan { id, generator_version: GENERATOR_VERSION.into(), request: req, data_cutoff: prior.last().map(|d|d.issue.clone()),
        history_hash, seed_fingerprint, first_prize_probability: tickets.len() as f64 / TOTAL_OUTCOMES as f64,
        tickets, front_pool, back_pool, front_bankers, back_bankers, cost_fen, front_exposure, back_exposure, print_text,
        note: if experimental { "冷热号和遗漏仅供实验对照，历史评分未经校准，也未证明提高未来中奖概率。" } else { "同一期号、参数、种子和历史输入生成相同票组。不同单注增加组合覆盖；倍投和追加不增加头奖命中概率。" }.into() };
    let evidence = capture.then(|| {
        crate::evidence::SelectionEvidence::capture(&plan, relevant, &front_rank, &back_rank)
    });
    Ok((plan, evidence))
}

pub fn presets(issue: &str) -> Vec<(&'static str, GenerateRequest)> {
    let base = GenerateRequest {
        issue: issue.into(),
        ..Default::default()
    };
    vec![
        ("随机单式5注", base.clone()),
        (
            "均衡覆盖7注",
            GenerateRequest {
                strategy: Strategy::Coverage,
                bets: 7,
                ..base.clone()
            },
        ),
        (
            "前区6后区2复式",
            GenerateRequest {
                mode: Mode::Compound,
                front_pool: 6,
                back_pool: 2,
                ..base.clone()
            },
        ),
        (
            "前区5后区4复式",
            GenerateRequest {
                mode: Mode::Compound,
                front_pool: 5,
                back_pool: 4,
                ..base.clone()
            },
        ),
        (
            "双区胆拖",
            GenerateRequest {
                mode: Mode::Dantuo,
                front_pool: 7,
                back_pool: 3,
                front_banker_count: 2,
                back_banker_count: 1,
                ..base
            },
        ),
    ]
}
