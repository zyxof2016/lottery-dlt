use crate::{digest, model::issue, store::Store};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Duration, NaiveDateTime, TimeZone, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

pub const SCHEDULE_URL: &str =
    "https://webapi.sporttery.cn/gateway/lottery/getDigitalDrawInfoV1.qry?param=85,0&isVerify=1";
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub issue: String,
    pub deadline: String,
    pub source_url: String,
    pub source_hash: String,
    pub verified_at: String,
    #[serde(default)]
    pub basis: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Automation {
    pub enabled: bool,
    pub lead_minutes: i64,
}
impl Default for Automation {
    fn default() -> Self {
        Self {
            enabled: true,
            lead_minutes: 120,
        }
    }
}
pub fn parse_schedule(value: &Value) -> Result<Schedule> {
    ensure!(value["success"] == true, "官方日程接口未成功返回");
    let row = value
        .pointer("/value/dlt")
        .context("官方响应缺少大乐透日程")?;
    ensure!(
        row["lotteryDrawResult"]
            .as_str()
            .unwrap_or("")
            .trim()
            .is_empty(),
        "接口只返回已开奖期，未提供未来销售日程"
    );
    let issue = row["lotteryDrawNum"]
        .as_str()
        .context("日程缺少官方期号")?
        .to_string();
    crate::model::issue(&issue)?;
    let naive = NaiveDateTime::parse_from_str(
        row["lotterySaleEndtime"]
            .as_str()
            .context("日程缺少停售时间")?,
        "%Y-%m-%d %H:%M:%S",
    )?;
    let deadline = chrono::FixedOffset::east_opt(8 * 3600)
        .unwrap()
        .from_local_datetime(&naive)
        .single()
        .context("无效停售时间")?
        .with_timezone(&Utc);
    ensure!(
        deadline > Utc::now() && deadline <= Utc::now() + Duration::days(30),
        "官方日程已截止或超出30天范围"
    );
    ensure!(
        naive.format("%y").to_string() == issue[..2],
        "日程年份与期号不符"
    );
    Ok(Schedule {
        issue,
        deadline: deadline.to_rfc3339(),
        source_url: SCHEDULE_URL.into(),
        source_hash: digest(value)?,
        verified_at: Utc::now().to_rfc3339(),
        basis: "explicit_official_schedule".into(),
    })
}
pub const CALENDAR_URL: &str = "https://webapi.sporttery.cn/gateway/lottery/getCloseTemplateV1.qry";
pub const SALES_RULE_URL: &str = "https://m.lottery.gov.cn/xxgk/tzggz/20221220/10027143.html";
pub fn derive_schedule(
    history: &[crate::model::Draw],
    current: &Value,
    closures: &Value,
    rules: &str,
) -> Result<Schedule> {
    derive_schedule_at(history, current, closures, rules, Utc::now())
}
pub fn derive_schedule_at(
    history: &[crate::model::Draw],
    current: &Value,
    closures: &Value,
    rules: &str,
    now: DateTime<Utc>,
) -> Result<Schedule> {
    use chrono::{Datelike, NaiveDate, Weekday};
    ensure!(
        current["success"] == true && closures["success"] == true,
        "官方开奖或休市接口未成功"
    );
    ensure!(
        rules.contains("21:00") && rules.contains("销售截止"),
        "官方停售规则发生变化，须人工核查"
    );
    let latest = history.last().context("缺少开奖历史")?;
    ensure!(
        current
            .pointer("/value/dlt/lotteryDrawNum")
            .and_then(Value::as_str)
            == Some(&latest.issue),
        "开奖缓存落后于官方，须先同步"
    );
    let row = current.pointer("/value/dlt").context("官方最新开奖缺失")?;
    let published: Vec<u8> = row["lotteryDrawResult"]
        .as_str()
        .context("官方缺少最新开奖号码")?
        .split_whitespace()
        .map(str::parse)
        .collect::<std::result::Result<_, _>>()?;
    let mut cached = latest.front.clone();
    cached.extend(&latest.back);
    ensure!(
        published == cached
            && row["lotteryDrawTime"]
                .as_str()
                .is_some_and(|d| d.starts_with(&latest.date)),
        "最新开奖的号码或日期与官方不符，拒绝核实日程"
    );
    let year = now
        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
        .year();
    ensure!(
        latest.issue[..2] == format!("{:02}", year % 100),
        "跨年销售日程须等待本年官方开奖或明确公告，不能自行推算"
    );
    let closed: std::collections::BTreeSet<_> = closures
        .pointer("/value/dateArr")
        .and_then(Value::as_array)
        .context("官方休市数据缺失")?
        .iter()
        .map(|v| v.as_str().context("休市日期格式错误"))
        .collect::<Result<_>>()?;
    let draws: Vec<_> = history
        .iter()
        .filter(|d| d.issue[..2] == latest.issue[..2])
        .collect();
    let end = chrono::NaiveDate::parse_from_str(&latest.date, "%Y-%m-%d")?;
    let mut date = NaiveDate::from_ymd_opt(year, 1, 1).unwrap();
    let mut count = 0;
    while date <= end {
        if matches!(date.weekday(), Weekday::Mon | Weekday::Wed | Weekday::Sat)
            && !closed.contains(date.format("%Y%m%d").to_string().as_str())
        {
            count += 1;
            ensure!(
                draws
                    .get(count - 1)
                    .is_some_and(|d| d.issue == format!("{:02}{count:03}", year % 100)
                        && d.date == date.to_string()),
                "官方周期/休市表与全年实际开奖不符，拒绝推导日程"
            );
        }
        date = date.succ_opt().unwrap();
    }
    ensure!(count == draws.len(), "全年开奖缓存存在缺期");
    while !matches!(date.weekday(), Weekday::Mon | Weekday::Wed | Weekday::Sat)
        || closed.contains(date.format("%Y%m%d").to_string().as_str())
    {
        date = date.succ_opt().unwrap();
    }
    ensure!(date.year() == year, "跨年不推算期号");
    let deadline = chrono::FixedOffset::east_opt(8 * 3600)
        .unwrap()
        .from_local_datetime(&date.and_hms_opt(21, 0, 0).unwrap())
        .single()
        .unwrap()
        .with_timezone(&Utc);
    ensure!(
        deadline > now,
        "下一期已过停售时间但尚无开奖，暂停归档，等待官方结果"
    );
    Ok(Schedule {
        issue: format!("{:02}{:03}", year % 100, count + 1),
        deadline: deadline.to_rfc3339(),
        source_url: CALENDAR_URL.into(),
        source_hash: digest(&(current, closures, rules))?,
        verified_at: now.to_rfc3339(),
        basis: "official_rules_calendar_checked_against_full_year".into(),
    })
}
pub async fn fetch_schedule(history: &[crate::model::Draw]) -> Result<Schedule> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (compatible; lottery-dlt/1.1)")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let current: Value = client
        .get(SCHEDULE_URL)
        .header("Referer", "https://www.lottery.gov.cn/")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    if let Ok(explicit) = parse_schedule(&current) {
        return Ok(explicit);
    }
    let closures: Value = client
        .get(CALENDAR_URL)
        .header("Referer", "https://www.lottery.gov.cn/")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    let rules = client
        .get(SALES_RULE_URL)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    derive_schedule(history, &current, &closures, &rules)
}

