use crate::{
    digest,
    model::{validate_history, Draw, Settings},
    plan::{generate, presets, GenerateRequest, Plan},
    prizes::{settle, Settlement},
};
use anyhow::{ensure, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct Store {
    pub path: PathBuf,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPlan {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub created_at: String,
    pub deadline: String,
    pub plan: Plan,
    pub settlement: Option<Settlement>,
    pub integrity_valid: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<crate::operations::Schedule>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveRequest {
    pub request: GenerateRequest,
    pub preview_id: String,
    pub deadline: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationBatch {
    pub issue: String,
    pub created_at: String,
    pub deadline: String,
    pub entries: Vec<RecommendationEntry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationEntry {
    pub label: String,
    pub plan_id: String,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let store = Self {
            path: path.as_ref().to_path_buf(),
        };
        if let Some(parent) = store.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let connection = store.connect()?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS settings (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS draws (issue TEXT PRIMARY KEY, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS plans (id TEXT PRIMARY KEY, issue TEXT NOT NULL, kind TEXT NOT NULL, body TEXT NOT NULL, content_hash TEXT NOT NULL);
            CREATE INDEX IF NOT EXISTS plans_issue ON plans(issue);
            CREATE TABLE IF NOT EXISTS selection_evidence (plan_id TEXT PRIMARY KEY, body TEXT NOT NULL, content_hash TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS recommendation_batches (issue TEXT PRIMARY KEY, body TEXT NOT NULL, content_hash TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS operations (key TEXT PRIMARY KEY, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS schedules (issue TEXT PRIMARY KEY, body TEXT NOT NULL, content_hash TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS automation_events (issue TEXT PRIMARY KEY, body TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS jobs (id TEXT PRIMARY KEY, state TEXT NOT NULL, body TEXT NOT NULL, history TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS audit (seq INTEGER PRIMARY KEY, time TEXT NOT NULL, action TEXT NOT NULL, object_id TEXT NOT NULL, previous_hash TEXT NOT NULL, hash TEXT NOT NULL);")?;
        connection.execute(
            "INSERT OR IGNORE INTO settings VALUES(1, ?1)",
            [serde_json::to_string(&Settings::default())?],
        )?;
        Ok(store)
    }
    pub(crate) fn connect(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path)?;
        connection.busy_timeout(Duration::from_secs(10))?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        Ok(connection)
    }
    pub fn settings(&self) -> Result<Settings> {
        let body: String =
            self.connect()?
                .query_row("SELECT body FROM settings WHERE id=1", [], |r| r.get(0))?;
        Ok(serde_json::from_str(&body)?)
    }
    pub fn update_settings(&self, value: &Settings) -> Result<()> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE settings SET body=?1 WHERE id=1",
            [serde_json::to_string(value)?],
        )?;
        Self::audit(&tx, "settings", &digest(value)?)?;
        tx.commit()?;
        Ok(())
    }
    pub fn draws(&self) -> Result<Vec<Draw>> {
        Self::read_draws(&self.connect()?)
    }
    pub(crate) fn read_draws(connection: &Connection) -> Result<Vec<Draw>> {
        let mut statement = connection.prepare("SELECT body FROM draws ORDER BY issue")?;
        let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
    }
    /// Validate and merge the entire batch in one transaction. Conflicting
    /// numbers/dates and conflicting published awards are never overwritten.
    pub fn import(&self, records: &[Draw]) -> Result<usize> {
        let records = validate_history(records)?;
        ensure!(!records.is_empty(), "导入数据不能为空");
        let mut connection = self.connect()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut merged: BTreeMap<_, _> = Self::read_draws(&tx)?
            .into_iter()
            .map(|d| (d.issue.clone(), d))
            .collect();
        let mut changed = 0;
        for draw in records {
            let result = match merged.get(&draw.issue) {
                None => draw,
                Some(existing) => {
                    ensure!(
                        existing.date == draw.date
                            && existing.front == draw.front
                            && existing.back == draw.back,
                        "{} 期开奖与缓存冲突，整批导入已拒绝",
                        draw.issue
                    );
                    let mut result = existing.clone();
                    for (grade, award) in &draw.awards {
                        let old = result.awards.entry(*grade).or_default();
                        ensure!(
                            old.basic_fen.is_none()
                                || award.basic_fen.is_none()
                                || old.basic_fen == award.basic_fen,
                            "{} 期基本奖金冲突",
                            draw.issue
                        );
                        ensure!(
                            old.additional_fen.is_none()
                                || award.additional_fen.is_none()
                                || old.additional_fen == award.additional_fen,
                            "{} 期追加奖金冲突",
                            draw.issue
                        );
                        old.basic_fen = old.basic_fen.or(award.basic_fen);
                        old.additional_fen = old.additional_fen.or(award.additional_fen);
                    }
                    ensure!(
                        existing.pool_before_fen.is_none()
                            || draw.pool_before_fen.is_none()
                            || existing.pool_before_fen == draw.pool_before_fen,
                        "{} 期开奖前奖池冲突",
                        draw.issue
                    );
                    result.pool_before_fen = result.pool_before_fen.or(draw.pool_before_fen);
                    if result != *existing {
                        result.source = draw.source.clone();
                    }
                    result
                }
            };
            if merged.get(&result.issue) != Some(&result) {
                tx.execute("INSERT INTO draws(issue,body) VALUES(?1,?2) ON CONFLICT(issue) DO UPDATE SET body=excluded.body", params![result.issue,serde_json::to_string(&result)?])?;
                Self::audit(&tx, "draw", &digest(&result)?)?;
                changed += 1;
            }
            merged.insert(result.issue.clone(), result);
        }
        validate_history(&merged.into_values().collect::<Vec<_>>())?;
        tx.commit()?;
        Ok(changed)
    }
    pub fn generate(&self, request: &GenerateRequest) -> Result<Plan> {
        // Both settings and draws are read from the same SQLite snapshot.
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let body: String =
            tx.query_row("SELECT body FROM settings WHERE id=1", [], |r| r.get(0))?;
        let settings: Settings = serde_json::from_str(&body)?;
        let draws = Self::read_draws(&tx)?;
        ensure!(
            draws.last().is_none_or(|d| request.issue > d.issue),
            "目标期号须晚于最新已知开奖，历史回放请使用回测"
        );
        generate(request, settings.seed, &draws)
    }
    /// Explicit maintenance only: replace awards from fresh official data after
    /// verifying every cached date and number. Normal imports still reject
    /// conflicts. Each applied correction links old/new digests in the audit.
    pub fn repair_official_awards(
        &self,
        records: &[Draw],
        apply: bool,
    ) -> Result<serde_json::Value> {
        let records = validate_history(records)?;
        ensure!(!records.is_empty(), "官方数据不能为空");
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: BTreeMap<_, _> = Self::read_draws(&tx)?
            .into_iter()
            .map(|d| (d.issue.clone(), d))
            .collect();
        let mut corrections = Vec::new();
        for record in &records {
            ensure!(
                record.source == crate::fetch::OFFICIAL_URL,
                "修复只接受重新获取的体彩官方数据"
            );
            let Some(old) = existing.get(&record.issue) else {
                continue;
            };
            ensure!(
                old.source == crate::fetch::OFFICIAL_URL,
                "{} 期非官方导入记录须人工核对",
                record.issue
            );
            ensure!(
                old.date == record.date && old.front == record.front && old.back == record.back,
                "{} 期号码或日期冲突，整次修复已拒绝",
                record.issue
            );
            ensure!(
                !record.awards.is_empty() || old.awards.is_empty(),
                "{} 期官方缺少奖金，拒绝清空缓存",
                record.issue
            );
            if old.awards == record.awards {
                continue;
            }
            let new = Draw {
                awards: record.awards.clone(),
                ..old.clone()
            };
            let old_hash = digest(old)?;
            let new_hash = digest(&new)?;
            if apply {
                tx.execute(
                    "UPDATE draws SET body=?1 WHERE issue=?2",
                    params![serde_json::to_string(&new)?, record.issue],
                )?;
                Self::audit(
                    &tx,
                    "official_awards_correction",
                    &digest(&(&record.issue, &old_hash, &new_hash))?,
                )?;
            }
            corrections.push(serde_json::json!({"issue":record.issue,"old_awards":old.awards,"new_awards":new.awards,"old_content_hash":old_hash,"new_content_hash":new_hash}));
        }
        if apply {
            tx.commit()?;
        }
        Ok(
            serde_json::json!({"applied":apply,"fetched":records.len(),"changed":corrections.len(),"corrections":corrections}),
        )
    }
    pub fn save(&self, input: &SaveRequest, kind: &str) -> Result<SavedPlan> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let saved = Self::save_in(&tx, input, kind)?;
        tx.commit()?;
        Ok(saved)
    }
    fn save_in(tx: &Connection, input: &SaveRequest, kind: &str) -> Result<SavedPlan> {
        ensure!(input.label.chars().count() <= 80, "方案名称最多 80 字");
        let deadline: DateTime<Utc> = DateTime::parse_from_rfc3339(&input.deadline)
            .context("保存须提供带时区的开奖前截止时间")?
            .with_timezone(&Utc);
        let existing: Option<(String, String)> = tx
            .query_row(
                "SELECT body,content_hash FROM plans WHERE id=?1",
                [&input.preview_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((body, hash)) = existing {
            let saved: SavedPlan = serde_json::from_str(&body)?;
            ensure!(digest(&saved)? == hash, "归档摘要校验失败");
            Self::read_selection_evidence(tx, &saved.plan)?;
            return Ok(saved);
        }
        Self::check_schedule(tx, &input.request.issue, deadline)?;
        ensure!(deadline > Utc::now(), "已到截止时间，不能补造开奖前记录");
        ensure!(
            deadline <= Utc::now() + chrono::Duration::days(30),
            "截止时间不能超过未来 30 天"
        );
        let draws = Self::read_draws(tx)?;
        ensure!(
            draws.last().is_none_or(|d| input.request.issue > d.issue),
            "目标期已开奖或不晚于数据截止期，不能保存"
        );
        let body: String =
            tx.query_row("SELECT body FROM settings WHERE id=1", [], |r| r.get(0))?;
        let settings: Settings = serde_json::from_str(&body)?;
        let (plan, evidence) =
            crate::plan::generate_with_evidence(&input.request, settings.seed, &draws)?;
        ensure!(
            plan.id == input.preview_id,
            "参数、种子或开奖数据已变化，请重新生成后保存"
        );
        let saved = SavedPlan {
            id: plan.id.clone(),
            label: input.label.clone(),
            kind: kind.into(),
            created_at: Utc::now().to_rfc3339(),
            deadline: deadline.to_rfc3339(),
            plan,
            settlement: None,
            integrity_valid: true,
            schedule: Self::read_schedules(tx)?
                .into_iter()
                .find(|s| s.issue == input.request.issue),
        };
        let hash = digest(&saved)?;
        tx.execute(
            "INSERT INTO plans VALUES(?1,?2,?3,?4,?5)",
            params![
                saved.id,
                saved.plan.request.issue,
                saved.kind,
                serde_json::to_string(&saved)?,
                hash
            ],
        )?;
        Self::audit(tx, "save", &hash)?;
        let evidence_hash = digest(&evidence)?;
        tx.execute(
            "INSERT INTO selection_evidence VALUES(?1,?2,?3)",
            params![saved.id, serde_json::to_string(&evidence)?, evidence_hash],
        )?;
        Self::audit(tx, "selection_evidence", &evidence_hash)?;
        Ok(saved)
    }
    /// Freeze all presets under one transaction and one settings/data snapshot.
    pub fn archive_presets(&self, issue: &str, deadline: &str) -> Result<Vec<SavedPlan>> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        // A period stays frozen even if settings or history have changed.
        if let Some(batch) = Self::read_batches(&tx)?
            .into_iter()
            .find(|b| b.issue == issue)
        {
            return Self::batch_records(&batch, &Self::read_records(&tx)?);
        }
        let deadline_time = DateTime::parse_from_rfc3339(deadline)?.with_timezone(&Utc);
        ensure!(
            deadline_time > Utc::now() && deadline_time <= Utc::now() + chrono::Duration::days(30),
            "不能在截止后补造整期推荐清单"
        );
        Self::check_schedule(&tx, issue, deadline_time)?;
        let body: String =
            tx.query_row("SELECT body FROM settings WHERE id=1", [], |r| r.get(0))?;
        let settings: Settings = serde_json::from_str(&body)?;
        let draws = Self::read_draws(&tx)?;
        ensure!(
            draws.last().is_none_or(|d| issue > d.issue.as_str()),
            "目标期已开奖，不能补造推荐清单"
        );
        let mut result = Vec::new();
        for (name, request) in presets(issue) {
            let plan = generate(&request, settings.seed, &draws)?;
            result.push(Self::save_in(
                &tx,
                &SaveRequest {
                    request,
                    preview_id: plan.id,
                    deadline: deadline.into(),
                    label: name.into(),
                },
                "recommendation",
            )?);
        }
        let batch = RecommendationBatch {
            issue: issue.into(),
            created_at: Utc::now().to_rfc3339(),
            deadline: deadline.into(),
            entries: presets(issue)
                .iter()
                .zip(&result)
                .map(|((label, _), r)| RecommendationEntry {
                    label: (*label).into(),
                    plan_id: r.id.clone(),
                })
                .collect(),
        };
        let hash = digest(&batch)?;
        tx.execute(
            "INSERT INTO recommendation_batches VALUES(?1,?2,?3)",
            params![issue, serde_json::to_string(&batch)?, hash],
        )?;
        Self::audit(&tx, "recommendation_batch", &hash)?;
        tx.commit()?;
        Ok(result)
    }
    pub(crate) fn read_batches(conn: &Connection) -> Result<Vec<RecommendationBatch>> {
        let mut statement = conn
            .prepare("SELECT body,content_hash FROM recommendation_batches ORDER BY issue DESC")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|row| {
            let (body, hash) = row?;
            let batch: RecommendationBatch = serde_json::from_str(&body)?;
            ensure!(
                digest(&batch)? == hash,
                "推荐归档 {} 摘要校验失败",
                batch.issue
            );
            Ok(batch)
        })
        .collect()
    }
    fn batch_records(batch: &RecommendationBatch, records: &[SavedPlan]) -> Result<Vec<SavedPlan>> {
        batch
            .entries
            .iter()
            .map(|entry| {
                let record = records
                    .iter()
                    .find(|r| r.id == entry.plan_id)
                    .context("推荐归档缺少原始方案")?;
                ensure!(
                    record.plan.request.issue == batch.issue,
                    "推荐归档期号与原始方案不符"
                );
                Ok(record.clone())
            })
            .collect()
    }
    pub fn recommendations(&self) -> Result<serde_json::Value> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        Self::read_recommendations(&tx)
    }
    fn read_recommendations(tx: &Connection) -> Result<serde_json::Value> {
        let batches = Self::read_batches(tx)?;
        let records = Self::read_records(tx)?;
        for batch in &batches {
            Self::batch_records(batch, &records)?;
        }
        let records: Vec<_> = records
            .into_iter()
            .filter(|r| {
                r.kind == "recommendation"
                    || batches
                        .iter()
                        .any(|b| b.entries.iter().any(|e| e.plan_id == r.id))
            })
            .collect();
        Ok(serde_json::json!({"batches": batches, "records": records}))
    }
    pub fn records(&self) -> Result<Vec<SavedPlan>> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        Self::read_records(&tx)
    }
    pub(crate) fn read_records(tx: &Connection) -> Result<Vec<SavedPlan>> {
        let draws: BTreeMap<_, _> = Self::read_draws(tx)?
            .into_iter()
            .map(|d| (d.issue.clone(), d))
            .collect();
        let mut statement =
            tx.prepare("SELECT body,content_hash FROM plans ORDER BY issue DESC,rowid DESC")?;
        let rows =
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|row| {
            let (body, hash) = row?;
            let mut saved: SavedPlan = serde_json::from_str(&body)?;
            ensure!(digest(&saved)? == hash, "归档 {} 摘要校验失败", saved.id);
            if let Some(draw) = draws.get(&saved.plan.request.issue) {
                saved.settlement = Some(settle(
                    &saved.plan.tickets,
                    saved.plan.request.additional,
                    saved.plan.request.multiplier,
                    draw,
                )?);
            }
            Ok(saved)
        })
        .collect()
    }
    pub(crate) fn audit(tx: &Connection, action: &str, object_id: &str) -> Result<()> {
        let previous: String = tx
            .query_row(
                "SELECT hash FROM audit ORDER BY seq DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default();
        let time = Utc::now().to_rfc3339();
        let hash = digest(&(&time, action, object_id, &previous))?;
        tx.execute(
            "INSERT INTO audit(time,action,object_id,previous_hash,hash) VALUES(?1,?2,?3,?4,?5)",
            params![time, action, object_id, previous, hash],
        )?;
        Ok(())
    }
    pub fn verify_audit(&self) -> Result<usize> {
        Self::verify_audit_in(&self.connect()?)
    }
    fn verify_audit_in(conn: &Connection) -> Result<usize> {
        let mut statement = conn
            .prepare("SELECT time,action,object_id,previous_hash,hash FROM audit ORDER BY seq")?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut previous = String::new();
        let mut count = 0;
        for row in rows {
            let (time, action, object, prev, hash) = row?;
            ensure!(
                prev == previous && digest(&(&time, &action, &object, &prev))? == hash,
                "审计链第 {} 条校验失败",
                count + 1
            );
            previous = hash;
            count += 1;
        }
        Ok(count)
    }
    pub fn export(&self) -> Result<serde_json::Value> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let settings: String =
            tx.query_row("SELECT body FROM settings WHERE id=1", [], |r| r.get(0))?;
        let records = Self::read_records(&tx)?;
        Ok(
            serde_json::json!({"schema_version":4,"exported_at":Utc::now().to_rfc3339(),
            "selection_evidence":Self::export_selection_evidence(&tx,&records)?,
            "settings":serde_json::from_str::<Settings>(&settings)?,"draws":Self::read_draws(&tx)?,
            "records":records,"recommendations":Self::read_recommendations(&tx)?,
            "schedules":Self::read_schedules(&tx)?,"audit_entries_verified":Self::verify_audit_in(&tx)?}),
        )
    }
}
