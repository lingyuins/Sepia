# octopus 与 magpie「路由模型」完整对比

> 对象：`D:\CodeWorkspace\octopus`、`D:\CodeWorkspace\magpie`
> 方式：只读源码逐文件核对，结论标注 `文件:行号`；逐项目细节见同目录
> `octopus-路由模型调查.md`、`magpie-路由模型调查.md`。
> 说明：这里的“路由模型”指**一个推理请求进来后，如何被解析成模型目标、如何生成候选、如何挑选到某个渠道/密钥/账号、失败如何转移、会话如何保持**——即请求级路由（不是 HTTP 端点路由）。

---

## 0. 结论速览

| 维度 | octopus | magpie |
|---|---|---|
| 路由抽象 | **分组 Group → 组项 GroupItem(渠道+模型+优先级+权重) → 渠道 Channel → Key 或 池账号** | **Provider / Group → 候选 Candidate(某 key 或 subscription account × 某模型) → 策略 Seat** |
| 路由粒度 | 按「模型/端点」建分组，组内是渠道 | 按 provider 的 keys/accounts，或 group 的 members（`provider/model[:effort]` 或 `group/<id>`） |
| 策略模型 | `balancer.Balancer` 接口 + `GroupMode` 枚举固定 5 种（**不可第三方扩展**） | `routes.Strategy` **注册表**，任意插件可 `Register`；内置另 9 种由 `plugins/rotation` 注册（**可扩展**） |
| 内建策略 | round_robin / random / failover(按 priority) / weighted(Smooth WRR) / auto(成功率+延迟+TTFT+价格+探索+softmax) | Smart / order / rotate / usage(least used) / pace / weight / free / fast / drain / cheap(仅组) |
| 健康与摘除 | **熔断器** Closed/Open/HalfOpen（阈值、基础冷却、指数退避、半开探测超时）+ 按 `(channel,key,model)` 的 **Key 冷却** + Key 软优先级(cost/availability/speed/priority) | **Rest**（按失败原因定时长：欠费 30m、额度到重置、429 retry-after、其它 1m 递增）+ `Sink`(429 但仍有额度→排到末尾) + `FirstToken` 超时换员 + `Patience` 重试 + HTTP/2 静默检测 |
| 会话亲和 | `Group.SessionKeepTime`：按 `apiKey+model` 粘到上次渠道（内存 sticky） | `Affinity`：auto/session/turn/off/strict，持久化 `affinity.json`，按 `conversationID`/`stickKey`（含 Claude Code 子 Agent 区分），remote 用 vendor 对 |
| 条件/智能路由 | `Group.Condition`(JSON 规则：model/api_key_name/content_type/hour/header_ + equals/contains/regex/in_list/gt/lt…) + `MatchRegex` + 自动分组 + **AI 路由生成**（离线生成/调整组配置） | `Group.Rules`(按 intent 规则，先匹配先得) + `Classifier`(Jev 决策模型或任意模型) + `Effort:"auto"`(在线分类思考强度) + `Match`(glob/regex) + **auto/<model> 发现组** |
| 分组嵌套 | **不支持**（组项只能是渠道） | **支持**（组可含组、含 fallback 组，按 `Member.Path/Via` 逐层展开） |
| 账号池 | 渠道可挂 `PoolID`，走独立 **池调度器 poolscheduler**（EWMA 错误率/TTFT、粘性、并发槽、优先级分层、轮询/least-loaded/EWMA、鉴权错误计数、限流/过载冷却） | 账号就是 provider 的候选（`candidate.Account`），无独立池调度器；账户级权重 `AccountWeights` |
| 模型映射/价格 | model-mapping / model-normalize / price-category / price-schedule / 模型市场 | catalog(models.dev) + `ModelIDs`/`ModelWires`(本地 id↔wire) + `PriceAt`/`FreeModel`/价格窗口 |
| 失败转移 | 组内候选遍历 + 多轮 route round + retry cache + 无进展 fail-fast + 每渠道 key 重试 | `Provider.Fallback` 列表（可含 fallback 组）+ 组内 planLevel + 每轮每 model 只试一次 + `restLast`（休息的排最后） |
| 可观测 | `ChannelAttempt`(每次尝试：跳过/熔断/成功/耗时) + metrics | `Route`/trace/route events（候选、rests、谁回答）+ `X-Magpie-Provider`/`X-Magpie-Model` + usage ledger |
| 选路触发点 | `internal/relay/relay.go`（`/v1` 核心）+ `media_relay.go`（媒体） | `internal/gateway/gateway.go serve` → `plan`（`fallback.go`）→ `weighRouted`（`routing.go`）→ `routes.Apply` |

