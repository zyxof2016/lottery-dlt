use crate::{
    analysis::{self, BacktestRequest},
    fetch,
    model::{Draw, Settings},
    plan::GenerateRequest,
    prizes,
    store::{SaveRequest, Store},
};
use anyhow::{ensure, Result};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct App {
    pub store: Store,
    token: Option<Arc<String>>,
    pub base: String,
    jobs: Arc<Semaphore>,
}
impl App {
    pub fn new(store: Store, token: Option<String>, base: String) -> Result<Self> {
        ensure!(
            base.is_empty()
                || (base.starts_with('/')
                    && !base.ends_with('/')
                    && base
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'/' | b'-' | b'_'))),
            "代理前缀须形如 /dlt，不能以斜杠结尾"
        );
        ensure!(
            token.as_ref().is_none_or(|t| t.len() >= 16),
            "管理员令牌至少16个字符"
        );
        Ok(Self {
            store,
            token: token.map(Arc::new),
            base,
            jobs: Arc::new(Semaphore::new(1)),
        })
    }
}
pub struct ApiError(StatusCode, String);
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self(StatusCode::UNPROCESSABLE_ENTITY, e.to_string())
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
type ApiResult = std::result::Result<Json<Value>, ApiError>;

async fn blocking<F>(f: F) -> ApiResult
where
    F: FnOnce() -> Result<Value> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .map(Json)
        .map_err(Into::into)
}

