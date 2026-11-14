use crate::model::{money_fen, validate_history, Award, Draw};
use anyhow::{ensure, Context, Result};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    time::Duration,
};

pub const OFFICIAL_URL: &str =
    "https://webapi.sporttery.cn/gateway/lottery/getHistoryPageListV1.qry";

pub fn parse_official(payload: &Value) -> Result<Vec<Draw>> {
    ensure!(
        payload.get("success").and_then(Value::as_bool) != Some(false),
        "体彩接口返回失败"
    );
    let rows = payload
        .pointer("/value/list")
        .and_then(Value::as_array)
        .context("体彩响应缺少 value.list，拒绝替换缓存")?;
    let mut records = Vec::new();
    for row in rows {
        let issue = row["lotteryDrawNum"]
            .as_str()
            .context("缺少期号")?
            .to_string();
        let time = row["lotteryDrawTime"].as_str().context("缺少开奖日期")?;
        let date = time.get(..10).context("无效开奖日期")?.to_string();
        let nums = row["lotteryDrawResult"]
            .as_str()
            .context("缺少开奖号码")?
            .split_whitespace()
            .map(str::parse::<u8>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(nums.len() == 7, "{} 期应有 5+2 个号码", issue);
        let mut awards = BTreeMap::new();
        // Some official responses contain a prize list; history-only responses
        // do not. Absence remains unknown and is never converted to zero.
        if let Some(list) = row.get("prizeLevelList").and_then(Value::as_array) {
            for item in list {
                let label = item["prizeLevel"].as_str().unwrap_or("");
                // Promotion rows can follow the base row with the same grade.
                // They are conditional bonuses, not replacement base awards.
                if label.contains("派奖") || label.contains("加奖") || label.contains("特别奖")
                {
                    continue;
                }
                let level = [
                    "一等奖",
                    "二等奖",
                    "三等奖",
                    "四等奖",
                    "五等奖",
                    "六等奖",
                    "七等奖",
                    "八等奖",
                    "九等奖",
                ]
                .iter()
                .position(|name| label.contains(name))
                .map(|n| n as u8 + 1);
                if let Some(grade) = level {
                    let amount = &item["stakeAmount"];
                    let text = amount
                        .as_str()
                        .map(String::from)
                        .unwrap_or_else(|| amount.to_string());
                    if text == "null" || text == "---" || text == "--" {
                        continue;
                    }
                    let amount = money_fen(&text)?;
                    let award: &mut Award = awards.entry(grade).or_default();
                    let slot = if label.contains("追加") {
                        &mut award.additional_fen
                    } else {
                        &mut award.basic_fen
                    };
                    ensure!(
                        slot.is_none_or(|old| old == amount),
                        "{} 期{}存在冲突的基础奖额，拒绝覆盖",
                        issue,
                        label
                    );
                    *slot = Some(amount);
                }
            }
        }
        records.push(Draw {
            issue,
            date,
            front: nums[..5].to_vec(),
            back: nums[5..].to_vec(),
            awards,
            pool_before_fen: None,
            source: OFFICIAL_URL.into(),
        });
    }
    validate_history(&records)
}

pub async fn official(pages: usize) -> Result<Vec<Draw>> {
    ensure!((1..=50).contains(&pages), "同步页数须为 1～50（每页100期）");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(8))
        .user_agent("lottery-dlt/1.0 (+https://www.lottery.gov.cn/)")
        .build()?;
    let mut seen = HashSet::new();
    let mut records = Vec::new();
    for page in 1..=pages {
        let mut response = client
            .get(OFFICIAL_URL)
            .header("Referer", "https://www.lottery.gov.cn/")
            .query(&[
                ("gameNo", "85"),
                ("provinceId", "0"),
                ("pageSize", "100"),
                ("isVerify", "1"),
                ("pageNo", &page.to_string()),
            ])
            .send()
            .await
            .context("连接体彩官方接口失败；可使用 import 导入已核实的 JSON/CSV，原有缓存保持不变")?
            .error_for_status()?;
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                body.len() + chunk.len() <= 8 * 1024 * 1024,
                "体彩响应超出8MB上限"
            );
            body.extend_from_slice(&chunk);
        }
        let rows =
            parse_official(&serde_json::from_slice(&body).context("体彩接口未返回有效 JSON")?)?;
        if rows.is_empty() {
            break;
        }
        let fingerprint = rows.iter().map(|d| d.issue.clone()).collect::<Vec<_>>();
        ensure!(seen.insert(fingerprint), "体彩分页重复，整次同步已拒绝");
        records.extend(rows);
    }
    ensure!(!records.is_empty(), "体彩未返回开奖数据，原缓存保持不变");
    validate_history(&records)
}

pub fn load(path: &Path) -> Result<Vec<Draw>> {
    if path.extension().is_some_and(|e| e == "csv") {
        #[derive(serde::Deserialize)]
        struct Row {
            issue: String,
            date: String,
            front: String,
            back: String,
        }
        let mut reader = csv::Reader::from_path(path)?;
        let mut records = Vec::new();
        for row in reader.deserialize::<Row>() {
            let row = row?;
            let parse = |s: &str| -> Result<Vec<u8>> {
                s.split(|c: char| c.is_whitespace() || c == ',')
                    .filter(|v| !v.is_empty())
                    .map(|n| Ok(n.parse()?))
                    .collect()
            };
            records.push(Draw {
                issue: row.issue,
                date: row.date,
                front: parse(&row.front)?,
                back: parse(&row.back)?,
                awards: BTreeMap::new(),
                pool_before_fen: None,
                source: "csv_import".into(),
            });
        }
        validate_history(&records)
    } else {
        let value: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        if value.pointer("/value/list").is_some() {
            parse_official(&value)
        } else if let Some(draws) = value.get("draws") {
            validate_history(&serde_json::from_value::<Vec<Draw>>(draws.clone())?)
        } else {
            validate_history(&serde_json::from_value::<Vec<Draw>>(value)?)
        }
    }
}