一句话：**octopus 是“按模型建组、组内按枚举模式在渠道/密钥/池账号之间均衡并带熔断降级”的经典 one-api/new-api 式路由**；**magpie 是“按 provider/组合并候选、用可插拔策略（Seat）排序、带丰富额度/速度/价格/亲和语义和插件化扩展点”的现代网关路由**。

---

## 1. octopus 的路由模型

### 1.1 数据模型（`internal/model/`）
- **Group**（`group.go:38-71`）：路由的基本单位，按 `EndpointType`+`Name` 唯一；关键字段：
  - `Mode GroupMode`（`:45`）——路由模式；
  - `MatchRegex`（`:46`）——模型名匹配，决定请求落到哪个组；
  - `Condition`（`:53`）——条件路由 JSON（见 §1.6）；
  - `FirstTokenTimeOut` / `AttemptTimeOut` / `SessionKeepTime`（`:47-52`）——首 token 超时、单次尝试超时、会话保持秒数；
  - `OutboundFormat`（`:44`）——出站格式（auto/chat/responses/messages/…/passthrough/raw）；
  - `ReasoningBufferStrategy` / `ThinkingMode`（`:66-70`）。
- **GroupItem**（`group.go:73-80`）：`GroupID + ChannelID + ModelName` 唯一，带 `Priority`、`Weight`。
- **Channel**（`channel.go:82-129`）：`Type`(出站类型)、`BaseUrls`、`Keys []ChannelKey`、`Model/CustomModel`、`ProxyMode`、`Disposable`(一次性渠道)、`ExpireAt`、`KeySelectionStrategy`、`PoolID`(号池)、`NodeID`、`MatchRegex` 等。
- **ChannelKey**（`channel.go:142-160`）：`TotalCost`、`Priority`、`SupportedModels`（按 key 隔离可服务模型）。
- **模型/价格**：`model_mapping.go`、`model_normalize.go`、`price_category.go`、`price_schedule.go`、`model_capability.go`、`endpoint_type.go`。
- **号池 / 站点**：`pool.go`(PoolAccount)、`site.go`/`site_channel.go`/`site_route_metadata.go`（把远端站点聚合成托管渠道）。

### 1.2 五种分组模式（`group.go:30-36`，实现 `relay/balancer/balancer.go`）
| 模式 | 常量 | 语义 | 实现 |
|---|---|---|---|
| 轮询 | `GroupModeRoundRobin=1` | 从上次位置轮转排列 | `balancer.go:47-60`（全局原子计数器） |
| 随机 | `GroupModeRandom=2` | 全量随机打乱 | `balancer.go:62-76` |
| 故障转移 | `GroupModeFailover=3` | 按 `Priority` 升序（0 视为最大→最后） | `balancer.go:78-86`、`sortByPriority` `:238-252` |
| 加权 | `GroupModeWeighted=4` | **Smooth Weighted Round-Robin**（Nginx 同款），展开成周期序列，`maxWeightedSequenceLen=1000` 防爆 | `balancer.go:88-96` → `weighted.go` |
| 自动 | `GroupModeAuto=5` | 探索+利用：未达最小样本数优先（exploration），达样本后按 `成功率×(1-latencyWeight)+延迟分×latencyWeight(+价格分×priceWeight)` 打分排序；同分桶随机洗牌 + softmax 探索；全熔断渠道降权到末尾（不剔除，保留半开探测） | `balancer.go:98-208`，参数读取 `:254-272` + `auto_shuffle.go` |