async fn guard(State(app): State<App>, request: Request, next: Next) -> Response {
    let write = matches!(
        request.method().as_str(),
        "POST" | "PUT" | "PATCH" | "DELETE"
    );
    if write {
        if let Some(origin) = request.headers().get(header::ORIGIN) {
            let authority = origin
                .to_str()
                .ok()
                .and_then(|s| s.split_once("://"))
                .map(|(_, v)| v);
            let host = request
                .headers()
                .get(header::HOST)
                .and_then(|h| h.to_str().ok());
            if authority.is_none() || authority != host {
                return ApiError(StatusCode::FORBIDDEN, "跨站写入已拒绝".into()).into_response();
            }
        }
    }
    if write
        || request.uri().path().ends_with("/api/export")
        || request.uri().path().ends_with("/api/auth")
        || request.uri().path().contains("/api/jobs")
    {
        if let Some(token) = &app.token {
            let provided = request
                .headers()
                .get(header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.strip_prefix("Bearer "))
                .unwrap_or("");
            let a = Sha256::digest(token.as_bytes());
            let b = Sha256::digest(provided.as_bytes());
            let diff = a.iter().zip(b.iter()).fold(0u8, |v, (a, b)| v | (a ^ b));
            if diff != 0 {
                return ApiError(
                    StatusCode::UNAUTHORIZED,
                    "请在系统设置填写管理员令牌".into(),
                )
                .into_response();
            }
        }
    }
    let mut response = next.run(request).await;
    let h = response.headers_mut();
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    h.insert("content-security-policy",HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'none'"));
    response
}

async fn index(State(app): State<App>) -> Html<String> {
    Html(include_str!("../web/index.html").replace("__BASE_PATH__", &app.base))
}
async fn js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/app.js"),
    )
        .into_response()
}
async fn css() -> Response {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../web/styles.css"),
    )
        .into_response()
}
async fn pool_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/pools.js"),
    )
        .into_response()
}
async fn format_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/format.js"),
    )
        .into_response()
}
async fn usability_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/usability.js"),
    )
        .into_response()
}
async fn workflow_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/workflow.js"),
    )
        .into_response()
}
async fn transport_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/transport.js"),
    )
        .into_response()
}
async fn review_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/reviews-ui.js"),
    )
        .into_response()
}
async fn replacements_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/replacements-ui.js"),
    )
        .into_response()
}
async fn evidence_js() -> Response {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../web/evidence-ui.js"),
    )
        .into_response()
}
async fn health(State(app): State<App>) -> ApiResult {
    blocking(move || {
        app.store.settings()?;
        Ok(json!({"status":"ok","service":"lottery-dlt","version":env!("CARGO_PKG_VERSION")}))
    })
    .await
}
async fn overview(State(app): State<App>) -> ApiResult {
    let protected = app.token.is_some();
    blocking(move || {
        let draws = app.store.draws()?; let records = app.store.records()?;
        let latest = draws.last(); let mut missing = Vec::new();
        for pair in draws.windows(2) {
            if pair[0].issue[..2] == pair[1].issue[..2] {
                for issue in pair[0].issue.parse::<u32>()?+1..pair[1].issue.parse::<u32>()? { missing.push(format!("{issue:05}")); }
            }
        }
        let age = latest.map(|d|(chrono::Utc::now().date_naive()-chrono::NaiveDate::parse_from_str(&d.date,"%Y-%m-%d").unwrap()).num_days());
        let settled: Vec<_> = records.iter().filter_map(|r|r.settlement.as_ref()).collect();
        let total_prize = (!settled.is_empty()).then(||settled.iter().map(|r|r.total_prize_fen).collect::<Option<Vec<_>>>().map(|v|v.iter().sum::<u64>())).flatten();
        let cost: u64 = settled.iter().map(|s|s.cost_fen).sum();
        Ok(json!({"draw_count":draws.len(),"latest_draw":latest,"record_count":records.len(),"settled_count":settled.len(),
            "pending_count":records.len()-settled.len(),"settled_cost_fen":cost,"settled_prize_fen":total_prize,
            "settled_net_fen":total_prize.map(|v|v as i64-cost as i64),"winning_record_count":settled.iter().filter(|s|s.winning_tickets>0).count(),
            "known_prize_fen":settled.iter().map(|s|s.known_prize_fen).sum::<u64>(),
            "data_quality":{"age_days":age,"stale":age.is_none_or(|d|d>4),"missing_issues":missing},
            "admin_token_required":protected,"rules":prizes::theory(),"note":"下一期期号请核对官方公告；网页不会自动生成或保存方案。"}))
    }).await
}
#[derive(Deserialize, Default)]
struct Page {
    page: Option<usize>,
    limit: Option<usize>,
    issue: Option<String>,
    kind: Option<String>,
    status: Option<String>,
}
async fn draws(State(app): State<App>, Query(page): Query<Page>) -> ApiResult {
    if page.page.is_some() {
        return blocking(move || {
            app.store.draw_page(
                page.page.unwrap_or(1),
                page.limit.unwrap_or(50),
                page.issue.as_deref().unwrap_or(""),
            )
        })
        .await;
    }
    blocking(move || {
        let mut draws = app.store.draws()?;
        draws.reverse();
        Ok(json!({"draws":draws}))
    })
    .await
}
async fn records(State(app): State<App>, Query(page): Query<Page>) -> ApiResult {
    if page.page.is_some() {
        return blocking(move || {
            app.store.record_page_with_status(
                page.page.unwrap_or(1),
                page.limit.unwrap_or(20),
                page.issue.as_deref().unwrap_or(""),
                page.kind.as_deref(),
                page.status.as_deref(),
            )
        })
        .await;
    }
    blocking(move || Ok(json!({"records":app.store.records()?}))).await
}
async fn recommendations(State(app): State<App>, Query(page): Query<Page>) -> ApiResult {
    if page.page.is_some() {
        return blocking(move || {
            app.store.recommendation_page(
                page.page.unwrap_or(1),
                page.limit.unwrap_or(5),
                page.issue.as_deref().unwrap_or(""),
            )
        })
        .await;
    }
    blocking(move || app.store.recommendations()).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewWindow {
    limit: Option<usize>,
}
async fn live_review(State(app): State<App>, Query(query): Query<ReviewWindow>) -> ApiResult {
    blocking(move || app.store.live_review(query.limit.unwrap_or(30)))
        .await
        .map_err(|e| {
            if e.1.contains("校验") || e.1.contains("清单") || e.1.contains("身份") {
                ApiError(StatusCode::CONFLICT, e.1)
            } else {
                e
            }
        })
}
async fn generate(State(app): State<App>, Json(input): Json<GenerateRequest>) -> ApiResult {
    blocking(move || Ok(serde_json::to_value(app.store.generate(&input)?)?)).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExplainInput {
    request: GenerateRequest,
    preview_id: String,
}
async fn explain(State(app): State<App>, Json(input): Json<ExplainInput>) -> ApiResult {
    blocking(move || {
        app.store
            .preview_evidence(&input.request, &input.preview_id)
    })
    .await
}
async fn selection_evidence(State(app): State<App>, Path(id): Path<String>) -> ApiResult {
    blocking(move || app.store.selection_evidence(&id))
        .await
        .map_err(|e| {
            if e.1.contains("校验") {
                ApiError(StatusCode::CONFLICT, e.1)
            } else if e.1.contains("不存在") {
                ApiError(StatusCode::NOT_FOUND, e.1)
            } else {
                e
            }
        })
}
async fn save(State(app): State<App>, Json(input): Json<SaveRequest>) -> ApiResult {
    blocking(move || Ok(serde_json::to_value(app.store.save(&input, "personal")?)?)).await
}
async fn archive(State(app): State<App>, Json(input): Json<SaveRequest>) -> ApiResult {
    blocking(move || {
        Ok(serde_json::to_value(
            app.store.save(&input, "recommendation")?,
        )?)
    })
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArchiveAll {
    issue: String,
    deadline: String,
}
async fn archive_all(State(app): State<App>, Json(input): Json<ArchiveAll>) -> ApiResult {
    blocking(move || {
        Ok(json!({"records":app.store.archive_presets(&input.issue,&input.deadline)?}))
    })
    .await
}
async fn import(State(app): State<App>, Json(input): Json<Value>) -> ApiResult {
    blocking(move || {
        let draws = if input.pointer("/value/list").is_some() {
            fetch::parse_official(&input)?
        } else {
            serde_json::from_value::<Vec<Draw>>(input.get("draws").cloned().unwrap_or(input))?
        };
        Ok(json!({"changed":app.store.import(&draws)?}))
    })
    .await
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Sync {
    pages: usize,
}
impl Default for Sync {
    fn default() -> Self {
        Self { pages: 1 }
    }
}
async fn sync(State(app): State<App>, Json(input): Json<Sync>) -> ApiResult {
    let _permit = app.jobs.clone().try_acquire_owned().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "已有同步或计算任务，请稍后再试".into(),
        )
    })?;
    let records = fetch::official(input.pages).await?;
    let changed = app.store.import(&records)?;
    let schedule = match crate::operations::fetch_schedule(&app.store.draws()?).await {
        Ok(s) => {
            app.store.set_schedule(&s)?;
            serde_json::to_value(s).map_err(anyhow::Error::from)?
        }
        Err(e) => json!({"error":e.to_string()}),
    };
    app.store.set_operation("last_sync", &json!({"at":chrono::Utc::now().to_rfc3339(),"fetched":records.len(),"changed":changed,"schedule":schedule}))?;
    Ok(Json(
        json!({"fetched":records.len(),"changed":changed,"schedule":schedule}),
    ))
}
async fn config(State(app): State<App>) -> ApiResult {
    blocking(move || Ok(serde_json::to_value(app.store.settings()?)?)).await
}
async fn update_config(State(app): State<App>, Json(input): Json<Settings>) -> ApiResult {
    blocking(move || {
        app.store.update_settings(&input)?;
        Ok(serde_json::to_value(input)?)
    })
    .await
}
#[derive(Deserialize)]
struct Window {
    window: usize,
}
async fn stats(
    State(app): State<App>,
    axum::extract::Query(input): axum::extract::Query<Window>,
) -> ApiResult {
    blocking(move || analysis::statistics(&app.store.draws()?, input.window)).await
}
async fn backtest(State(app): State<App>, Json(input): Json<BacktestRequest>) -> ApiResult {
    let permit = app.jobs.clone().try_acquire_owned().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "已有计算任务，请稍后再试".into(),
        )
    })?;
    blocking(move || {
        let _permit = permit;
        analysis::backtest(&app.store.draws()?, &input)
    })
    .await
}
async fn research(State(app): State<App>, Json(input): Json<BacktestRequest>) -> ApiResult {
    let permit = app.jobs.clone().try_acquire_owned().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "已有计算任务，请稍后再试".into(),
        )
    })?;
    blocking(move || {
        let _permit = permit;
        analysis::research(&app.store.draws()?, &input)
    })
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Simulation {
    request: GenerateRequest,
    preview_id: String,
    trials: usize,
}
async fn simulate(State(app): State<App>, Json(input): Json<Simulation>) -> ApiResult {
    let permit = app.jobs.clone().try_acquire_owned().map_err(|_| {
        ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "已有计算任务，请稍后再试".into(),
        )
    })?;
    blocking(move || {
        let _permit = permit;
        let plan = app.store.generate(&input.request)?;
        ensure!(
            plan.id == input.preview_id,
            "预览已失效，请重新生成后模拟；不会模拟另一组号码"
        );
        analysis::simulate(&plan, input.trials)
    })
    .await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobSubmission {
    kind: String,
    input: Value,
}
fn start_job(app: App, id: String) {
    tokio::spawn(async move {
        if let Ok(permit) = app.jobs.clone().acquire_owned().await {
            let _ = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let result = app.store.run_job(&id);
                let _ = app.store.prune_jobs();
                result
            })
            .await;
        }
    });
}
async fn jobs(State(app): State<App>) -> ApiResult {
    blocking(move || Ok(json!({"jobs":app.store.jobs()?}))).await
}
async fn submit_job(
    State(app): State<App>,
    Json(input): Json<JobSubmission>,
) -> Result<impl IntoResponse, ApiError> {
    let job = app.store.submit_job(&input.kind, input.input)?;
    let summary = crate::jobs::job_summary(&job);
    start_job(app, job.id);
    Ok((StatusCode::ACCEPTED, Json(summary)))
}
async fn job(State(app): State<App>, Path(id): Path<String>) -> ApiResult {
    blocking(move || Ok(serde_json::to_value(app.store.job(&id)?)?)).await
}
async fn cancel_job(State(app): State<App>, Path(id): Path<String>) -> ApiResult {
    blocking(move || Ok(serde_json::to_value(app.store.cancel_job(&id)?)?)).await
}
async fn schedules(State(app): State<App>) -> ApiResult {
    blocking(move || Ok(json!({"schedules":app.store.schedules()?}))).await
}
async fn sync_schedule(State(app): State<App>) -> ApiResult {
    let schedule = crate::operations::fetch_schedule(&app.store.draws()?).await?;
    app.store.set_schedule(&schedule)?;
    Ok(Json(
        serde_json::to_value(schedule).map_err(anyhow::Error::from)?,
    ))
}
async fn operations(State(app): State<App>) -> ApiResult {
    blocking(move || Ok(json!({"automation":app.store.automation_config()?,"events":app.store.automation_events()?,"last_sync":app.store.operation("last_sync")?,"last_backup":app.store.operation("last_backup")?,"strict_schedule":app.store.operation("strict_schedule")?.unwrap_or(json!(false))}))).await
}
async fn configure_automation(
    State(app): State<App>,
    Json(input): Json<crate::operations::Automation>,
) -> ApiResult {
    blocking(move || {
        app.store.configure_automation(&input)?;
        Ok(serde_json::to_value(input)?)
    })
    .await
}
async fn run_automation(State(app): State<App>) -> ApiResult {
    blocking(move || app.store.automate()).await
}
async fn backup(State(app): State<App>) -> ApiResult {
    blocking(move || {
        let folder = std::env::var_os("LOTTERY_BACKUP_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                app.store
                    .path
                    .parent()
                    .unwrap_or(std::path::Path::new("."))
                    .join("backups")
            });
        std::fs::create_dir_all(&folder)?;
        app.store.verified_backup(&folder.join(format!(
            "dlt-{}-{}.sqlite3",
            chrono::Utc::now().format("%Y%m%d-%H%M%S"),
            rand::random::<u32>()
        )))
    })
    .await
}
async fn export(State(app): State<App>) -> ApiResult {
    blocking(move || app.store.export()).await
}
async fn theory() -> Json<Value> {
    Json(prizes::theory())
}
async fn not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({"error":"接口不存在"})))
}

