use crate::{analysis, digest, model::Draw, optimizer, research, store::Store};
use anyhow::{ensure, Context, Result};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub progress: f64,
    pub cancel_requested: bool,
    pub input: Value,
    pub history_hash: String,
    #[serde(with = "crate::model::seed_text")]
    pub seed: u64,
    pub generator_version: String,
    pub result: Option<Value>,
    pub error: Option<String>,
}
impl Store {
    pub fn submit_job(&self, kind: &str, input: Value) -> Result<Job> {
        ensure!(
            ["backtest", "research", "optimize"].contains(&kind),
            "未知计算任务"
        );
        // Deserialize before enqueuing, so typos never produce a phantom success.
        match kind {
            "backtest" => {
                serde_json::from_value::<analysis::BacktestRequest>(input.clone())?;
            }
            "research" => {
                serde_json::from_value::<research::ResearchRequest>(input.clone())?;
            }
            _ => {
                serde_json::from_value::<optimizer::OptimizeRequest>(input.clone())?;
            }
        }
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM jobs WHERE state IN ('queued','running')",
            [],
            |r| r.get(0),
        )?;
        ensure!(pending < 8, "队列已满，最多8个未完成任务");
        let history = Self::read_draws(&tx)?;
        let settings: String =
            tx.query_row("SELECT body FROM settings WHERE id=1", [], |r| r.get(0))?;
        let seed = serde_json::from_str::<crate::model::Settings>(&settings)?.seed;
        let now = Utc::now().to_rfc3339();
        let id = digest(&(kind, &input, &now, rand::random::<u64>()))?;
        let job = Job {
            id,
            kind: kind.into(),
            state: "queued".into(),
            created_at: now.clone(),
            updated_at: now,
            progress: 0.,
            cancel_requested: false,
            input,
            history_hash: digest(&history)?,
            seed,
            generator_version: crate::plan::GENERATOR_VERSION.into(),
            result: None,
            error: None,
        };
        tx.execute(
            "INSERT INTO jobs VALUES(?1,?2,?3,?4)",
            params![
                job.id,
                job.state,
                serde_json::to_string(&job)?,
                serde_json::to_string(&history)?
            ],
        )?;
        tx.commit()?;
        Ok(job)
    }
    pub fn job(&self, id: &str) -> Result<Job> {
        let body: String = self
            .connect()?
            .query_row("SELECT body FROM jobs WHERE id=?1", [id], |r| r.get(0))
            .optional()?
            .context("任务不存在")?;
        Ok(serde_json::from_str(&body)?)
    }
    pub fn jobs(&self) -> Result<Vec<Value>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT body FROM jobs ORDER BY rowid DESC LIMIT 50")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.map(|r| {
            let mut j: Value = serde_json::from_str(&r?)?;
            j.as_object_mut().unwrap().remove("result");
            Ok(j)
        })
        .collect()
    }
    fn update_job(&self, id: &str, update: impl FnOnce(&mut Job) -> Result<()>) -> Result<Job> {
        let mut conn = self.connect()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let body: String = tx.query_row("SELECT body FROM jobs WHERE id=?1", [id], |r| r.get(0))?;
        let mut job: Job = serde_json::from_str(&body)?;
        update(&mut job)?;
        job.updated_at = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE jobs SET body=?1,state=?2 WHERE id=?3",
            params![serde_json::to_string(&job)?, job.state, id],
        )?;
        tx.commit()?;
        Ok(job)
    }
    pub fn cancel_job(&self, id: &str) -> Result<Job> {
        self.update_job(id, |j| {
            ensure!(
                ["queued", "running"].contains(&j.state.as_str()),
                "任务已经结束"
            );
            j.cancel_requested = true;
            if j.state == "queued" {
                j.state = "cancelled".into();
            }
            Ok(())
        })
    }
    pub fn recover_jobs(&self) -> Result<Vec<String>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT id FROM jobs WHERE state IN ('queued','running')")?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut queued = Vec::new();
        for id in ids {
            let j = self.job(&id)?;
            if j.state == "running" || j.generator_version != crate::plan::GENERATOR_VERSION {
                self.update_job(&id, |j| {
                    j.state = "interrupted".into();
                    j.error = Some("服务重启，运行中的任务已中断；可提交新任务".into());
                    Ok(())
                })?;
            } else {
                queued.push(id);
            }
        }
        Ok(queued)
    }
    pub fn run_job(&self, id: &str) -> Result<()> {
        let job = self.update_job(id, |j| {
            ensure!(
                j.state == "queued" && !j.cancel_requested,
                "任务已取消或已领取"
            );
            j.state = "running".into();
            Ok(())
        })?;
        let body: String =
            self.connect()?
                .query_row("SELECT history FROM jobs WHERE id=?1", [id], |r| r.get(0))?;
        let history: Vec<Draw> = serde_json::from_str(&body)?;
        let progress_clock =
            std::cell::Cell::new(std::time::Instant::now() - std::time::Duration::from_secs(1));
        let callback = |done: usize, total: usize| -> Result<()> {
            let j = self.job(id)?;
            ensure!(!j.cancel_requested, "任务已取消");
            let fraction = done as f64 / total.max(1) as f64;
            if (fraction - j.progress >= 0.01
                && progress_clock.get().elapsed() >= std::time::Duration::from_millis(250))
                || done == total
            {
                self.update_job(id, |j| {
                    ensure!(!j.cancel_requested, "任务已取消");
                    j.progress = j.progress.max(fraction).min(1.);
                    Ok(())
                })?;
                progress_clock.set(std::time::Instant::now());
            }
            Ok(())
        };
        let result = (|| {
            ensure!(
                digest(&history)? == job.history_hash,
                "任务历史快照摘要不符"
            );
            match job.kind.as_str() {
                "backtest" => analysis::backtest_with(
                    &history,
                    &serde_json::from_value(job.input.clone())?,
                    &callback,
                ),
                "research" => research::evaluate(
                    &history,
                    &serde_json::from_value(job.input.clone())?,
                    &callback,
                ),
                "optimize" => optimizer::optimize(
                    &history,
                    job.seed,
                    &serde_json::from_value(job.input.clone())?,
                    &callback,
                ),
                _ => anyhow::bail!("未知任务"),
            }
        })();
        self.update_job(id, |j| {
            if j.cancel_requested {
                j.state = "cancelled".into();
                j.error = Some("用户取消".into());
            } else {
                match result {
                    Ok(v) => {
                        j.state = "completed".into();
                        j.progress = 1.;
                        j.result = Some(v);
                    }
                    Err(e) => {
                        j.state = "failed".into();
                        j.error = Some(e.to_string());
                    }
                }
            }
            Ok(())
        })?;
        Ok(())
    }
    pub fn prune_jobs(&self) -> Result<usize> {
        Ok(self.connect()?.execute("DELETE FROM jobs WHERE state NOT IN ('running','queued') AND rowid NOT IN (SELECT rowid FROM jobs ORDER BY rowid DESC LIMIT 50)",[])?)
    }
}
pub fn job_summary(job: &Job) -> Value {
    json!({"id":job.id,"state":job.state,"kind":job.kind,"progress":job.progress})
}
