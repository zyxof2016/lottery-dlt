# 体彩大乐透 · Rust

参考 `lottery-k8` 的选号、跟踪、复盘和研究流程，按体彩大乐透的双区规则重新实现。后端、命令行、官方数据请求、组合展开、逐注对奖和回测全部使用 Rust；网页资源编入二进制，无需 Python、Node 或外部 CDN。

当前版本1.10.0以号码池与排序为首页：前区和后区分开按热号、冷号、遗漏、跟随、避开、分区和均匀对照排序，并列出本期冻结号码池。详见 [号码池与排序](docs/number-pools-1.10.0.md)。1.9.0的组票目标核对仍保留在接口里，见 [1.9.0说明](docs/structure-audit-1.9.0.md)。1.8.0把单注距头奖的差距拆成池外不足和未集中到同一注，并将浮动奖回本与只靠固定奖补足成本分开；金额未知时不把回撤或回本记为0，见 [1.8.0说明](docs/review-optimization-1.8.0.md)。读取恢复见 [1.7.1说明](docs/review-optimization-1.7.1.md)，同预算换号及来源重叠见 [1.7.0说明](docs/review-optimization-1.7.0.md)，草稿保留见 [1.6.0说明](docs/applicability-1.6.0.md)，评分依据及稳定性见 [1.5.0说明](docs/optimization-1.5.0.md)。选号规则仍为 `dlt-plan-v3`。

网页保留七个入口：方案生成、我的方案与对奖、每期推荐与复盘、开奖历史、概率与历史回测、策略对照实验、系统设置。

## 启动

```bash
cargo build --release --locked
./target/release/lottery-dlt serve
```

打开 **http://127.0.0.1:6792/**。默认数据库为 `data/lottery-dlt.sqlite3`，首次运行自动创建。端口与快乐8、双色球分开，避免占用原项目端口。

已部署服务器入口：**<https://1.14.208.189/dlt/>**，应用目录为 `/data/apps/lottery-dlt`，管理员令牌读取方式及升级步骤见 [生产部署说明](docs/deployment.md)。

也可开发启动：

```bash
cargo run --locked -- serve --bind 127.0.0.1:6792
```

本机serve不自动跑定时任务。生产每10分钟同步开奖、核实销售日程，并在截止前窗口固定参考推荐。期号和截止时间从核实日程自动填写；官方接口无未来日程时按官方规则、休市表和本年完整开奖核对推导，明确标注来源。随机与均衡选号无需历史；冷热号和遗漏实验必须有目标期以前的数据。

## 投注与留档

- **单式多注**：1～1000个不同的5+2单注；均匀随机或按号码使用次数均衡覆盖。
- **复式**：前区5～35个、后区2～12个号码完整展开，注数为 `C(前区数,5) × C(后区数,2)`。
- **胆拖**：前区0～4胆、后区0～1胆，至少一区有胆。号码池包含胆码；有胆的前区至少6个号码，后区至少3个号码。展开为 `C(前区池-前胆,5-前胆) × C(后区池-后胆,2-后胆)`。
- **追加与倍投**：基本2元/注，追加3元/注，1～99倍；单方案最多展开1000注，同时校验基本2万元或追加3万元的单张限额。

均衡单式7注可让35个前区号码各用一次；这描述的是号码并集覆盖，不能保证某一注命中5个前区号码。头奖概率按**不同完整单注数量 / 21,425,712**计算，倍投和追加不增加该概率。整组见奖率通过实际票组模拟，保留注间相关性，并报告模拟误差区间。

同一期号、选号参数及主种子可复现相同票组；冷热/遗漏另绑定历史窗口。追加和倍数不参与选号，改变它们保留原号码；随机、均衡及结构策略不因历史缓存更新换号。生成摘要绑定原始输入和完整票组。保存接口重新生成并核验预览摘要；参数、种子或开奖数据变化后必须重新生成。主种子通过字符串传给浏览器，保留全部64位，不发生 JavaScript 大整数舍入。

生产保存须匹配已核实的销售日程，截止时间在未来30天内，验证不得早于2小时前。全年开奖日期、官方最新号码及休市规则不符时拒绝归档；本机时间及摘要并非独立可信的开奖前证明。

已保存记录没有覆盖/删除接口。重复保存同一票组返回首次档案，保留首次名称、时间与原始号码。开奖或奖额导入后，按原票组复算；不会用新种子重建旧推荐。个人页只展示个人保存的票组；推荐页按期展示推荐留档、投入和结算对比。每期首次在一个事务中固定5个常用预设及其清单，之后修改种子、同步开奖或重复归档仍返回首次清单。已归档个人票组可以被清单引用，保留其原始身份和摘要；各预设预算不同，研究页另做同预算比较。

## 奖表与金额

实现按期号选择规则，而非将当前奖表套到全部历史：

| 期号 | 核算规则 |
|---|---|
| 19019～26013 | 2019九奖级 |
| 26014起 | 2026七奖级 |
| 19019以前 | 允许号码统计，暂不核算奖金 |