impl Store {
    pub fn operation(&self, key: &str) -> Result<Option<Value>> {
        let body: Option<String> = self
            .connect()?
            .query_row("SELECT body FROM operations WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        body.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
    }
    pub fn set_operation(&self, key: &str, value: &Value) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("INSERT INTO operations VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET body=excluded.body",params![key,serde_json::to_string(value)?])?;
        Self::audit(&tx, "operation", &digest(&(key, value))?)?;
        tx.commit()?;
        Ok(())
    }
    pub fn strict_schedule(&self, enabled: bool) -> Result<()> {
        self.set_operation("strict_schedule", &json!(enabled))
    }
    pub fn set_schedule(&self, schedule: &Schedule) -> Result<()> {
        issue(&schedule.issue)?;
        ensure!(
            [SCHEDULE_URL, CALENDAR_URL].contains(&schedule.source_url.as_str())
                && schedule.source_hash.len() == 64,
            "销售日程须来自体彩官方接口"
        );
        let end = DateTime::parse_from_rfc3339(&schedule.deadline)?.with_timezone(&Utc);
        ensure!(
            end > Utc::now() && end <= Utc::now() + Duration::days(30),
            "日程已过期或超出30天"
        );
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let old: Option<String> = tx
            .query_row(
                "SELECT body FROM schedules WHERE issue=?1",
                [&schedule.issue],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(old) = old {
            let old: Schedule = serde_json::from_str(&old)?;
            ensure!(
                old.deadline == schedule.deadline,
                "官方截止时间变化，须先核查已有归档，拒绝静默修改"
            );
        }
        tx.execute("INSERT INTO schedules VALUES(?1,?2,?3) ON CONFLICT(issue) DO UPDATE SET body=excluded.body,content_hash=excluded.content_hash",params![schedule.issue,serde_json::to_string(schedule)?,digest(schedule)?])?;
        tx.commit()?;
        Ok(())
    }
    pub(crate) fn read_schedules(conn: &Connection) -> Result<Vec<Schedule>> {
        let mut stmt =
            conn.prepare("SELECT body,content_hash FROM schedules ORDER BY issue DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|r| {
            let (body, hash) = r?;
            let value: Schedule = serde_json::from_str(&body)?;
            ensure!(digest(&value)? == hash, "销售日程摘要校验失败");
            Ok(value)
        })
        .collect()
    }
    pub fn schedules(&self) -> Result<Vec<Schedule>> {
        Self::read_schedules(&self.connect()?)
    }
    pub(crate) fn check_schedule(
        conn: &Connection,
        issue: &str,
        deadline: DateTime<Utc>,
    ) -> Result<()> {
        let strict: Option<String> = conn
            .query_row(
                "SELECT body FROM operations WHERE key='strict_schedule'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if strict.as_deref() != Some("true") {
            return Ok(());
        }
        let schedule = Self::read_schedules(conn)?
            .into_iter()
            .find(|s| s.issue == issue)
            .context("目标期缺少已核实的官方销售日程，请先同步日程")?;
        let verified = DateTime::parse_from_rfc3339(&schedule.verified_at)?.with_timezone(&Utc);
        ensure!(
            verified <= Utc::now() && verified > Utc::now() - Duration::hours(2),
            "销售日程超过2小时未核实，须先同步"
        );
        let official = DateTime::parse_from_rfc3339(&schedule.deadline)?.with_timezone(&Utc);
        ensure!(deadline == official, "归档截止时间必须与官方停售时间一致");
        let draws = Self::read_draws(conn)?;
        let latest = draws.last().context("缺少真实开奖缓存，请先同步")?;
        let age = (Utc::now().date_naive()
            - chrono::NaiveDate::parse_from_str(&latest.date, "%Y-%m-%d")?)
        .num_days();
        ensure!(
            (age <= 4 || schedule.basis == "official_rules_calendar_checked_against_full_year")
                && latest.issue < schedule.issue,
            "开奖缓存过期或目标期已开奖"
        );
        for pair in draws.windows(2) {
            if pair[0].issue[..2] == pair[1].issue[..2] {
                ensure!(
                    pair[1].issue.parse::<u32>()? == pair[0].issue.parse::<u32>()? + 1,
                    "开奖缓存存在缺期，须补齐后归档"
                );
            }
        }
        Ok(())
    }
    pub fn automation_config(&self) -> Result<Automation> {
        Ok(self
            .operation("automation")?
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default())
    }
    pub fn configure_automation(&self, config: &Automation) -> Result<()> {
        ensure!(
            (15..=10080).contains(&config.lead_minutes),
            "提前留档窗口须为15分钟至7天"
        );
        self.set_operation("automation", &serde_json::to_value(config)?)
    }
    pub fn automation_events(&self) -> Result<Vec<Value>> {
        let conn = self.connect()?;
        let mut stmt =
            conn.prepare("SELECT body FROM automation_events ORDER BY issue DESC LIMIT 100")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| Ok(serde_json::from_str(&r?)?)).collect()
    }
    pub fn automate(&self) -> Result<Value> {
        let config = self.automation_config()?;
        let mut events = Vec::new();
        if !config.enabled {
            return Ok(json!({"enabled":false,"events":events}));
        }
        for schedule in self.schedules()? {
            let end = DateTime::parse_from_rfc3339(&schedule.deadline)?.with_timezone(&Utc);
            let existing = self.recommendations()?["batches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b["issue"] == schedule.issue);
            let (status, error) = if existing {
                let records = self.recommendations()?;
                let group: Vec<_> = records["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|r| r["plan"]["request"]["issue"] == schedule.issue)
                    .collect();
                if group.iter().all(|r| !r["settlement"].is_null()) {
                    if group
                        .iter()
                        .any(|r| r["settlement"]["total_prize_fen"].is_null())
                    {
                        ("prize_pending", None)
                    } else {
                        ("settled", None)
                    }
                } else {
                    ("archived", None)
                }
            } else if end <= Utc::now() {
                ("missed", Some("截止前未完成归档".to_string()))
            } else if end - Utc::now() > Duration::minutes(config.lead_minutes) {
                continue;
            } else {
                match self.archive_presets(&schedule.issue, &schedule.deadline) {
                    Ok(_) => ("archived", None),
                    Err(e) => ("failed", Some(e.to_string())),
                }
            };
            let event = json!({"issue":schedule.issue,"deadline":schedule.deadline,"status":status,"error":error,"checked_at":Utc::now().to_rfc3339()});
            self.connect()?.execute("INSERT INTO automation_events VALUES(?1,?2) ON CONFLICT(issue) DO UPDATE SET body=excluded.body",params![schedule.issue,serde_json::to_string(&event)?])?;
            events.push(event);
        }
        Ok(json!({"enabled":true,"events":events}))
    }
    pub fn backup(&self, destination: &Path) -> Result<Value> {
        ensure!(destination != self.path, "备份不能覆盖当前数据库");
        std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)?;
        self.connect()?
            .backup(rusqlite::DatabaseName::Main, destination, None)?;
        let copy = Self::open(destination)?;
        let verification = copy.export()?;
        {
            let conn = copy.connect()?;
            let status: String = conn.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            ensure!(status == "ok", "备份完整性校验失败");
            conn.pragma_update(None, "journal_mode", "DELETE")?;
        }
        let hash = hex::encode(Sha256::digest(std::fs::read(destination)?));
        Ok(
            json!({"path":destination,"content_hash":hash,"integrity":"ok","records_verified":verification["records"].as_array().unwrap().len(),"audit_entries_verified":verification["audit_entries_verified"]}),
        )
    }
    pub fn restore(source: &Path, destination: &Path) -> Result<Value> {
        ensure!(source.exists(), "备份不存在");
        // Restore to a new path. The running DB cannot be replaced accidentally.
        Self {
            path: source.to_path_buf(),
        }
        .backup(destination)
    }
}

