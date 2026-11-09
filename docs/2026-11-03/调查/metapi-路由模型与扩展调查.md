# metapi 路由模型与扩展机制调查

> 调查对象：`D:\CodeWorkspace\metapi`（TypeScript/Fastify + React/Vite + Electron + drizzle，**“中转站的中转站”**）
> 调查方式：只读源码；未修改仓库。调查日期：2026-11-03。路径相对仓库根。
> 说明：metapi 把多个 AI 中转站（New API / One API / OneHub / DoneHub / Veloera / AnyRouter / Sub2API / OpenAI / Claude / Gemini / Codex / Antigravity 等）聚合为一个网关（一个 API Key、一个入口）。

## 1. 概览与技术栈

- **定位**：`README.md` — “A relay for relays — aggregate scattered AI relay stations into one unified gateway”；自动发现模型、智能路由、成本最优。
- **后端**：TypeScript + **Fastify**（`src/server/index.ts`；`@fastify/cors`、`@fastify/static`）。
- **数据库**：**drizzle-orm**，支持 SQLite（better-sqlite3）/MySQL/postgres（`src/server/db/schema.ts`、`migrate.ts`、`runtimeSchemaBootstrap.ts`）。
- **前端**：React + Vite（`src/web/*`，`App.tsx`、`pages/`）；图表 `@visactor/react-vchart`。
- **桌面**：Electron（`src/desktop/*`，`electron-builder.yml`）。
- **共享**：`src/shared/*`（tokenRouteContract、tokenRoutePatterns、logCleanupRetentionDays 等）。
- **其他依赖**：`rate-limiter-flexible`、`node-cron`、`nodemailer`、`marked`、`mermaid`。

## 2. 页面与 HTTP 路由

### 2.1 后端路由（Fastify）

`src/server/index.ts` 注册两组路由：

- **管理 API `/api/*`**（`src/server/routes/api/*`）：`sites`、`accounts`、`accountTokens`、`tokens`（=token routes? 见下）、`stats`、`settings`、`checkin`、`search`、`events`、`tasks`、`test`、`monitor`、`downstreamApiKeys`、`oauth`、`siteAnnouncements`、`updateCenter`。
- **代理入口 `/proxy/*`**（`src/server/routes/proxy/*`）：`chat`、`completions`、`responses`、`gemini`、`embeddings`、`images`、`rerank`、`files`、`inputFiles`、`videos`、`search`、`models`、`multipart`；`router.ts` 组合、`routeAliases.ts` 别名、`runtimeExecutor.ts` 执行。
- 鉴权：`src/server/middleware/auth.ts`；下游 API Key 由 `downstreamApiKeyService` 管理，`downstreamPolicy.ts` 描述策略。

### 2.2 前端页面（`src/web/pages`）

Dashboard / Sites / Accounts / Tokens / TokenRoutes / Models / ModelTester / ProxyLogs / ProgramLogs / DownstreamKeys / OAuthManagement / Settings / Monitors / CheckinLog / SiteAnnouncements / ImportExport / About。
`src/desktop` 提供 Electron 外壳；`src/web/App.tsx` 为路由与布局。

## 3. 聚合数据模型（`src/server/db/schema.ts`）

```
sites(站点) 1─N accounts(账号) 1─N accountTokens(账号令牌)
tokenRoutes(路由,按 modelPattern) 1─N routeChannels(渠道=account+token)
oauthRouteUnits / oauthRouteUnitMembers( OAuth 组合单元 )
```