新规则中，固定奖随**开奖前奖池**是否达到8亿元变化。公布的各期奖金优先；不能用开奖后滚入下期的奖池余额推断本期固定奖。一、二等奖或新规则固定奖资料缺失时，中奖条件仍可核对，金额保持 `null`（待核实），不记为0元。不中奖时奖金明确为0。

追加只参与一、二等奖。浮动奖追加金额要求当期公告数据，避免将未经核实的80%计算或取整结果当作已公布奖额。固定奖标准推算标为 `standard_rules`，公布金额标为 `published`。金额全程使用整数“分”，奖金为税前基础奖额，不自动计活动派奖或税。规则来源和边界详见 [规则说明](docs/prize-rules.md)。

## 数据同步与导入

```bash
# 每页100期，最多50页；1页足够初次试用
./target/release/lottery-dlt fetch --pages 1

# 第三方调用环境无法访问官方接口时，导入已核实数据
./target/release/lottery-dlt import /path/to/history.json
./target/release/lottery-dlt import /path/to/history.csv

# 已核实的单期格式示例，仅26014期，不是全历史数据
./target/release/lottery-dlt import tests/fixtures/26014.json
```

官方接口为 `webapi.sporttery.cn` 的 `getHistoryPageListV1.qry`，游戏编号85。网络、上游格式或分页重复错误会拒绝整次同步，保留原缓存。本开发环境 DNS 受限；已在Ubuntu生产服务器实测同步2935期，当前上游响应同时提供开奖号码及奖金明细，上游未返回的奖额不会自行生成。

原生 JSON 为 `Draw` 数组或 `{"draws":[...]}`，也支持官方 `value.list` 响应文件。网页可粘贴原生开奖 JSON 或官方 value.list 响应；CSV 通过命令行导入。最小格式：

```json
[
  {
    "issue": "26014",
    "date": "2026-02-02",
    "front": [16, 18, 23, 34, 35],
    "back": [1, 6],
    "awards": {
      "2": {"basic_fen": 24215900, "additional_fen": 19372700},
      "3": {"basic_fen": 500000}
    },
    "source": "https://www.gdlottery.cn/f_html/kjgg/P085_26014.html"
  }
]
```

`awards` 可省略；只有已核实的**开奖前**奖池才填 `pool_before_fen`。允许为已有同期开奖补齐奖额；号码、日期、已知奖额或已知奖池冲突则整批回滚。校验号码数量、范围、同区重复、年份、未来日期和日期顺序。CSV 列名为 `issue,date,front,back`，号码用空格分隔；参考 [CSV示例](examples/history.csv)。

## 命令行

```bash
./target/release/lottery-dlt theory
./target/release/lottery-dlt predict --issue 26119
./target/release/lottery-dlt predict --config examples/compound.json
./target/release/lottery-dlt predict --config examples/dantuo.json

# 示例期号、截止时间须替换为实际尚未开奖的目标
./target/release/lottery-dlt predict --issue 26119 --save \
  --deadline '2026-10-17T20:00:00+08:00' --label '我的方案'

./target/release/lottery-dlt history
./target/release/lottery-dlt settle
./target/release/lottery-dlt stats --window 60
./target/release/lottery-dlt config --seed 123456
./target/release/lottery-dlt backtest --config examples/backtest.json
./target/release/lottery-dlt research --config examples/backtest.json
./target/release/lottery-dlt verify
./target/release/lottery-dlt export --out /tmp/dlt-export.json
```

所有命令可加全局参数 `--db /path/to/data.sqlite3`。`theory` 和 `healthcheck` 不创建数据库。`history` 与 `settle` 都返回原票组的最新核算结果，不额外改写票面。

## 回测和研究

回测支持三种投注形态、自选池、胆码、追加和倍数，网页可直接带入生成器配置。结构优化按同预算比较三种单式结构，支持覆盖、见奖与固定奖回本下界目标，选优后用独立模拟样本验证全部候选。

逐期使用目标期**之前**的历史窗口，固定种子处理同分；当期开奖结果只用于评分。默认预热60期，回放最近300期。样本不足时明确报错。回测和研究不写入实际投注记录。

研究固定比较均匀随机、均衡覆盖、热号、冷号、遗漏五组；同一批开奖、相同注数与预算，报告按开奖期配对的中奖注数差。完整轨迹包含目标期、数据截止期、票组摘要、中奖注数、并集命中、成本与可核实奖金。

2026-10-15的覆盖审查数据见 [覆盖、见奖与回本比较](docs/coverage-review-2026-10-15.json)。该报告使用1.0.1的 dlt-plan-v1，复现旧数值须使用对应发布源码。当前可用 `cargo run --release --locked --example coverage_review -- /path/to/history.json` 生成新版本报告；该程序只读历史，后区轮转组仅为离线实验，未加入正式生成器。比较覆盖5/6/7/20注、随机对照、复式、胆拖及实验策略，并为固定票组运行50万次共享均匀开奖模拟。