### 1.3 候选编排（`relay/balancer/iterator.go`）
`NewIterator(group, apiKeyID, requestModel, excludedChannels)`：
1. `GetBalancer(mode).Candidates(items)` 或 `groupRotationCandidates`（轮询/加权用组持久轮转）；
2. 按 API Key 的渠道黑名单 `excludedChannels` 剔除；
3. `Disposable`（一次性渠道）稳定提到最前；
4. `SessionKeepTime>0` 时把粘性渠道移到 index 0（`GetSticky`）；
5. 遍历时 `SkipCircuitBreak` 硬过滤熔断，`Skip` 记录跳过（渠道禁用/无 key/类型不兼容/模型不匹配等），`StartAttempt/End` 记录真实尝试。跳过明细上限 200 条防日志爆炸（`:36`）。

### 1.4 Key 选择与健康（`relay/balancer/`）
- **熔断器** `circuit.go`：`(channelID,keyID,model)` 维度，`Closed/Open/HalfOpen`；阈值默认 5（`setting.go:155`），基础冷却 60s、最大 600s、指数退避，半开探测超时 60s（`circuit.go:16-70`）。
- **Key 冷却** `key_cooldown.go`：按 `(channel,key,model)` 独立冷却（默认 300s，`setting.go:159`），解决“同 key 对 model A 429 连带 B/C”的问题（`:16-25`）。
- **Key 选择策略**（`Channel.KeySelectionStrategy` / 全局 `SettingKeyKeySelectionStrategy`，`setting.go:37`）：`cost`(默认，按 TotalCost) / `availability`(可用度分数，出错衰减、成功/时间恢复) / `speed`(EMA 平滑 TPS) / `priority`。实现 `key_availability.go`、`key_speed.go`。
- **自动洗牌** `auto_shuffle.go`：Auto 同分桶容差 + softmax 探索概率。
- 文件注入式解耦：`AutoPriceFunc`、`KeyCooldownFunc`、`KeySpeedTPSFunc`、`KeyAvailabilityScoreFunc` 由 `relay/init_hooks.go` 在启动时注入，避免包循环依赖。

### 1.5 号池调度（`relay/poolscheduler/scheduler.go`）
渠道 `PoolID>0` 时候选循环里改走 `SelectAccountLease(poolID, sessionHash, excludeAccountIDs, concurrency, model)`（`relay.go:1754-1778`）。调度链：
1. sticky 命中（同会话）→ 若该账号错误率/TTFT 超阈值则逃逸（`trySticky`/`shouldEscapeStickyAccount`/`stickyEscapeThresholds`）；
2. `filterByModel`（账号支持该模型）→ `filterExcluded`（本次已失败）→ `filterLayeredByPriority`（优先级分层）→ `filterBySlot`（并发槽未满）；
3. `selectByStrategy`：`selectByLeastLoaded`（最小负载）或 `selectByEWMA`（错误率+TTFT+重置就绪因子+额度余量因子，权重可配）；
4. 租约 `lease.go`（`acquireSlot/ReleaseSlot`）；结果 `ReportResult` 更新 EWMA 与 DB，并发 `pool.account.changed` 事件。
- 限流/过载/鉴权错误：`SetRateLimitCooldown`、`SetOverload`、`ReportAuthErrorCount`/`auth_error_counter.go`、`ratelimit_headers.go`。

### 1.6 条件路由、自动分组与 AI 路由
- **条件路由** `relay/condition/evaluator.go`：规则 `{key,op,value}` 全 AND；`key` 支持 `model/api_key_id/api_key_name/content_type/hour/header_<name>`；`op` 支持 `equals/not_equals/contains/not_contains/starts_with/ends_with/regex/in_list/gt/lt`；regex 有 250ms 超时。
- **自动分组** `model/auto_group.go`：扫描渠道模型，做模型名归一/去重（`ModelIdentity`），生成 `CandidateGroup`→创建分组。
- **AI 路由** `internal/helper/ai_route.go` + `internal/model/ai_route*.go` + `op/airoute`：异步任务（30min 超时、进度心跳 2s、SSE 进度流），用 LLM 生成/调整分组与路由，落库为普通 Group。