impl Store {
    pub fn draw_page(&self, page: usize, limit: usize, filter: &str) -> Result<Value> {
        ensure!(
            (1..=100000).contains(&page) && (1..=100).contains(&limit),
            "分页参数超出范围"
        );
        ensure!(
            filter.len() <= 5 && filter.bytes().all(|b| b.is_ascii_digit()),
            "期号筛选只接受最多5位数字"
        );
        let conn = self.connect()?;
        let pattern = format!("{filter}%");
        let total: usize = conn.query_row(
            "SELECT COUNT(*) FROM draws WHERE issue LIKE ?1",
            [&pattern],
            |r| r.get(0),
        )?;
        let mut stmt = conn.prepare(
            "SELECT body FROM draws WHERE issue LIKE ?1 ORDER BY issue DESC LIMIT ?2 OFFSET ?3",
        )?;
        let rows = stmt.query_map(params![pattern, limit, (page - 1) * limit], |r| {
            r.get::<_, String>(0)
        })?;
        let draws = rows
            .map(|r| Ok(serde_json::from_str::<Value>(&r?)?))
            .collect::<Result<Vec<_>>>()?;
        Ok(json!({"draws":draws,"page":page,"limit":limit,"total":total}))
    }
    pub fn record_page(
        &self,
        page: usize,
        limit: usize,
        filter: &str,
        kind: Option<&str>,
    ) -> Result<Value> {
        self.record_page_with_status(page, limit, filter, kind, None)
    }
    pub fn record_page_with_status(
        &self,
        page: usize,
        limit: usize,
        filter: &str,
        kind: Option<&str>,
        status: Option<&str>,
    ) -> Result<Value> {
        ensure!(
            (1..=100000).contains(&page) && (1..=50).contains(&limit),
            "分页参数超出范围"
        );
        ensure!(
            filter.len() <= 5 && filter.bytes().all(|b| b.is_ascii_digit()),
            "期号筛选只接受最多5位数字"
        );
        ensure!(
            kind.is_none_or(|v| ["personal", "recommendation"].contains(&v)),
            "未知方案类别"
        );
        ensure!(
            status.is_none_or(|s| ["", "pending", "settled"].contains(&s)),
            "开奖状态须为全部、待开奖或已开奖"
        );
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let pattern = format!("{filter}%");
        let total: usize = tx.query_row(
            "SELECT COUNT(*) FROM plans WHERE issue LIKE ?1 AND (?2 IS NULL OR kind=?2) AND (?3 IS NULL OR ?3='' OR (?3='pending' AND NOT EXISTS(SELECT 1 FROM draws WHERE draws.issue=plans.issue)) OR (?3='settled' AND EXISTS(SELECT 1 FROM draws WHERE draws.issue=plans.issue)))",
            params![pattern, kind,status],
            |r| r.get(0),
        )?;
        let mut stmt=tx.prepare("SELECT body,content_hash FROM plans WHERE issue LIKE ?1 AND (?2 IS NULL OR kind=?2) AND (?3 IS NULL OR ?3='' OR (?3='pending' AND NOT EXISTS(SELECT 1 FROM draws WHERE draws.issue=plans.issue)) OR (?3='settled' AND EXISTS(SELECT 1 FROM draws WHERE draws.issue=plans.issue))) ORDER BY issue DESC,rowid DESC LIMIT ?4 OFFSET ?5")?;
        let rows = stmt.query_map(
            params![pattern, kind, status, limit, (page - 1) * limit],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )?;
        let mut records = Vec::new();
        let mut draws = Vec::new();
        for row in rows {
            let (body, hash) = row?;
            let mut saved: crate::store::SavedPlan = serde_json::from_str(&body)?;
            ensure!(digest(&saved)? == hash, "原始方案摘要不符");
            let draw: Option<String> = tx
                .query_row(
                    "SELECT body FROM draws WHERE issue=?1",
                    [&saved.plan.request.issue],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(draw) = draw {
                draws.push(serde_json::from_str::<Value>(&draw)?);
                saved.settlement = Some(crate::prizes::settle(
                    &saved.plan.tickets,
                    saved.plan.request.additional,
                    saved.plan.request.multiplier,
                    &serde_json::from_str(&draw)?,
                )?);
            }
            records.push(saved);
        }
        Ok(
            json!({"selection_evidence":Self::evidence_summaries(&tx,&records)?,"records":records,"draws":draws,"page":page,"limit":limit,"total":total}),
        )
    }
}
impl Store {
    pub fn recommendation_page(&self, page: usize, limit: usize, filter: &str) -> Result<Value> {
        ensure!(
            (1..=100000).contains(&page) && (1..=10).contains(&limit),
            "每页推荐期数须1～10"
        );
        ensure!(
            filter.len() <= 5 && filter.bytes().all(|b| b.is_ascii_digit()),
            "期号筛选只接受最多5位数字"
        );
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let pattern = format!("{filter}%");
        let total:usize=tx.query_row("SELECT COUNT(*) FROM (SELECT issue FROM recommendation_batches UNION SELECT issue FROM plans WHERE kind='recommendation') WHERE issue LIKE ?1",[&pattern],|r|r.get(0))?;
        let mut stmt=tx.prepare("SELECT issue FROM (SELECT issue FROM recommendation_batches UNION SELECT issue FROM plans WHERE kind='recommendation') WHERE issue LIKE ?1 ORDER BY issue DESC LIMIT ?2 OFFSET ?3")?;
        let issues = stmt
            .query_map(params![pattern, limit, (page - 1) * limit], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut batches = Vec::new();
        let mut records = Vec::new();
        let mut draws = Vec::new();
        for issue in issues {
            let body: Option<(String, String)> = tx
                .query_row(
                    "SELECT body,content_hash FROM recommendation_batches WHERE issue=?1",
                    [&issue],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()?;
            let batch = body
                .map(
                    |(body, hash)| -> Result<crate::store::RecommendationBatch> {
                        let value: crate::store::RecommendationBatch = serde_json::from_str(&body)?;
                        ensure!(
                            digest(&value)? == hash && value.issue == issue,
                            "推荐摘要校验失败"
                        );
                        Ok(value)
                    },
                )
                .transpose()?;
            let draw: Option<String> = tx
                .query_row("SELECT body FROM draws WHERE issue=?1", [&issue], |r| {
                    r.get(0)
                })
                .optional()?;
            let draw: Option<crate::model::Draw> =
                draw.map(|s| serde_json::from_str(&s)).transpose()?;
            if let Some(draw) = &draw {
                draws.push(draw.clone());
            }
            // Read the complete recommendation set directly. Personal records must
            // not displace frozen entries through the personal-page limit of 50.
            let mut stmt = tx.prepare(
                "SELECT id,kind,body,content_hash FROM plans WHERE issue=?1 ORDER BY rowid DESC",
            )?;
            let rows = stmt.query_map([&issue], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?;
            let mut found = std::collections::BTreeSet::new();
            for row in rows {
                let (id, kind, body, hash) = row?;
                if kind != "recommendation"
                    && !batch
                        .as_ref()
                        .is_some_and(|b| b.entries.iter().any(|e| e.plan_id == id))
                {
                    continue;
                }
                let mut saved: crate::store::SavedPlan = serde_json::from_str(&body)?;
                ensure!(
                    digest(&saved)? == hash && saved.id == id && saved.plan.request.issue == issue,
                    "原始方案摘要校验失败"
                );
                if let Some(draw) = &draw {
                    saved.settlement = Some(crate::prizes::settle(
                        &saved.plan.tickets,
                        saved.plan.request.additional,
                        saved.plan.request.multiplier,
                        draw,
                    )?);
                }
                found.insert(id);
                records.push(saved);
            }
            if let Some(batch) = batch {
                ensure!(
                    batch.entries.iter().all(|e| found.contains(&e.plan_id)),
                    "推荐清单缺少原票组"
                );
                batches.push(batch);
            }
        }
        Ok(
            json!({"selection_evidence":Self::evidence_summaries(&tx,&records)?,"records":records,"draws":draws,"batches":batches,"total":total,"page":page,"limit":limit}),
        )
    }
}
impl Store {
    pub fn verified_backup(&self, destination: &Path) -> Result<Value> {
        let mut result = self.backup(destination)?;
        let restored =
            destination.with_extension(format!("restore-check-{}.sqlite3", rand::random::<u64>()));
        let restore_result = Self::restore(destination, &restored);
        let verification = (|| -> Result<()> {
            restore_result?;
            let original = Self {
                path: destination.to_path_buf(),
            }
            .export()?;
            let restored_data = Self {
                path: restored.clone(),
            }
            .export()?;
            for key in [
                "settings",
                "draws",
                "records",
                "recommendations",
                "schedules",
                "selection_evidence",
                "audit_entries_verified",
            ] {
                ensure!(original[key] == restored_data[key], "恢复校验失败：{key}");
            }
            Ok(())
        })();
        for path in [
            &restored,
            &restored.with_extension("sqlite3-wal"),
            &restored.with_extension("sqlite3-shm"),
        ] {
            let _ = std::fs::remove_file(path);
        }
        verification?;
        result["restore_verified"] = json!(true);
        self.set_operation(
            "last_backup",
            &json!({"at":Utc::now().to_rfc3339(),"verification":result}),
        )?;
        Ok(result)
    }
}
