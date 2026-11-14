use anyhow::{bail, ensure, Context, Result};
use chrono::{Datelike, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const TOTAL_OUTCOMES: u64 = 21_425_712;
pub const MAX_TICKETS: usize = 1_000;

pub fn issue(value: &str) -> Result<()> {
    ensure!(
        value.len() == 5 && value.bytes().all(|c| c.is_ascii_digit()),
        "期号须为五位数字，例如 26118"
    );
    ensure!(&value[2..] != "000", "期序不能为 000");
    Ok(())
}

pub fn numbers(values: &[u8], min: usize, max: usize, upper: u8) -> Result<Vec<u8>> {
    ensure!(
        (min..=max).contains(&values.len()),
        "号码数量须在 {min}～{max} 之间"
    );
    ensure!(
        values.iter().all(|n| (1..=upper).contains(n)),
        "号码须在 1～{upper} 之间"
    );
    let unique: BTreeSet<_> = values.iter().copied().collect();
    ensure!(unique.len() == values.len(), "同一区号码不能重复");
    Ok(unique.into_iter().collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    pub front: Vec<u8>,
    pub back: Vec<u8>,
}
impl Ticket {
    pub fn validate(&self) -> Result<Self> {
        Ok(Self {
            front: numbers(&self.front, 5, 5, 35)?,
            back: numbers(&self.back, 2, 2, 12)?,
        })
    }
    pub fn hits(&self, draw: &Draw) -> (u8, u8) {
        (
            self.front.iter().filter(|n| draw.front.contains(n)).count() as u8,
            self.back.iter().filter(|n| draw.back.contains(n)).count() as u8,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Award {
    pub basic_fen: Option<u64>,
    #[serde(default)]
    pub additional_fen: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Draw {
    pub issue: String,
    pub date: String,
    pub front: Vec<u8>,
    pub back: Vec<u8>,
    #[serde(default)]
    pub awards: BTreeMap<u8, Award>,
    /// Opening pool, never the balance rolled into the next issue.
    #[serde(default)]
    pub pool_before_fen: Option<u64>,
    #[serde(default = "manual_source")]
    pub source: String,
}
fn manual_source() -> String {
    "manual".into()
}
impl Draw {
    pub fn validate(&self) -> Result<Self> {
        issue(&self.issue)?;
        let date =
            NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").context("日期须为 YYYY-MM-DD")?;
        ensure!(
            (2007..=2099).contains(&date.year())
                && date.format("%y").to_string() == self.issue[..2],
            "日期年份与期号不符"
        );
        ensure!(date <= Utc::now().date_naive(), "不能导入未来开奖");
        ensure!(self.source.len() <= 512, "数据来源过长");
        let max_level = if self.issue.as_str() >= "26014" { 7 } else { 9 };
        ensure!(
            self.awards.keys().all(|n| (1..=max_level).contains(n)),
            "奖级不适用于本期规则"
        );
        ensure!(
            self.awards
                .values()
                .all(|a| a.basic_fen.unwrap_or(0) <= 10_000_000_000
                    && a.additional_fen.unwrap_or(0) <= 10_000_000_000),
            "奖金金额异常（单位为分）"
        );
        Ok(Self {
            front: numbers(&self.front, 5, 5, 35)?,
            back: numbers(&self.back, 2, 2, 12)?,
            ..self.clone()
        })
    }
}

pub fn validate_history(draws: &[Draw]) -> Result<Vec<Draw>> {
    let mut result = draws
        .iter()
        .map(Draw::validate)
        .collect::<Result<Vec<_>>>()?;
    result.sort_by(|a, b| a.issue.cmp(&b.issue));
    for pair in result.windows(2) {
        ensure!(pair[0].issue < pair[1].issue, "重复期号 {}", pair[0].issue);
        ensure!(pair[0].date < pair[1].date, "开奖日期须随期号严格递增");
    }
    Ok(result)
}

pub fn choose(n: usize, k: usize) -> u64 {
    if k > n {
        return 0;
    }
    let k = k.min(n - k);
    (0..k).fold(1, |v, i| v * (n - i) as u64 / (i + 1) as u64)
}

pub fn money_fen(value: &str) -> Result<u64> {
    let clean = value.replace([',', '，'], "");
    let parts: Vec<_> = clean.trim().split('.').collect();
    ensure!(
        parts.len() <= 2 && !parts[0].is_empty() && parts[0].bytes().all(|c| c.is_ascii_digit()),
        "无效金额"
    );
    let whole: u64 = parts[0].parse()?;
    let fraction = match parts.get(1) {
        None | Some(&"") => 0,
        Some(v) if v.len() <= 2 && v.bytes().all(|c| c.is_ascii_digit()) => {
            v.parse::<u64>()? * if v.len() == 1 { 10 } else { 1 }
        }
        _ => bail!("奖金最多保留两位小数"),
    };
    whole
        .checked_mul(100)
        .and_then(|v| v.checked_add(fraction))
        .context("金额过大")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    #[serde(with = "seed_text")]
    pub seed: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            seed: rand::random(),
        }
    }
}

// Preserve all 64 seed bits when settings pass through a JavaScript browser.
pub(crate) mod seed_text {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Text(String),
            Number(u64),
        }
        match Input::deserialize(deserializer)? {
            Input::Text(s) => s.parse().map_err(serde::de::Error::custom),
            Input::Number(n) => Ok(n),
        }
    }
}