关键字段：
- **sites**：`platform`（new-api/one-api/veloera/one-hub/done-hub/sub2api/orcarouter/openai/claude/gemini/codex/gemini-cli/antigravity）、`url`、`proxyUrl`、`customHeaders`、`status`、`isPinned`、`sortOrder`、`globalWeight`、`maxConcurrency`、`postRefreshProbe*`（`:4-28`）。
- **accounts**：`siteId`、`accessToken`/`apiToken`、`balance`/`balanceUsed`/`quota`、`unitCost`、`valueScore`、`status`、`isPinned`、`sortOrder`、`checkinEnabled`、`oauth*`、`extraConfig`（`:60-86`）。
- **accountTokens**：`accountId`、`token`、`tokenGroup`、`valueStatus`（ready…）、`source`(manual/sync/legacy)、`enabled`、`isDefault`（`:91-…`）。
- **tokenRoutes**：`modelPattern`、`routeMode`(pattern)、`modelMapping`、`decisionSnapshot`、`decisionRefreshedAt`、`routingStrategy`(weighted 默认)、`enabled`（`:150-…`）。
- **routeChannels**：`routeId`、`accountId`、`tokenId`、`oauthRouteUnitId`、`sourceModel`、`priority`、`weight`(默认 10)、`enabled`、`manualOverride`、`successCount`/`failCount`、`totalLatencyMs`/`totalCost`、`lastUsedAt`/`lastSelectedAt`/`lastFailAt`、`consecutiveFailCount`、`cooldownLevel`、`cooldownUntil`（`:215-…`）。
- 其他：`modelAvailability`/`tokenModelAvailability`（模型可用性探测）、`proxyLogs`/`proxyDebugTraces`/`proxyDebugAttempts`、`settings`、`adminSnapshots`、`downstreamApiKeys`、`siteAnnouncements`、`events`。

## 4. 路由模型（聚合 + 智能路由 + 成本优化）

### 4.1 选路全流程（`/proxy/*` → 上游站点）

1. **入口**：`/proxy/...` 路由（`src/server/routes/proxy/*`）→ `runtimeExecutor` → `proxy-core`。
2. **模型/别名归一**：`normalizeModelAlias`（`tokenRouter.ts:1411`）、`routeAliases.ts`；按 `modelPattern`（精确/glob/`re:` 正则）匹配 `tokenRoutes`（`isExactTokenRouteModelPattern`/`isTokenRouteRegexPattern`）。
3. **选择渠道**：`tokenRouter.selectChannel(requestedModel, downstreamPolicy)`（`tokenRouter.ts:1835`）——在该 route 的 `routeChannels`（account+token）里按 `routingStrategy`（**weighted 默认 / round_robin / stable_first**，`routeRoutingStrategy.ts`）排序，结合权重、优先级、并发、健康、冷却、成本选择。
4. **前置过滤**：`filterSiteRuntimeBrokenCandidates`（站点熔断）、`filterRecentlyFailedCandidates`（近期失败）、`isChannelRecentlyFailed`、`routeCooldownService`（`cooldownLevel`/`cooldownUntil`）、模型可用性（`modelAvailability`/`tokenModelAvailability`）、账号/站点状态与 `maxConcurrency`。
5. **站点健康**：`SiteRuntimeHealth` + breaker（`getSiteRuntimeHealthMultiplier`/`isSiteRuntimeBreakerOpen`，`tokenRouter.ts:912-…`）；成功/失败 `recordSuccess`/`recordFailure`（`:2448`/`:2655`）更新延迟/成本/失败计数。
6. **执行**：`proxy-core/conductor`（`DefaultProxyConductor` + `retryPolicy.ts` + `streamTermination.ts` + `usageHooks.ts`）编排；`providers/*`（claude/codex/antigravity/gemini-cli 的 providerProfile+executor）、`surfaces/*`（chat/responses/gemini/models/files…）、`transformers/{openai,anthropic,gemini,canonical}` 做协议转换；`orchestration/upstreamRequest.ts` 构造上游请求；`firstByteTimeout.ts` 首字节超时；`serviceTierPolicy.ts` 服务等级。
7. **会话/粘性**：`tokenRouter.session-decoupling.test`、`session-binding`、`downstreamClientContext`；模型可用性探测与价格参考成本在选路时参与（`modelPricingService` 的 `referenceCost`）。
8. **失败转移**：`proxyChannelRetry`/`proxyRetryPolicy`/`proxyFailureJudge` 决定重试或换渠道；`routeCooldownService` 冷却；`channelRecoveryProbeService` 后台探活；`accountHealthService` 账号健康。
9. **计费**：`proxyBilling`/`proxyUsageParser`/`proxyUsageFallbackService` 解析并计费。