### 1.7 重试与失败转移（`relay/relay.go`）
- 外层 **route round** 循环，每轮 `applyRouteFilterPlugin`（`relay.route.filter` 插件可裁剪组内候选）→ `NewIterator` 遍历；
- 每个 item：取渠道→校验启用/类型/模型→解析上游模型名→`keyAttempts`（每渠道最多 `maxKeyRetriesPerRoute` 次，池账号或渠道 key）→ `StartAttempt` 真发；结果由 `AttemptSpan.End` 记录；
- `maxTotalAttempts` 上限；一轮“无真实进展”则 fail-fast（`relay.go:1661`）；`retry_cache.go` 缓存重试；错误分类决定是否换 key/换渠道/熔断/冷却。

### 1.8 会话保持
`Group.SessionKeepTime>0`：`balancer.GetSticky(apiKeyID, requestModel, ttl)` 把上次渠道提到最前（`iterator.go:82-99`），`balancer/session.go` 维护 TTL + 清理，`PurgeStaleSticky` 防内存膨胀。sticky 键实为 `apiKeyID:stickyRoutingKey`（含 Group/EndpointType/模型/配置 generation），配置变化后不粘旧路由；sticky 仍会被黑名单/禁用/冷却/熔断拦截，非强制。注：`internal/relay/session.go` 不存在；`WSResponseAffinity` 目前只有表结构，未接入选路。

### 1.9 补充事实（来自逐源码核对）
- **三层选择相互独立**：`GroupMode` 只排「渠道+模型」候选；`Channel.KeySelectionStrategy` 只选同渠道 inline Key（`cost` 默认 / `availability` / `speed` / `priority`）；`AccountPool.Strategy` 只选池账号（`round_robin`/`random`/`least_loaded`/`ewma` 默认，EWMA = `0.7*errorRate + 0.3*(ttft/10000)`，再减 `weight*0.001`、`priority*0.001`，可选 reset/quota 因子）。
- **条件路由是“选中组之后的过滤”**：`condition.Evaluate` 为 false 或 JSON 非法直接 404，不是“在多组之间按条件择优”。
- **错误四类 scope**：`ScopeNone`（不重试，如 400）/`ScopeSameChannel`（401/403/429/空输出→换 Key/账号）/`ScopeNextChannel`（404/5xx/超时/网络→换候选）/`ScopeAbortAll`（已写流式/断连/插件失败/节点不确定→终止且不计健康）。默认预算：`relay_retry_count`→每候选 `N+1` 次 Key 尝试（默认 3→4）、`relay_route_retries`=2 轮、`relay_max_total_attempts` 默认 64、`ratelimit_cooldown`=300s。
- **短路面**：语义缓存可短路直返；非流式安全请求可 singleflight 共享在途结果；可恢复 `relayStreamSession`（断线重连/事件重放）是会话恢复，**不是选路 sticky**。
- **model_normalize ≠ 请求时 ModelMapping**：前者用于模型广场/名称归并，后者才是请求时模型名→组的映射。

---

## 2. magpie 的路由模型

