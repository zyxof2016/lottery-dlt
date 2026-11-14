use anyhow::{ensure, Context, Result};
use clap::{Parser, Subcommand};
use lottery_dlt::{
    analysis::{self, BacktestRequest},
    fetch,
    model::Settings,
    plan::GenerateRequest,
    prizes,
    store::{SaveRequest, Store},
    web,
};
use serde_json::{json, Value};
use std::{net::SocketAddr, path::PathBuf};

#[derive(Parser)]
#[command(
    name = "lottery-dlt",
    version,
    about = "体彩大乐透 · Rust 选号、对奖和滚动回测"
)]
struct Cli {
    #[arg(
        long,
        global = true,
        env = "LOTTERY_DB",
        default_value = "data/lottery-dlt.sqlite3"
    )]
    db: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// 启动网页（默认 http://127.0.0.1:6792）
    Serve {
        #[arg(long, env = "LOTTERY_BIND", default_value = "127.0.0.1:6792")]
        bind: SocketAddr,
        #[arg(long, env = "LOTTERY_BASE_PATH", default_value = "")]
        base_path: String,
    },
    /// 理论组合数、各命中形态概率、固定奖期望
    Theory,
    /// 同步官方开奖，每页100期
    Fetch {
        #[arg(long, default_value_t = 1)]
        pages: usize,
    },
    /// 重新获取官方奖金并核对差异，默认仅预览；修复不修改开奖号码
    RepairPrizes {
        #[arg(long, default_value_t = 50)]
        pages: usize,
        #[arg(long)]
        apply: bool,
    },
    /// 导入原生 JSON、官方响应 JSON 或 CSV；冲突整批拒绝
    Import { file: PathBuf },
    /// 用 JSON 参数生成方案；默认只预览
    Predict {
        #[arg(long)]
        issue: Option<String>,
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long)]
        save: bool,
        #[arg(long)]
        deadline: Option<String>,
        #[arg(long, default_value = "")]
        label: String,
    },
    /// 展示原始方案和逐注结算
    History,
    /// 按已缓存开奖复算原始票组
    Settle,
    /// 严格逐期历史回放，参数文件可选
    Backtest {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// 同期同预算比较五种策略（探索性）
    Research {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// 查看号码频次及遗漏
    Stats {
        #[arg(long, default_value_t = 60)]
        window: usize,
    },
    /// 查看或更新主种子（已保存方案保留原票面）
    Config {
        #[arg(long)]
        seed: Option<u64>,
    },
    /// 导出开奖、方案与摘要校验结果
    Export {
        #[arg(long)]
        out: PathBuf,
    },
    /// 同步开奖、核实官方日程并执行自动归档
    Operate {
        #[arg(long, default_value_t = 2)]
        pages: usize,
    },
    /// 核实并保存未来销售日程
    Schedule,
    /// 在线备份并验证恢复可读性，不覆盖已有文件
    Backup {
        #[arg(long)]
        out: PathBuf,
    },
    /// 从备份恢复到新路径，不覆盖正在运行的数据
    Restore {
        #[arg(long)]
        from: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// 核验记录摘要和审计链
    Verify,
    /// 用 HTTP 检查服务健康；用于容器
    Healthcheck {
        #[arg(long, default_value = "http://127.0.0.1:6792/healthz")]
        url: String,
    },
}
fn read_config<T: serde::de::DeserializeOwned + Default>(file: Option<PathBuf>) -> Result<T> {
    match file {
        Some(path) => Ok(serde_json::from_slice(&std::fs::read(path)?)?),
        None => Ok(T::default()),
    }
}
fn output(value: Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&value)?);
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    // Health checks and theory never create/open a database.
    if let Some(Command::Healthcheck { ref url }) = cli.command {
        let response = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(4))
            .build()?
            .get(url)
            .send()
            .await?
            .error_for_status()?;
        ensure!(
            response.json::<Value>().await?["status"] == "ok",
            "服务未就绪"
        );
        return Ok(());
    }
    if matches!(cli.command, Some(Command::Theory)) {
        return output(prizes::theory());
    }
    let store = Store::open(cli.db)?;
    let command = match cli.command {
        Some(command) => command,
        None => Command::Serve {
            bind: std::env::var("LOTTERY_BIND")
                .unwrap_or_else(|_| "127.0.0.1:6792".into())
                .parse()?,
            base_path: std::env::var("LOTTERY_BASE_PATH").unwrap_or_default(),
        },
    };
    match command {
        Command::Serve { bind, base_path } => {
            if std::env::var("LOTTERY_REQUIRE_SCHEDULE").as_deref() == Ok("1")
                && store.operation("strict_schedule")? != Some(json!(true))
            {
                store.strict_schedule(true)?;
            }
            web::serve(
                web::App::new(
                    store,
                    std::env::var("LOTTERY_ADMIN_TOKEN")
                        .ok()
                        .filter(|v| !v.is_empty()),
                    base_path,
                )?,
                bind,
            )
            .await
        }
        Command::Fetch { pages } => {
            let draws = fetch::official(pages).await?;
            output(json!({"fetched":draws.len(),"changed":store.import(&draws)?}))
        }
        Command::Schedule => {
            let schedule = lottery_dlt::operations::fetch_schedule(&store.draws()?).await?;
            store.set_schedule(&schedule)?;
            output(serde_json::to_value(schedule)?)
        }
        Command::Operate { pages } => {
            let result=async {
                let draws=fetch::official(pages).await?;let changed=store.import(&draws)?;
                let schedule=lottery_dlt::operations::fetch_schedule(&store.draws()?).await?;
                store.set_schedule(&schedule)?;
                let automation=store.automate()?;
                anyhow::Ok(json!({"at":chrono::Utc::now().to_rfc3339(),"changed":changed,"fetched":draws.len(),"schedule":schedule,"automation":automation}))
            }.await;
            match result {
                Ok(value) => {
                    store.set_operation("last_sync", &value)?;
                    output(value)
                }
                Err(e) => {
                    store.set_operation(
                        "last_sync",
                        &json!({"at":chrono::Utc::now().to_rfc3339(),"error":e.to_string()}),
                    )?;
                    let _ = store.automate();
                    Err(e)
                }
            }
        }
        Command::Backup { out } => output(store.verified_backup(&out)?),
        Command::Restore { from, out } => output(Store::restore(&from, &out)?),
        Command::RepairPrizes { pages, apply } => {
            let draws = fetch::official(pages).await?;
            output(store.repair_official_awards(&draws, apply)?)
        }
        Command::Import { file } => output(json!({"changed":store.import(&fetch::load(&file)?)?})),
        Command::Predict {
            issue,
            config,
            save,
            deadline,
            label,
        } => {
            let mut request: GenerateRequest = read_config(config)?;
            if let Some(issue) = issue {
                request.issue = issue;
            }
            let plan = store.generate(&request)?;
            if save {
                output(serde_json::to_value(store.save(&SaveRequest {request,preview_id:plan.id,deadline:deadline.context("--save 须提供 --deadline，例如 2026-10-17T20:00:00+08:00；请核对官方截止时间")?,label},"personal")?)?)
            } else {
                output(serde_json::to_value(plan)?)
            }
        }
        Command::History | Command::Settle => output(json!({"records":store.records()?})),
        Command::Backtest { config } => output(analysis::backtest(
            &store.draws()?,
            &read_config::<BacktestRequest>(config)?,
        )?),
        Command::Research { config } => output(analysis::research(
            &store.draws()?,
            &read_config::<BacktestRequest>(config)?,
        )?),
        Command::Stats { window } => output(analysis::statistics(&store.draws()?, window)?),
        Command::Config { seed } => {
            if let Some(seed) = seed {
                store.update_settings(&Settings { seed })?;
            }
            output(serde_json::to_value(store.settings()?)?)
        }
        Command::Export { out } => {
            // create_new refuses to overwrite an existing export.
            use std::io::Write;
            let content = serde_json::to_vec_pretty(&store.export()?)?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&out)?;
            file.write_all(&content)?;
            file.sync_all()?;
            output(json!({"exported":out}))
        }
        Command::Verify => output(
            json!({"records_verified":store.records()?.len(),"recommendation_batches_verified":store.recommendations()?["batches"].as_array().map(Vec::len),"audit_entries_verified":store.verify_audit()?,"note":"本机摘要用于完整性核验；独立可信的开奖前时间证明需外部存证。"}),
        ),
        Command::Theory | Command::Healthcheck { .. } => unreachable!(),
    }
}