### 4.2 策略/权重要点
- **route 级策略**：`weighted`（默认，按 `routeChannels.weight`）、`round_robin`、`stable_first`（稳定优先，`stablePluginBucket`/stable-first cache）；可对此 token/route 做 `decisionSnapshot`（决策快照）与 `explainSelection`（可解释选择）。
- **多层优先级**：`priority`（越小越优先）+ `weight`；`priorityBuckets`/`priorityRail`（前端也有）。
- **健康/成本**：站点 `globalWeight`、账号 `valueScore`、`unitCost`、`balance`，模型 `price/referenceCost`；故障按站点熔断乘子降权。
- **并发**：站点 `maxConcurrency` + `account_load_factor` 相关服务；`proxyChannelCoordinator` 做负载快照。
- **模型发现**：`modelService.discovery`（站点模型自动发现）、`modelAvailabilityProbeService`、`modelAnalysisService`。

## 5. 扩展机制（无通用插件系统）

metapi **没有插件/市场系统**（src 无 plugins 目录）。扩展点是编译期 TS 模块 + 注册表：
- `src/server/proxy-core/providers/registry.ts`：providerProfile（claude/codex/antigravity/gemini-cli）注册。
- `src/server/proxy-core/executors/*`：各执行器。
- `src/server/proxy-core/surfaces/*`：协议面（chat/responses/gemini/models/files…）。
- `src/server/transformers/{openai,anthropic,gemini,canonical}`：协议转换。
- `src/server/services/platforms/*` + `platformDiscoveryRegistry.ts` + `siteDetector.ts`：识别/发现新平台站点。
- `src/server/services/*`：路由/定价/探测/健康等可配置服务（非插件）。

即“新增一个上游平台 = 写 provider/executor/surface/transformer 并注册”，无运行时热插拔、无第三方插件。

## 6. 关键参数与特点

- `routingStrategy` 默认 `weighted`；`routeChannels.weight` 默认 10；`priority` 默认 0。
- 支持 SQLite/MySQL/PostgreSQL 三库（drizzle schema 三方言 + bootstrap/upgrade SQL）。
- `stable_first` 缓存、`decisionSnapshot`、`explainSelection` 提供可解释路由。
- 站点级熔断/健康乘子、账号 valueScore/unitCost、模型 referenceCost 共同决定“成本最优”。
- 部署形态：`npm start`（node dist/server）、Docker（`Dockerfile`）、Zeabur/Render 模板、Electron 桌面。

## 7. 证据（文件:路径）

- `README.md`/`README_EN.md`、`package.json`（scripts/deps）
- `src/server/index.ts`（路由注册）、`src/server/routes/api/*`、`src/server/routes/proxy/*`
- `src/server/db/schema.ts:4-235`（sites/accounts/accountTokens/tokenRoutes/routeChannels 等）
- `src/server/services/tokenRouter.ts:912-2920`（selectChannel/selectNextChannel/recordSuccess/recordFailure/site health/breaker/normalizeModelAlias）
- `src/server/services/routeRoutingStrategy.ts`（weighted/round_robin/stable_first）
- `src/server/services/{routeChannelService,routeCooldownService,routeDecisionRefreshService,modelService,modelPricingService,modelAvailabilityProbeService,proxyChannelCoordinator,proxyChannelRetry,proxyRetryPolicy,proxyFailureJudge,upstreamRequestBuilder,accountHealthService}.ts`
- `src/server/proxy-core/{channelSelection.ts,conductor/*,executors/*,providers/*,surfaces/*,transformers/*}`
- `src/server/middleware/auth.ts`、`src/server/services/downstreamApiKeyService.ts`

## 8. 未确认项
- `tokenRouter.ts`（144KB）内部所有分支未逐行穷举，本报告按函数与测试归纳。
- 具体计费公式/窗口细节未逐行核对（见 `proxyBilling`/`proxyUsageParser`）。
- 未运行 metapi 做行为验证。
