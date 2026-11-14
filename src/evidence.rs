//! Scores and tie information captured alongside a new plan, never inferred for old archives.
use crate::{
    model::Draw,
    plan::{Mode, Plan, Strategy},
};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NumberEvidence {
    pub number: u8,
    pub rank: usize,
    pub score: Option<i64>,
    pub tie_first_rank: Option<usize>,
    pub tie_last_rank: Option<usize>,
    pub tie_count: Option<usize>,
    pub frequency: Option<usize>,
    pub omission: Option<usize>,
    pub omission_censored: Option<bool>,
    pub in_candidate_pool: bool,
    pub in_tickets: bool,
    pub banker: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ZoneEvidence {
    pub score_status: String,
    pub score_kind: String,
    pub all_scores_equal: Option<bool>,
    pub candidate_basis: String,
    pub boundary_tie: Option<(usize, usize, usize)>,
    pub numbers: Vec<NumberEvidence>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SelectionEvidence {
    pub schema_version: u8,
    pub plan_id: String,
    pub generator_version: String,
    pub history_hash: String,
    pub data_cutoff: Option<String>,
    pub scored_periods: usize,
    pub front: ZoneEvidence,
    pub back: ZoneEvidence,
    pub note: String,
}
impl SelectionEvidence {
    pub fn capture(plan: &Plan, history: &[Draw], front_rank: &[u8], back_rank: &[u8]) -> Self {
        let scored = matches!(
            plan.request.strategy,
            Strategy::Hot | Strategy::Cold | Strategy::Omission
        );
        let zone = |front: bool, rank: &[u8]| {
            let upper = if front { 35 } else { 12 };
            let manual = if front {
                &plan.request.front_numbers
            } else {
                &plan.request.back_numbers
            };
            let size = if front {
                plan.request.front_pool
            } else {
                plan.request.back_pool
            };
            let prefix = (plan.request.mode == Mode::Single && scored)
                || (plan.request.mode != Mode::Single && manual.is_empty());
            let candidates = if prefix {
                rank[..size].to_vec()
            } else if !manual.is_empty() {
                manual.clone()
            } else {
                rank.to_vec()
            };
            let tickets = if front {
                &plan.front_pool
            } else {
                &plan.back_pool
            };
            let bankers = if front {
                &plan.front_bankers
            } else {
                &plan.back_bankers
            };
            let mut scores = vec![None; upper];
            let mut frequencies = vec![0; upper];
            let mut ages = vec![history.len(); upper];
            if scored {
                for (age, draw) in history.iter().rev().enumerate() {
                    for &number in if front { &draw.front } else { &draw.back } {
                        frequencies[number as usize - 1] += 1;
                        if ages[number as usize - 1] == history.len() {
                            ages[number as usize - 1] = age;
                        }
                    }
                }
                for i in 0..upper {
                    scores[i] = Some(match plan.request.strategy {
                        Strategy::Hot => frequencies[i] as i64,
                        Strategy::Cold => -(frequencies[i] as i64),
                        _ => ages[i] as i64,
                    });
                }
            }
            let numbers = rank
                .iter()
                .enumerate()
                .map(|(i, &number)| {
                    let score = scores[number as usize - 1];
                    let ties: Vec<_> = rank
                        .iter()
                        .enumerate()
                        .filter(|(_, n)| scores[**n as usize - 1] == score)
                        .map(|(i, _)| i + 1)
                        .collect();
                    NumberEvidence {
                        number,
                        rank: i + 1,
                        score,
                        tie_first_rank: score.map(|_| ties[0]),
                        tie_last_rank: score.map(|_| *ties.last().unwrap()),
                        tie_count: score.map(|_| ties.len()),
                        frequency: scored.then_some(frequencies[number as usize - 1]),
                        omission: scored.then_some(ages[number as usize - 1]),
                        omission_censored: scored
                            .then_some(ages[number as usize - 1] == history.len()),
                        in_candidate_pool: candidates.contains(&number),
                        in_tickets: tickets.contains(&number),
                        banker: bankers.contains(&number),
                    }
                })
                .collect::<Vec<_>>();
            let boundary_tie = if scored
                && prefix
                && size < upper
                && numbers[size - 1].score == numbers[size].score
            {
                Some((
                    size,
                    numbers[size - 1].tie_first_rank.unwrap(),
                    numbers[size - 1].tie_last_rank.unwrap(),
                ))
            } else {
                None
            };
            ZoneEvidence {
                score_status: if scored {
                    "recorded_scores"
                } else {
                    "not_scored"
                }
                .into(),
                score_kind: match plan.request.strategy {
                    Strategy::Hot => "frequency_desc",
                    Strategy::Cold => "negative_frequency_desc",
                    Strategy::Omission => "omission_desc",
                    _ => "no_historical_score",
                }
                .into(),
                all_scores_equal: scored.then(|| scores.iter().all(|s| *s == scores[0])),
                candidate_basis: if prefix {
                    "rank_prefix"
                } else if !manual.is_empty() {
                    "manual_pool"
                } else {
                    "whole_zone"
                }
                .into(),
                boundary_tie,
                numbers,
            }
        };
        Self {schema_version:1,plan_id:plan.id.clone(),generator_version:plan.generator_version.clone(),history_hash:plan.history_hash.clone(),data_cutoff:plan.data_cutoff.clone(),scored_periods:if scored {history.len()}else{0},
            front:zone(true,front_rank),back:zone(false,back_rank),note:"分值是历史启发式排序依据，不是中奖概率。同分名次按原生成器的固定种子破局；排在前面不代表分值更高。候选池与最终入票号码分别记录；未记录的旧档案不补造评分。".into()}
    }
    pub fn validate(&self, plan: &Plan) -> Result<()> {
        ensure!(
            self.schema_version == 1
                && self.plan_id == plan.id
                && self.generator_version == plan.generator_version
                && self.history_hash == plan.history_hash
                && self.data_cutoff == plan.data_cutoff,
            "选号依据身份校验失败"
        );
        for (zone, upper, used, bankers, manual, size) in [
            (
                &self.front,
                35,
                &plan.front_pool,
                &plan.front_bankers,
                &plan.request.front_numbers,
                plan.request.front_pool,
            ),
            (
                &self.back,
                12,
                &plan.back_pool,
                &plan.back_bankers,
                &plan.request.back_numbers,
                plan.request.back_pool,
            ),
        ] {
            ensure!(zone.numbers.len() == upper, "选号依据号码数量校验失败");
            let ids: BTreeSet<_> = zone.numbers.iter().map(|r| r.number).collect();
            ensure!(
                ids == (1..=upper as u8).collect(),
                "选号依据号码范围校验失败"
            );
            let scored = zone.score_status == "recorded_scores";
            ensure!(
                scored || zone.score_status == "not_scored",
                "选号依据评分类型校验失败"
            );
            ensure!(
                scored
                    == matches!(
                        plan.request.strategy,
                        Strategy::Hot | Strategy::Cold | Strategy::Omission
                    ),
                "选号依据策略校验失败"
            );
            ensure!(
                !scored || zone.numbers.windows(2).all(|p| p[0].score >= p[1].score),
                "选号依据分值顺序校验失败"
            );
            let prefix = (plan.request.mode == Mode::Single && scored)
                || (plan.request.mode != Mode::Single && manual.is_empty());
            let basis = if prefix {
                "rank_prefix"
            } else if !manual.is_empty() {
                "manual_pool"
            } else {
                "whole_zone"
            };
            let kind = match plan.request.strategy {
                Strategy::Hot => "frequency_desc",
                Strategy::Cold => "negative_frequency_desc",
                Strategy::Omission => "omission_desc",
                _ => "no_historical_score",
            };
            ensure!(
                zone.candidate_basis == basis
                    && zone.score_kind == kind
                    && if scored {
                        self.scored_periods > 0 && self.scored_periods <= plan.request.window
                    } else {
                        self.scored_periods == 0
                    },
                "选号依据候选或窗口校验失败"
            );
            let boundary = if scored
                && prefix
                && size < upper
                && zone.numbers[size - 1].score == zone.numbers[size].score
            {
                Some((
                    size,
                    zone.numbers[size - 1].tie_first_rank.unwrap_or(0),
                    zone.numbers[size - 1].tie_last_rank.unwrap_or(0),
                ))
            } else {
                None
            };
            ensure!(zone.boundary_tie == boundary, "选号依据截取边界校验失败");
            ensure!(
                zone.all_scores_equal
                    == scored.then(|| zone
                        .numbers
                        .iter()
                        .all(|r| r.score == zone.numbers[0].score)),
                "等分依据校验失败"
            );
            for (i, row) in zone.numbers.iter().enumerate() {
                ensure!(
                    row.rank == i + 1
                        && row.score.is_some() == scored
                        && row.in_tickets == used.contains(&row.number)
                        && row.banker == bankers.contains(&row.number)
                        && row.in_candidate_pool
                            == if prefix {
                                i < size
                            } else if manual.is_empty() {
                                true
                            } else {
                                manual.contains(&row.number)
                            }
                        && (!row.in_tickets || row.in_candidate_pool),
                    "选号依据排名或入票校验失败"
                );
                if scored {
                    ensure!(
                        row.frequency.is_some_and(|n| n <= self.scored_periods)
                            && row.omission.is_some_and(|n| n <= self.scored_periods)
                            && row.omission_censored
                                == row.omission.map(|n| n == self.scored_periods),
                        "选号依据历史统计校验失败"
                    );
                    let score = match plan.request.strategy {
                        Strategy::Hot => row.frequency.map(|n| n as i64),
                        Strategy::Cold => row.frequency.map(|n| -(n as i64)),
                        _ => row.omission.map(|n| n as i64),
                    };
                    ensure!(row.score == score, "选号依据分值校验失败");
                } else {
                    ensure!(
                        row.frequency.is_none()
                            && row.omission.is_none()
                            && row.omission_censored.is_none(),
                        "未评分策略历史统计校验失败"
                    );
                }
                let ties: Vec<_> = zone
                    .numbers
                    .iter()
                    .filter(|r| r.score == row.score)
                    .map(|r| r.rank)
                    .collect();
                ensure!(
                    row.tie_first_rank == scored.then_some(ties[0])
                        && row.tie_last_rank == scored.then_some(*ties.last().unwrap())
                        && row.tie_count == scored.then_some(ties.len()),
                    "同分区间校验失败"
                );
            }
        }
        Ok(())
    }
    pub fn summary(&self, hash: &str) -> Value {
        json!({"status":"recorded","content_hash":hash,"front_all_scores_equal":self.front.all_scores_equal,"back_all_scores_equal":self.back.all_scores_equal,
            "front_candidate_basis":self.front.candidate_basis,"back_candidate_basis":self.back.candidate_basis,
            "front_score_status":self.front.score_status,"back_score_status":self.back.score_status})
    }
    pub fn review(&self, draw: Option<&Draw>) -> Value {
        let zone = |evidence: &ZoneEvidence, front: bool| {
            let result = draw.map(|d| if front { &d.front } else { &d.back });
            let partition = |candidate: bool, ticket: bool| {
                result.map(|values| {
                    values
                        .iter()
                        .copied()
                        .filter(|n| {
                            let row = evidence.numbers.iter().find(|r| r.number == *n).unwrap();
                            row.in_candidate_pool == candidate && row.in_tickets == ticket
                        })
                        .collect::<Vec<_>>()
                })
            };
            json!({"covered":partition(true,true),"in_pool_not_ticketed":partition(true,false),"outside_pool":partition(false,false)})
        };
        json!({"draw_issue":draw.map(|d|&d.issue),"front":zone(&self.front,true),"back":zone(&self.back,false)})
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordedEvidence {
    pub content_hash: String,
    pub evidence: SelectionEvidence,
}
impl crate::store::Store {
    pub(crate) fn read_selection_evidence(
        tx: &rusqlite::Connection,
        plan: &Plan,
    ) -> Result<Option<RecordedEvidence>> {
        use anyhow::Context;
        use rusqlite::OptionalExtension;
        let row: Option<(String, String)> = tx
            .query_row(
                "SELECT body,content_hash FROM selection_evidence WHERE plan_id=?1",
                [&plan.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        row.map(|(body, hash)| {
            let evidence: SelectionEvidence =
                serde_json::from_str(&body).context("选号依据格式校验失败")?;
            ensure!(crate::digest(&evidence)? == hash, "选号依据摘要校验失败");
            evidence.validate(plan)?;
            Ok(RecordedEvidence {
                content_hash: hash,
                evidence,
            })
        })
        .transpose()
    }
    pub(crate) fn evidence_summaries(
        tx: &rusqlite::Connection,
        records: &[crate::store::SavedPlan],
    ) -> Result<Value> {
        let mut map = serde_json::Map::new();
        for record in records {
            let summary = match Self::read_selection_evidence(tx, &record.plan)? {
                Some(stored) => stored.evidence.summary(&stored.content_hash),
                None => json!({"status":"not_recorded"}),
            };
            map.insert(record.id.clone(), summary);
        }
        Ok(Value::Object(map))
    }
    pub(crate) fn export_selection_evidence(
        tx: &rusqlite::Connection,
        records: &[crate::store::SavedPlan],
    ) -> Result<Value> {
        let mut map = serde_json::Map::new();
        for record in records {
            if let Some(stored) = Self::read_selection_evidence(tx, &record.plan)? {
                map.insert(record.id.clone(), serde_json::to_value(stored)?);
            }
        }
        let count: usize =
            tx.query_row("SELECT COUNT(*) FROM selection_evidence", [], |r| r.get(0))?;
        ensure!(count == map.len(), "选号依据存在无原票组引用，校验失败");
        Ok(Value::Object(map))
    }
    pub fn selection_evidence(&self, id: &str) -> Result<Value> {
        use anyhow::Context;
        use rusqlite::OptionalExtension;
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let row: Option<(String, String)> = tx
            .query_row(
                "SELECT body,content_hash FROM plans WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let (body, hash) = row.context("方案不存在")?;
        let record: crate::store::SavedPlan = serde_json::from_str(&body)?;
        ensure!(
            crate::digest(&record)? == hash && record.id == id,
            "原始方案摘要校验失败"
        );
        let stored = Self::read_selection_evidence(&tx, &record.plan)?;
        let draw: Option<String> = tx
            .query_row(
                "SELECT body FROM draws WHERE issue=?1",
                [&record.plan.request.issue],
                |r| r.get(0),
            )
            .optional()?;
        let draw: Option<Draw> = draw.map(|body| serde_json::from_str(&body)).transpose()?;
        let audit =
            crate::structure_audit::audit(&record.plan, stored.as_ref().map(|item| &item.evidence));
        Ok(match stored {
            Some(stored) => {
                json!({"status":"recorded","plan_id":id,"content_hash":stored.content_hash,"review":stored.evidence.review(draw.as_ref()),"evidence":stored.evidence,"structure_audit":audit})
            }
            None => {
                json!({"status":"not_recorded","plan_id":id,"evidence":null,"review":null,"note":"原档案未记录评分，不能按当前历史或种子补造当时依据。","structure_audit":audit})
            }
        })
    }
    pub fn preview_evidence(
        &self,
        request: &crate::plan::GenerateRequest,
        preview_id: &str,
    ) -> Result<Value> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let settings: String =
            tx.query_row("SELECT body FROM settings WHERE id=1", [], |r| r.get(0))?;
        let seed = serde_json::from_str::<crate::model::Settings>(&settings)?.seed;
        let draws = Self::read_draws(&tx)?;
        ensure!(
            draws.last().is_none_or(|d| request.issue > d.issue),
            "目标期已开奖，请查看原始留档依据"
        );
        let (plan, evidence) = crate::plan::generate_with_evidence(request, seed, &draws)?;
        ensure!(plan.id == preview_id, "预览已失效，请重新生成后查看依据");
        let audit = crate::structure_audit::audit(&plan, Some(&evidence));
        Ok(
            json!({"status":"preview_only","plan_id":plan.id,"content_hash":crate::digest(&evidence)?,"review":evidence.review(None),"evidence":evidence,"structure_audit":audit}),
        )
    }
}