### 2.1 数据模型（`internal/provider/`）
- **Provider**（`provider.go:50+`）：`Keys`/`Accounts`、`Routing`(smart 默认/order/rotate/usage/pace/weight/drain…)、`Affinity`(auto/session/turn/off/strict)、`Sink`、`Fallback []string`、`AccountWeights`、`MaxConcurrency`、`KeepLogin`、`SwitchOnly`、`Unlisted`、`PriceAt`/`FreeModel` 等。
- **Group**（`provider/group.go:168-288`）：`Members []string`（`provider/model[:effort]` 或 `group/<id>`）、`Match`(glob/`re:`正则)、`Placed`、`Routing`、`Affinity`、`Sink`、`FirstToken`、`Patience`、`Off`、`Pick`(Manual)、`Rules []Rule`、`Classifier`、`Effort`、`Context`、`Levels`、`Fast`、`Weights`、`Family`、`Unlists`、`Accounts`、`Auto`(发现组)、`Disabled`。
- **Member**（`group.go:291-323`）：解析后的成员，`Path`/`Via`(嵌套组路径)、`Provider`、`Model`、`Effort`、`Levels`、`Fast`、`At`、`Also`。
- **candidate**（`fallback.go`）：一个具体可尝试的路由（provider+model+某 key 或 account），带 `seat`/`restKey`/`vendor`。
- **Seat**（`internal/routes/routes.go:22-89`）：策略输入，字段含 `Key/Account/Known/Learns/Used/Soon/Lapse/Pace/Weight/Resting/Spent/Tokens/Free/Credits/First/TTFT/TPS/Left/LeftIn/Priced/Cost/CacheCost`。
- **Route/trace**（`gateway/trace.go`、`route_events.go`）：一次请求的决策记录。

### 2.2 模型解析（`docs/subsystems/gateway-routing.md` + `provider/group.go`）
`serve` 解析请求模型名，顺序要点：
1. Claude tier stand-in；2. 路由组 `provider.GroupFor`（id、模型 id、组名 slug，`group.go:876-…`）；3. auto stand-in；4. Agent 自己的 stand-in（Codex config `model` 等）；5. `provider.Resolve`。
- **auto/<model> 发现组**（`group.go:659-819`）：同一模型被多个 provider 提供时自动合成 `auto-<slug>` 组，成员按 Providers 顺序，走 affinity；用户改过的组按用户保存。
- 名称归一：`~` 拼写（`provider.Unflat`）、Claude 版本 `-`/`.` 互转（`ClaudeUndashed`）；未知模型 404 并给最近 3 个候选名（`unknownModel`）。
- `X-Magpie-Provider`/`X-Magpie-Model` 回写谁回答了。

### 2.3 候选与计划（`gateway/fallback.go`）
- `candidates → plan → planPaused`（`:483-556`）：`add(provider,model)` = `perKeyBarred`（按 key/account 生成候选，分 out/aside/left/barred）→ `weigh` → `weighed`；随后追 `Provider.Fallback`（可为路由组 `provider.FallbackGroup`，经 `planFallbackGroup`）；最后 `restLast`（休息/不可用排末尾）。
- `planGroup → planLevel`（`:600-854`）：组内成员按键/账号**合并或逐个**按组的 `Routing` 排序；嵌套组按自己的 `Routing` 走；支持 `Sink`、`Off`、`Rules`、`Classifier`、`FirstToken`、`Patience`。
- 每轮每个 `provider/model` 只试一次（避免组回环），fallback 组展开。

### 2.4 策略矩阵（`internal/routes/strategies.go` + `plugins/rotation`）
`routes.Strategy{Order func(pool, seats) []int, Exact, ByUse, ByFree, BySpeed, ByPrice}`；`Register/Find/Apply`（`routes.go:91-187`，`Apply` 校验为排列、panic 失败开放回退原序）。

