# 1.4.0 易用性优化与实际验收

本轮检查并修改“选号设置 → 确认并保存 → 查票与复盘”三步。保留现有红蓝配色、七个页面入口及 `dlt-plan-v3` 选号规则；使用独立临时数据库完成截图和交互，截图号码与奖金均为合成测试数据，不是实际推荐成绩。

## 1. 设置方案：已简化并验证

原界面在普通随机单式下仍显示号码池、胆码、历史窗口等灰色参数，生成前无法直接看到修改后的注数和金额。改为仅显示当前投注形态适用的参数；复式、胆拖和历史实验需要的字段会随选择出现。

增加实时注数及金额预估：单式按注数，复式与胆拖按两区组合展开；追加和倍数同步更新。超过1000注、单张金额限额或候选组合不足时，直接说明原因，阻止无效生成。前端估算不替代Rust校验，不生成或修改任何号码。

普通单式当前可见的不适用禁用字段从5项减为0项。切换到胆拖时，提供合法起始胆码设置，之后仍可自行修改。

![优化前：普通单式同时出现多个不适用参数](../test-results/usability-before/01-generator-desktop.png)

![优化后：仅显示适用参数并预估费用](../test-results/usability-1.4.0/01-generator-desktop.png)

## 2. 确认与保存：已完成手机实际操作

原界面先展示全部票组，再显示保存入口。375×812手机查看20注胆拖时，保存按钮位于票组顶部以下约1726px。现在总金额、截止时间、名称、保存和复制位于票组顶部，保存按钮约在358px处；生成后自动定位到预览标题。

保存成功显示原始档案确认和“查看这组方案与对奖”，进入正确的个人方案／推荐来源、筛选目标期，并直接展开该票组。重复档案继续保留首次身份，不覆盖旧号码、名称和时间。

超过20注的预览先展示20注，可展开全部。展开时保留已填写的截止时间与名称；保存和复制始终包含完整票组。实际检查了25注的完整复制，以及20注生成、保存与重新读取逐项一致。

提示改为屏幕底部可见、可关闭的反馈，错误使用警报语义，支持Esc关闭；失败后保留表单输入。不存在自动购票、付款或发送消息行为。

![优化前：20注预览需继续下翻才能保存](../test-results/usability-before/02-preview-mobile.png)

![优化后：金额和保存入口位于票组前](../test-results/usability-1.4.0/02-preview-mobile.png)

![保存成功：直接查看这组原始方案](../test-results/usability-1.4.0/05-saved-mobile.png)

## 3. 查票与复盘：已验证筛选、定位及移动端

个人方案增加“全部／待开奖／已开奖”筛选，与期号前缀查询组合使用，并提供查询和清除按钮。筛选在SQLite中先于计数和分页执行，按全部匹配记录计算总数；“已开奖”包括奖额仍待核实的记录。没有匹配时说明如何调整筛选，不误称用户没有保存过方案。个人与推荐页的期号筛选分别保留。

逐注详情按钮具备展开状态和目标区域；保存后的直达链接自动展开原票。命中号码增加文字标记与可访问名称，辅助区别前后区及命中状态。

手动整期归档与逐期对比默认折叠，原始票组先展示。375px下七个导航入口直接可见、按钮高度至少44px；切页后定位标题。调整次要文字和焦点提示，并保留原有表格键盘横向滚动。

![优化前：手机首屏主要为手动归档表单](../test-results/usability-before/04-reviews-mobile.png)

![优化后：导航全部可见，手动归档默认折叠](../test-results/usability-1.4.0/04-reviews-mobile.png)

![保存后直达原票并展开逐注号码](../test-results/usability-1.4.0/06-saved-record-mobile.png)

## 实现与验收

- 版本1.4.0；无需迁移数据库，保存票组与选号算法继续兼容。
- 新增可选查询：`GET /api/records?page=1&limit=20&kind=personal&status=pending|settled`，不传状态沿用原行为；非法状态拒绝。
- 52项Rust全目标测试、25项JavaScript检查、Clippy及Release构建通过。
- `scripts/verify-usability.cjs` 实际完成设置、生成、保存、直达档案、状态筛选、筛选空结果、限额前置提示、大票组展开、完整复制和错误恢复。
- `scripts/verify-k8-alignment.cjs` 保留复盘、预算增量、池分析、CSV／JSON、复制失败与故障恢复回归。
- 两套浏览器流程均检查1440px桌面与375px手机全部七页签，无脚本错误或页面横向溢出；原始记录、配置、开奖和审计数据保持一致，除测试中明确执行的一次新保存外未写入其它档案。

本轮截图和报告分别位于 `test-results/usability-before/`、`test-results/usability-1.4.0/`、`test-results/k8-alignment-1.4.0/`。这些目录是本地验收产物，按项目规则被Git忽略；未上传到外部应用，未修改生产服务或向项目的正式数据目录导入合成开奖。

验证覆盖真实Chromium点击、键盘、原生表单、剪贴板及响应式布局；未做屏幕阅读器、移动实体设备及完整无障碍合规认证。

```bash
cargo fmt --check
cargo test --locked --all-targets
cargo clippy --all-targets --locked -- -D warnings
node tests/pools.cjs
node tests/review-ui.cjs
node tests/format.cjs
node tests/usability.cjs
cargo build --release --locked
# 需本地已有Playwright及Chromium；PLAYWRIGHT_MODULE可指向已有模块
node scripts/verify-usability.cjs --binary target/release/lottery-dlt --browser /path/to/chrome
```