缺失奖额的期数不产生完整收益，单列可核算子集的成本和奖金。新增多种子历史留出研究，按期聚合配对，提供近似区间、Holm多重比较校正和规则分段。统计属于**探索性描述**，没有未来预注册确认窗，不宣称预测优势；未移植快乐8专用选1～选10模型和研究确认工作流。冷热号评分是排序启发式，不是校准后的中奖概率。

## 部署

默认仅监听本机。对外监听必须设置至少16字符的管理员令牌：

```bash
export LOTTERY_ADMIN_TOKEN="$(openssl rand -hex 32)"
./target/release/lottery-dlt serve --bind 0.0.0.0:6792
```

在网页“系统设置”填写同一令牌。令牌只保留在当前标签页的 `sessionStorage`；写入、计算请求和完整数据导出需要令牌。读接口展示开奖、方案与配置，适合个人共享看板；有隐私要求时在反向代理配置访问控制。代理对外入口应使用 HTTPS。

Docker Compose：

```bash
cp .env.example .env
# 编辑.env，填写生成的 LOTTERY_ADMIN_TOKEN
mkdir -p data
# 容器使用UID 1000，确保data目录可写
docker compose up -d --build
docker compose logs -f
```

镜像只包含 Rust 二进制与系统运行库，未包含 Python/Node。数据挂载 `./data`，包含健康检查、日志轮转和自动重启。`LOTTERY_BIND`、`LOTTERY_DB`、`LOTTERY_BASE_PATH` 可配置监听、数据库和代理前缀。访问 `/dlt/` 时以 `--base-path /dlt` 启动，并使用 [Nginx示例](deploy/nginx.conf) **保留**请求前缀代理。原生部署可参考 [systemd模板](deploy/lottery-dlt.service)，需按实际用户、路径和环境文件调整。

数据库使用 WAL 和事务，不要仅复制运行中主库文件。在线备份可用 `sqlite3 data/lottery-dlt.sqlite3 ".backup '/path/to/backup.sqlite3'"`；JSON导出用于检查/共享，`import` 仅导入其中的开奖，不恢复已归档方案。

## 验证与结构

```bash
cargo fmt --check
cargo test --locked --all-targets
cargo clippy --all-targets --locked -- -D warnings
node --check web/app.js  # 开发时检查，不是运行依赖
node --check web/pools.js
node --check web/format.js
node --check web/usability.js
node --check web/reviews-ui.js
node --check web/evidence-ui.js
node --check web/workflow.js
node --check web/transport.js
node --check web/replacements-ui.js
node --test tests/*.cjs
```

1.1.0已完成release构建、42项核心／HTTP／优化／覆盖实验测试、Clippy和JavaScript语法检查；已在Ubuntu服务器核验HTTPS监听、官方数据同步、访问认证、实际票组预览、模拟、滚动回测及策略对照。Chromium实测桌面和375px手机下的全部七个页签，无脚本错误和横向溢出。Docker镜像构建尚未验证，生产使用原生Rust二进制和systemd。

1.0.1修复派奖活动行覆盖基础奖额的问题，追加顺序无关和冲突拒绝的解析测试，以及维护预览、原始票面保留和审计测试；包含离线覆盖程序共25项测试通过。141期官方奖额缓存已备份、更正并审计；活动奖金仍不自动计入基础奖金或回本统计。

```text
src/model.rs     数据与范围校验、整数金额
src/plan.rs      可复现选号、复式/胆拖展开
src/prizes.rs    分期奖表、逐注核算、精确概率
src/store.rs     SQLite事务、原始留档、摘要与审计
src/reviews.rs   实际留档复盘、共同期对照、覆盖诊断
src/budget.rs    实际预算增量、两区集合及完整5+2票组比较
src/fetch.rs     官方请求、JSON/CSV导入
src/analysis.rs  滚动回测、策略对照、统计与模拟
src/web.rs       Axum接口、访问控制、内置资源
src/main.rs      CLI入口
web/            七页签响应式界面
tests/          奖表边界、展开、留档、导入、无前视、接口测试
```

1.0.2修复功能范围标注、每期推荐固定与分栏展示、实验候选池大小、无效参数影响种子、预览与模拟一致性和官方JSON网页导入。新生成规则为 `dlt-plan-v2`；旧方案保留原始票面和版本，旧版历史评估数值不能当作新版本结果。管理员令牌会验证后再保存，未登录页面明确提示计算和写入的访问要求。

1.1.0完成七项优化，生成规则dlt-plan-v3。新增官方日程、自动推荐状态、持久任务、单一快照导出、实际备份恢复校验及每日备份；完整说明与验收见 [优化清单](docs/optimization-1.1.0.md)。

1.2.0新增实际留档持续复盘及前后区号码池交集。47项Rust全目标测试、7项集合测试、4项前端状态检查、Release构建及Clippy通过；本轮沙箱禁止套接字，浏览器验收待在允许启动服务的环境执行。详见 [1.2.0实现与验证边界](docs/optimization-1.2.0.md)。

1.3.0补齐预算增量与交集丢号诊断；原票组优先显示，辅助统计故障可独立重试。51项Rust测试、21项JS检查和桌面／手机7页签Chromium验收通过，详见 [1.3.0记录](docs/optimization-1.3.0.md)。