| 路由名 | 依据（Seat 字段） | 实现/要点 |
|---|---|---|
| `""` Smart（默认） | Used/Soon/Lapse/Left/Credits/First/Learns | 分三档：tier0（<90%）按**窗口重置时刻**逐窗口比较（大窗口优先，重置早者先用掉将作废余量）；tier1（≥90%）按 Used 升序；tier2（≥98%）Credits 账号优先；并列比 Lapse、钱包 Left；`Learns` 未知账号先试一次 |
| `order` | 原顺序 | 第 1 个直到不能用 |
| `rotate` | 轮流 | 每 pool 计数器；新会话由 `leastHeld` 挑近 30 分钟持有最少者 |
| `usage` | Tokens（半衰 1h，`routing.go:37`） | 最近发送最少；未知份额排后 |
| `pace` | Pace（剩余/小时，未知记整周） | 重置前少浪费，10% 内同 band |
| `weight` | Weight | **NGINX 平滑加权轮询**；亲和留住时 `MoveRound` 把份额记到留住者 |
| `free` | Free（该模型在此免费） | 免费整体在前，不引入 tier |
| `fast` | TTFT + TPS（半衰 20m，`speed.go`） | 估算时延 `TTFT+200*1000/TPS`；未测速者每 5 个请求给一次探测 |
| `drain` | Spent/Used | 先用尽一个再用下一个（spent 后移到下一位并停住） |
| `cheap`（仅组） | Priced/Cost/CacheCost | 请求时刻 `PriceAt`（含高峰/费率窗口）最低；provider 侧会被 normalize 去掉 |
| `manual`（仅组） | 用户 `Pick` | 只发选中的成员，其 keys/accounts 按 Smart 排 |
| `<plugin>/<name>` | JS 策略 | `internal/middleware/points.go` 注册，20ms 限时，fail-open；插件关按列出顺序 |

`plugins/rotation`（`rotation.go`）把 `routes.Builtin` 逐个 `routes.Register` 注册；插件关则退回 order。**任何插件都可通过 `routes.Register` 增加新策略**（P3 router strategy 扩展点）。

### 2.5 亲和（`gateway/affinity.go`）
- 模式：`""`auto / `session` / `turn` / `off` / `strict`（`provider.go:145-149`）。
- 持久化 `affinity.json`（`affinity.go:283`，512 会话、24h）；记录 `stick{rest,who,model,effort,cacheRead,cacheWrite,stoodIn}`。
- 键 `stickKey = scope|conversationID[|agent:子代理]`；Claude Code 子 Agent 用 `x-claude-code-agent-id` 区分。
- auto：turn 内恒保持；跨 turn 要求 `cacheRead+cacheWrite ≥1024` 且未过 `coldAfter`（默认 5min，ChatGPT/DeepSeek 官方 1h，Anthropic 1h-cache 再续 1h）。`strict` 只留同一 `who`。
- 归位规则：`backToMain`（fallback→主力）、`backToFirst`（order 组）、`backFromStandIn`（非 order 组“顶替者归位”）。
- remote magpie：把远端 key 当候选，用 `remoteUpstream`/`sameVendor` 以远端 provider+model 保持缓存粘性。

### 2.6 失败转移与健康（`gateway/routing.go`、`upstream.go`）
- **失败分类**：`failure/failureOf` 用状态码+中英正则分出 credit/quota/rate/other/auth/verify/restricted/slow/refused/shape/prompt/effort/proxy/offline/overflow/room/loop/cut/foreign 等。
- **Rest 阶梯**（`restAfter`，`routing.go:1193-…`）：`credit` 走 `creditLadder` 30m→7d（唯一持久化到 `rests.json`）；`quota` 到 vendor 指明时间或窗口重置（上限 8d/月度 35d）；`rate` 1min 起翻倍封顶 30min；普通失败 1min 起翻倍封顶 10min；模型级降级（pooled 配额/模型未服务/远端 magpie/过载等）记 `restID`。
- **Sink**：429 但仍有额度的 key/account 排到末尾（`provider.go:137-143`）。
- **FirstToken / Patience**：流式首内容超时换成员；全员失败且未发送内容时对可重试失败者暂停后再轮。
- **连接健康**：HTTP/2 静默检测（20s ping、10s 无响应丢弃）、大 body 特例、`replayable` 连接级重试、`unfit` 记忆（本次启动内不再用某 API）。
- `restLast`（ready/busy/resting/creditsLast 四档）、`capHeld/creditsHeld`、`probeOne`（多个 resting 只让最早回来的一个参与本轮）。

### 2.7 分类器与智能路由（`gateway/decide.go`）
- `Group.Rules` + `Classifier`：`Jev`（TypeSafe decision API）或任意模型，按 `intent` 分类消息，命中先手成员（`decide.go:1-45`）。
- `Group.Effort:"auto"`：分类器判断回合难度，向模型请求相应 reasoning effort。
- `Match` 支持 `*` glob 与 `re:` 正则，动态把模型并入组（`Group.Matched`）。

