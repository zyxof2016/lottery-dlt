# Ubuntu 生产部署

服务器：`ubuntu@1.14.208.189`。大乐透应用位于 `/data/apps/lottery-dlt`，入口为 <https://1.14.208.189/dlt/>，HTTP同路径会跳转HTTPS。服务仅监听 `127.0.0.1:6792`，通过现有彩票站点 Nginx 代理，保留 `/dlt` 前缀。

## 目录与服务

```text
/data/apps/lottery-dlt/
  current -> releases/20261016-72fcd8601f63
  releases/                  二进制、SHA256版本清单、对应源代码
  shared/service.env         监听、代理前缀、数据库和管理员令牌，权限0600
  shared/data/lottery-dlt.sqlite3
  backups/                   配置与数据库备份，权限0700
  staging/                   发布暂存和验证结果，权限0700
```

- `lottery-dlt.service`：以 `ubuntu` 运行，开机自启，失败自动重启。
- `lottery-dlt-sync.timer`：每10分钟调用 `operate --pages 2`，同步最近200期开奖，核实销售日程，执行提前窗口内的参考推荐归档及结算状态检查。
- `lottery-dlt-backup.timer`：每天北京时间03:10进行在线备份和独立恢复验证；自动备份保留30天。
- systemd环境文件设置 `LOTTERY_BASE_PATH=/dlt`、`LOTTERY_BIND=127.0.0.1:6792` 和绝对数据库路径；1.1.0设置 `LOTTERY_REQUIRE_SCHEDULE=1`、`LOTTERY_BACKUP_DIR=/data/apps/lottery-dlt/backups`。
- Nginx配置位于 `/etc/nginx/conf.d/lottery-k8.conf`，大乐透路由在 `# lottery-dlt begin/end` 标记之间；现有快乐8和双色球路由保留。

管理员令牌在服务器首次部署时随机生成，未写入源码、日志或发布清单。通过SSH读取后，粘贴到网页“系统设置 → 管理员访问”：

```bash
ssh ubuntu@1.14.208.189 'sed -n "s/^LOTTERY_ADMIN_TOKEN=//p" /data/apps/lottery-dlt/shared/service.env'
```

## 日常检查

```bash
ssh ubuntu@1.14.208.189 'systemctl is-active lottery-dlt.service lottery-dlt-sync.timer'
curl -fsS https://1.14.208.189/dlt/healthz
ssh ubuntu@1.14.208.189 'sudo -n journalctl -u lottery-dlt.service -u lottery-dlt-sync.service -n 40 --no-pager'
ssh ubuntu@1.14.208.189 '/data/apps/lottery-dlt/current/lottery-dlt --db /data/apps/lottery-dlt/shared/data/lottery-dlt.sqlite3 verify'
```

`scripts/verify-deployment.py` 可在服务器上验证页面资源、认证、预览、模拟、回测与同预算研究；读取服务器环境文件取得令牌，不输出令牌，不保存投注方案。

## 升级与回滚

1. 本地验证并构建release，生成二进制SHA256及源代码摘要。新建版本目录，上传二进制、清单和对应源代码；在服务器再次核验SHA256及运行库兼容性。
2. 记录 `readlink current`，使用 Python `sqlite3.Connection.backup` 或 SQLite `.backup` 在线备份数据库；不要只复制运行中的主库文件。
3. 暂停同步timer并等待当前同步结束，短暂停止网页服务，用同目录临时链接及 `mv -T` 原子切换 `current`。`shared/` 中的数据库和令牌保持原位，不用本地测试数据替换。
4. 启动服务，核验健康、首页、静态资源、实际方案数量与摘要；恢复原本启用的同步timer。失败时将 `current` 原子切回旧目录，重启并再次核验。
5. Nginx仅在路由确需变化时修改；先备份，`nginx -t` 成功后reload，失败恢复备份。勿整体替换其他项目的站点配置。