pub fn router(app: App) -> Router {
    let base = app.base.clone();
    let routes = Router::new()
        .route("/", get(index))
        .route("/reviews", get(index))
        .route("/app.js", get(js))
        .route("/pools.js", get(pool_js))
        .route("/format.js", get(format_js))
        .route("/usability.js", get(usability_js))
        .route("/workflow.js", get(workflow_js))
        .route("/transport.js", get(transport_js))
        .route("/reviews-ui.js", get(review_js))
        .route("/replacements-ui.js", get(replacements_js))
        .route("/evidence-ui.js", get(evidence_js))
        .route("/styles.css", get(css))
        .route("/favicon.ico", get(|| async { StatusCode::NO_CONTENT }))
        .route("/healthz", get(health))
        .route("/api/overview", get(overview))
        .route("/api/theory", get(theory))
        .route("/api/draws", get(draws))
        .route("/api/draws/import", post(import))
        .route("/api/draws/sync", post(sync))
        .route("/api/plans/generate", post(generate))
        .route("/api/plans/explain", post(explain))
        .route("/api/plans/simulate", post(simulate))
        .route("/api/records", get(records).post(save))
        .route("/api/records/{id}/evidence", get(selection_evidence))
        .route("/api/recommendations", get(recommendations).post(archive))
        .route("/api/recommendations/live-review", get(live_review))
        .route("/api/recommendations/archive", post(archive_all))
        .route("/api/config", get(config).put(update_config))
        .route(
            "/api/auth",
            get(|| async { Json(json!({"authenticated":true})) }),
        )
        .route("/api/statistics", get(stats))
        .route("/api/backtest", post(backtest))
        .route("/api/research", post(research))
        .route("/api/jobs", get(jobs).post(submit_job))
        .route("/api/jobs/{id}", get(job))
        .route("/api/jobs/{id}/cancel", post(cancel_job))
        .route("/api/schedules", get(schedules))
        .route("/api/schedules/sync", post(sync_schedule))
        .route("/api/operations", get(operations))
        .route(
            "/api/automation",
            post(run_automation).put(configure_automation),
        )
        .route("/api/backups", post(backup))
        .route("/api/export", get(export))
        .fallback(not_found);
    let routes = if base.is_empty() {
        routes
    } else {
        Router::new()
            .route(&format!("{base}/"), get(index))
            .nest(&base, routes)
    };
    routes
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}

pub async fn serve(app: App, address: SocketAddr) -> Result<()> {
    ensure!(
        address.ip().is_loopback() || app.token.is_some(),
        "对外监听须设置 LOTTERY_ADMIN_TOKEN（至少16字符）"
    );
    for id in app.store.recover_jobs()? {
        start_job(app.clone(), id);
    }
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("体彩大乐透: http://{}{}/", listener.local_addr()?, app.base);
    axum::serve(listener, router(app))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