### 2.8 可观测
`Route`/`trace` 记录候选、rests、谁回答、prompt 结构、耗时；`route_events.go` 事件流；GUI「Routing」页展示；usage ledger 记账；`X-Magpie-Provider`/`X-Magpie-Model` 回给客户端。

---

## 3. 逐项差异

1. **抽象层级**：octopus 的核心是“组内渠道选择”；magpie 的核心是“候选列表 + 策略排序”，组只是候选来源之一（provider 的 keys/accounts 或 group 的 members）。magpie 的 Seat 抽象把额度/速度/价格/钱包/信用都纳入策略输入，octopus 的 GroupItem 只有 priority/weight。
2. **可扩展性**：octopus 的路由算法是编译期固定的 5 种 `Balancer` 实现；magpie 是**运行时注册表**，插件（`plugins/rotation` 或第三方）可新增任意策略，且策略失败“失败开放”回退原序。
3. **健康处理**：octopus 用显式**熔断器状态机**（含半开探测、指数退避）+ 独立 Key 冷却；magpie 用**按原因的 Rest 时长**（更贴合额度/限流语义）+ Sink + FirstToken/Patience + HTTP2 探测。
4. **额度语义**：magpie 深度建模订阅额度（`Known/Learns/Used/Soon/Lapse/Pace/Left`、窗口重置、钱包余额、信用户），Smart 策略按“最早重置 / 剩余最多”排序；octopus 主要按成功率/延迟/价格/成本与权重，额度更多体现在号池账户与站点余额。
5. **会话亲和**：magpie 的 `Affinity` 五档 + 持久化 + 子 Agent/remote 细分远丰富于 octopus 的 `SessionKeepTime` 粘性。
6. **智能路由**：octopus 是**离线 AI 生成组配置**（AI Route 任务），运行期无分类器；magpie 是**在线分类器**（Jev/rules intent + Effort auto），按内容动态选成员/思考强度。
7. **分组能力**：octopus 组不嵌套；magpie 支持组嵌套、fallback 组、`Match` 动态成员、`Rules` 规则。
8. **号池**：octopus 有独立账号池调度器（并发槽、优先级分层、EWMA、鉴权错误计数、限流/过载冷却）；magpie 的 account 只是候选，没有独立池调度器，但候选级 control（并发 `MaxConcurrency`）。
9. **模型识别/价格**：octopus 自建 model-mapping/normalize/price-category/schedule + 模型市场；magpie 依赖 models.dev catalog + 本地 id/wire 映射 + 价格窗口/免费判断。
10. **失败转移结构**：octopus 组内遍历 + route round + retry cache + fail-fast；magpie `Provider.Fallback`（可指组）+ `planLevel` 递归 + 每轮每模型一次。

---

## 4. 选路全流程对比

**octopus（`/v1/chat/completions`）**
1. `middleware.APIKeyAuth()` 校验 key，注入 api_key_id。
2. `relay.Handler` 解析请求 → 按模型/端点找 `Group`（MatchRegex/Condition）。
3. `applyBeforeRoutePlugin`（插件可改请求）→ route round 开始 → `applyRouteFilterPlugin`（插件可裁剪组项）。
4. `balancer.NewIterator`：按 `GroupMode` 生成候选顺序 → 剔除黑名单 → 一次性渠道前置 → 粘性渠道置顶。
5. 遍历候选：取 `Channel` → 校验启用/类型/模型 → 解析上游模型名 → 选 key（`KeySelectionStrategy`）或池账号（`poolscheduler.SelectAccountLease`）→ `SkipCircuitBreak`/`Skip` 过滤 → `StartAttempt` 发上游。
6. 失败：按错误分类换 key / 换渠道 / 记冷却 / 触发熔断；无进展 fail-fast。
7. 成功：`AttemptSpan.End` 记录 → metrics/日志 → 返回；粘性会话记录。