首次上线已同步2935期真实开奖，最新为26117期（2026-10-14），缓存区间内无缺期，未导入本地测试方案。发布二进制SHA256为 `399a1d24eb4965d9bbc4a4baa8c147cede3b29f7bd28026d6388d3d76b6ac1db`。

2026-10-15升级到1.0.1，修复活动派奖行覆盖基础奖金的问题。以官方25049期公告及原始响应验证：九等奖基础5元，活动加奖2.5元，后者不能替代前者。重新获取官方全历史，对141期受影响的奖金缓存作事务更正，记录新旧内容摘要并追加审计链；备份为 `backups/pre-award-fix-20261015.sqlite3`。开奖号码、日期、开奖前奖池、主种子、原始方案和既有审计记录逐项核验保持一致。该版二进制SHA256为 `59e54690e83f30a51a2f24d4328ef7c31782b71689041f58aefdbce691b8514c`。

`repair-prizes --pages 50` 默认只预览官方奖额差异；显式加 `--apply` 才修改缓存。它独立于日常导入的冲突拒绝规则，只接受官方来源且要求原始开奖号码、日期完全一致。维护前须备份，不通过此命令修改开奖号码或补造推荐。

线上验证已覆盖HTTPS健康检查、管理员写入认证、实际票组预览、模拟、严格回测和五组同预算研究。Chromium实测桌面1440px和手机375px下的七个页签，验证7注均衡、6注复式、20注胆拖、修改参数后预览失效及页面分析操作；无页面脚本错误或横向溢出，全程实际保存方案数量保持0。验证JSON与截图存放在服务器 `staging/browser-qa/`，初始SQLite在线备份为 `backups/initial-20261015.sqlite3`，完整性检查为 `ok`。

2026-10-16功能核查修正1.0.2：推荐按整期首次清单固定，个人方案与推荐分开显示，严格使用实验候选池，禁用不适用参数，模拟校验预览摘要，接通官方JSON网页导入，并移除未实现功能的名称承诺。新增表 `recommendation_batches`，不改写已有开奖、方案、配置和审计。发布前用独立端口6793及临时数据库完成浏览器保存、刷新、导出、推荐归档、改种子、服务重启、导入隔离合成开奖、奖金缺失和补充后结算验证。

本版二进制SHA256：`db5622e34132b9897900e4d7f601612ddbf93a04f87ee40553c582d7002c727f`，升级备份 `backups/pre-functional-fix-20261016.sqlite3`；验证结果存放 `staging/functional-audit-20261016/`。生成规则改为 `dlt-plan-v2`，旧方案票面保持原样。功能未完整对齐 lottery-k8，具体实现边界见 [功能核查](functional-audit-2026-10-16.md)。

1.1.0发布：`releases/20261016-72fcd8601f63`，二进制SHA256 `72fcd8601f63d528608877074919cc083c2546b23cd2e295b86510e8a03afbde`。升级前备份在 `backups/pre-1.1.0-20261016/`，包含SQLite、私有环境配置和原服务单元；保持0700目录及0600环境文件。原始开奖号码、日期、奖额、主种子、保存票面和既有审计记录保留；新增运行记录和审计会继续追加。

新增任务、日程和备份流程详见 [1.1.0实现与验收](optimization-1.1.0.md)。验收证据在 `staging/optimization-20261016/`。新规则或新策略生成的记录可能无法由1.0.x解析，已有新记录时不要直接切回旧程序；先在副本核验兼容性，优先向前修复，不用升级前备份覆盖新增用户记录。

1.9.0 使用 `scripts/deploy-release.sh` 发布。脚本上传已构建的 release 二进制和源码包，在线备份生产库，先在副本上执行 `verify`，再原子切换 `current`。`shared/` 里的数据库和管理员令牌保持原位。切换后核对开奖、方案、设置、审计和运行记录的内容摘要未变；健康检查失败时切回上一版。1.9.0 只增加空的 `selection_evidence` 表，不改写已有记录。本版二进制 SHA256 为 `fb5ac7ca8626bbec7cc219add59e3fe252da345a959413219c52a5478c3f7710`。