**magpie（`POST /v1/chat/completions`）**
1. `gateway.Server.Handler` 中间件链（cors/caller/readOnly/keyLimited…）。
2. `serve` 解析模型：`GroupFor`/auto 发现组/stand-in/`Resolve` → 得到 provider/model 或 group。
3. `plan`（`fallback.go`）：生成 candidate 列表（该 provider 的每个 key/account × model，按 `keyFit` 排序；再追 `Fallback`，组则 `planGroup/planLevel` 递归）→ `restLast`。
4. `weighRouted`（`routing.go`）：把候选读成 `routes.Seat`，读取需要的额度/速度/价格数据 → `routes.Find(routing)` 取策略 → `routes.Apply` 得顺序。
5. `affine`（`affinity.go`）按会话粘性调整顺序；`Sink` 处理 429。
6. 逐个候选 `attempt`：经代理/账号发送；首 token 超时（`FirstToken`）或失败 → 记录 Rest → 换下一个；全员失败看 `Patience`。
7. 成功：写 trace/route events/usage；回写 `X-Magpie-Provider/Model`；`noteSticks` 记录亲和。

---

## 5. 证据（关键文件:行号）

**octopus**
- `internal/model/group.go:30-36`（GroupMode）、`:38-80`（Group/GroupItem）、`internal/model/channel.go:82-160`（Channel/ChannelKey）、`internal/model/auto_group.go`
- `internal/relay/balancer/balancer.go:22-208`（5 种模式）、`weighted.go:35-135`（Smooth WRR）、`iterator.go:51-208`（候选编排/跳过/尝试）、`circuit.go:16-70`（熔断）、`key_cooldown.go`、`key_availability.go`、`key_speed.go`、`auto_shuffle.go`、`group_rotation.go`
- `internal/relay/poolscheduler/scheduler.go:98-147`（选号/上报）、`:448-…`（sticky/过滤/策略）、`lease.go`、`auth_error_counter.go`、`ratelimit_headers.go`
- `internal/relay/relay.go:1666-1790`（组过滤/迭代/池选号）、`:2014`（ForwardedAttempts）、`retry_helper.go`、`retry_cache.go`、`media_relay.go:184`
- `internal/relay/condition/evaluator.go:12-128`、`internal/helper/ai_route.go`、`internal/model/ai_route*.go`、`internal/op/airoute`
- `internal/model/setting.go:30-37,62-69,155-159`（熔断/冷却/Auto 策略默认值）

**magpie**
- `internal/routes/routes.go:21-187`（Seat/Strategy/Register/Find/Apply）、`strategies.go:16-…`（Smart/ByLeft/tier 等）、`internal/routes/fastest.go`
- `internal/gateway/fallback.go:483-620`（plan/planGroup/planFallbackGroup）、`:221-466`（perKey/keyFit）、`:657-854`（planLevel）
- `internal/gateway/routing.go:35-…`（usageHalfLife、Rest、served）、`internal/gateway/affinity.go:283-…`（affinity.json/stickKey/affine）
- `internal/gateway/decide.go:1-45`（Jev 分类器）、`internal/gateway/speed.go`、`internal/gateway/trace.go`、`internal/gateway/route_events.go`
- `internal/provider/group.go:168-323`（Group/Member）、`:876-…`（GroupFor）、`:659-819`（auto 发现组）、`internal/provider/provider.go:50-175`（Provider 字段）
- `plugins/rotation/rotation.go:19-49`（策略注册）、`docs/subsystems/gateway-routing.md`（权威说明）

---

### 附：未确认项
- octopus 的 `Channel.Type` 与出站适配（`internal/transformer/outbound`）如何逐一影响具体协议转换，本对比只概括到“按类型过滤/转换”，未逐适配器展开。
- magpie `planLevel` 的全部细节（4000+ 行 `fallback.go`）与各失败分类的精确时长表，本对比按 `gateway-routing.md` 与源码注释归纳，可能有未覆盖的边角。
