# magpie 路由模型 / 请求路由机制调查

> 调查对象：`D:\CodeWorkspace\magpie`（只读）。本报告覆盖：一个推理请求进入本地网关后，模型名如何被解析、候选（provider/key/account）如何生成与排序、策略如何挑选、失败如何转移与记忆、会话亲和、插件策略、remote-magpie 与可观测性。
> 日期：2026-11-03。行号基于当日工作区快照。

---

## 1. 概览

magpie 的网关（`internal/gateway`，默认 `127.0.0.1:3425`）对外提供 OpenAI Chat Completions、Responses、Anthropic Messages、Gemini 四种协议。一次请求的路由分为五层：

1. **模型解析（resolution）**：`serve` 把请求体里的模型名经替身（stand-in）、组归一（`GroupFor`/`CanonicalRef`）、`provider.Resolve` 解析成一个 provider+model，或一个路由组（routing group）。
2. **候选与计划（plan）**：`plan`（单 provider，含其 `Fallback` 链）或 `planGroup`（组）展开为若干 `candidate`（一个 key 或一个账号×模型），并产出 `planned`（`order`/`left`，即路由轨迹里“按顺序/被留下”的清单）。
3. **排序（routing）**：`weighRouted` 把每个 candidate 折成 `routes.Seat`，交给该池的路由策略（`routes.Find` → Smart 或插件注册的 Builtin/JS 策略）排序；之后依次叠加 `restLast`（resting/busy/creditsLast 置后）、`sink`、`probeOne`（resting 只试最早回来的一个）、`roomy`（窗口装不下不问）、`behindOverflow`（本会话被拒过长的排后）、`pinTo`（X-Magpie-Account 固定）。
4. **亲和（affinity）**：`affine` 按会话把上次应答者提前（auto/session/turn/off/strict），并按 backToMain/backToFirst/backFromStandIn/creditsLast/busy 等规则把“临时顶替”的会话还回主力。
5. **尝试与失败转移（attempt loop）**：`serve` 的候选循环对每轮每个 provider/model 只试一次（`attempt`），失败经 `holdWriter` 判定是否已发送字节；未发送则失败转移，`restAfter` 按错误分类（quota/credit/rate/other/auth/…）给候选设置 rest；全部失败且都是“可过性”错误时按 Patience 重排（`replan`）。

关键文件地图：

| 层 | 文件 |
|---|---|
| 策略库（Seat/Strategy/Smart/Builtin） | `internal/routes/routes.go`、`strategies.go`、`round.go`、`fastest.go` |
| 排序与 rest/失败分类 | `internal/gateway/routing.go`（weighRouted、restAfter、Rest、creditLadder） |
| 候选与计划 | `internal/gateway/fallback.go`（candidate、perKeyEvery、plan/planPaused/planFallbackGroup/planGroup/planLevel、restLast、probeOne、roomy） |
| 亲和 | `internal/gateway/affinity.go`（affine、stickKey、strictKeep、leastHeld、spreadsSessions） |
| 主流程 | `internal/gateway/gateway.go`（handle→serveAgent→serve→候选循环→attempt） |
| 决策（System One/Jev） | `internal/gateway/decide.go`、`decide_group.go` |
| 规则与分类器 | `internal/gateway/rules.go`、`classify.go` |
| 模型解析与组 | `internal/provider/models.go`（Resolve/resolveIn）、`group.go`（GroupFor/FindGroup/membersFound/ResolveRoute）、`autoref.go`（CanonicalRef/PublicRef/auto 组） |
| 速度测量 | `internal/gateway/speed.go` |
| 轨迹 | `internal/gateway/trace.go`（Route/Try/Weighed）、`route_events.go`、`session_route.go` |
| 并发/RPM | `internal/gateway/concurrency.go`、`rpm.go` |
| 回复命名 | `internal/gateway/member.go` |
| 重排/耐心 | `internal/gateway/patience.go` |
| 后台探测 | `internal/gateway/rest_probe.go` |
| 中间件策略注册 | `internal/middleware/points.go`、`plugins/middleware/middleware.go` |
| 内置策略注册 | `plugins/rotation/rotation.go` |
| 用户组开关 | `plugins/groups/groups.go`、`internal/provider/group_serve.go` |
| GUI 组 API | `internal/gui/groups.go` |

---

## 2. 数据模型

### 2.1 Provider / Key / Account（`internal/provider`）

- **`provider.Provider`**（`internal/provider/provider.go:50`）：一个供应商。路由相关字段：
  - `ID`、`Name`、`Preset`（`remote-magpie` 预设见 `provider/remote_magpie.go:69`）、`Key`（第一把 key）、`Keys []KeyAccount`（其余 key）、`KeyName`/`KeyProtocol`/`KeyWeight`/`KeyPlan`。
  - 端点：`Chat`/`Responses`/`Anthropic`/`Gemini`/`Decide`/`BaseAPI`。
  - **`Routing string`**（provider.go:135）：路由名，`""`=smart；常量 `Ordered="order"`、`Rotate="rotate"`、`LeastUsed="usage"`、`Pace="pace"`、`Weighted="weight"`、`FreeFirst="free"`、`Fastest="fast"`、`Cheapest="cheap"`、`Drain="drain"`（`provider/routing.go:25-52`），另有 `Manual="manual"`（仅组）。
  - `Sink bool`、`Affinity string`、`Fallback []string`（fallback 链，`provider/model` 或 `group/<id>`）。
  - 并发与限流：`MaxConcurrency *int`、`MaxAccounts *int`、`AccountConcurrency`、`QueueLimit`、`QueueWait`、`MaxRPM`、`QuotaRest`、`QuotaResetAt`、`RestCap`、`LimitRestOff`。
  - 价格：`PriceRate`、`CountFree`、`RateWindows`；`FreeModel`（models.go:1892）、`PriceAt`（models.go:2486）。
- **`provider.KeyAccount`**（`internal/provider/keys.go:24`）：一把保存的 key。`Name`、`Key`、`Off`、`Protocol`（仅限某协议）、`Weight`（weight 路由份额）、`Plan`（key 专属套餐/区域）。`KeyID` 是 key 的 sha256 前 5 字节指纹（keys.go:103-109），用作 rest 标识的一部分。
- **Account（订阅账号）**：不是独立结构，而是 `Provider.Account *Account`（见 `internal/provider/account.go`），一个订阅 provider 每开一个账号就是一个 candidate；`AlsoOn()` 列出该订阅所有开着的账号（fallback.go:309-319）。`Account.Agent`（codex/claude/copilot/antigravity/…）、`User`、`Plan`、`Lists`（账号自己的模型清单）。

### 2.2 Group / Member（`internal/provider/group.go`）

- **`provider.Group`**（group.go:168）：路由组。字段：`ID`、`Name`、`Members []string`（`provider/model[:effort]` 或 `group/<id>`，可含 `Match`/`Matched`/`Placed` 模式成员）、`Routing`（同 Provider.Routing，另允许 `Manual`）、`Affinity`、`Sink`、`FirstToken int`（首内容超时秒）、`Patience int`（重排预算秒，`-1`=PatienceOff）、`Off`、`Pick`（Manual 选中成员）、`Rules []Rule`、`Classifier string`（意图分类器模型/组）、`Effort`（"auto" 用分类器定推理强度）、`Context`、`Levels`、`Fast`、`Weights map[string]int`、`Unlists`、`Accounts []string`（组被限定到某些账号，`<provider>/<stable id or key fingerprint>`）、`Auto bool`（自动发现组）、`Spelled`（auto 组的公开拼写）、`Hidden`、`Disabled`。
  - `Live()`（group.go:115）：Manual 组规则不生效。
  - `Within(in)`（group.go:129）：嵌套组未设 routing 且外层是“池化”路由（`pools`：非 order/weight/manual，group.go:138-140）时继承外层 routing。
  - `Waits()`（group.go:153）：Patience 预算；`PatienceDefault=60s`、`PatienceLongest=240s`、`PatienceOff=-1`（group.go:146-148）。
- **`provider.Member`**（group.go:291）：解析后的一名成员。字段：`ID`、`Path []string`（从顶层成员到模型的路径）、`Via []Group`（所经嵌套组，外层在前）、`Provider`、`Model`、`Effort`（固定推理档，`provider/model:low`）、`Levels`（可选档位 `low+high`）、`Fast`、`At`（遍历序）、`Also []Member`（同一 provider+model+effort 在更后位置的重复出现，只问一次）。
- **`provider.Entry`**（models.go:2742）：目录条目（provider/model 组或组），供 `/v1/models` 与解析用。

### 2.3 Routing（路由名）与策略

- 路由名即 `Provider.Routing` / `Group.Routing`。核心内置 `routes.Smart`（`""`）；其余由内置插件 `plugins/rotation` 把 `routes.Builtin` 逐个 `routes.Register`（rotation.go:26-31）：
  `order→Ordered`、`rotate→Rotate`、`usage→LeastUsed`、`pace→Pace`、`weight→Weighted`、`free→FreeFirst`、`fast→Fastest`、`cheap→Cheapest`、`drain→Drain`（strategies.go:508-518）。
- `routes.Find(routing)`（routes.go:142）：`""` 恒为 Smart；否则取注册栈顶；未注册时 `routingOf`（gateway/routing.go:2040-2045）回落为 `order`。
- **`routes.Seat`**（routes.go:22-89）：策略看到的“席位”：`Key`（rest key）、`Account`（是否订阅账号）、`Known`（allowance 已知）、`Learns`（Claude 账号边答边学）、`Used`（窗口用量份额 0-100）、`Soon []time.Time`（窗口重置，大窗口在前）、`Lapse`（最早到期时刻）、`Pace`（每周剩余/小时）、`Weight`、`Resting`、`Spent`（因 quota/credit 而 rest）、`Tokens`（近期服务 token，1h 半衰）、`Free`、`Credits`（Codex 账号可花 credits）、`First`（credits-first 设置）、`TTFT/TPS`、`Left/LeftIn`（预付钱包余额及单位）、`Priced/Cost/CacheCost`（价格）。
- **`routes.Strategy`**（routes.go:92-116）：`Order(pool string, seats []Seat) []int` 返回下标排列；`Exact`（不做事后 keyFit 微调）；`ByUse/ByFree/BySpeed/ByPrice` 标志位，决定 `weighRouted` 是否为此填 Tokens/Free/TTFT+TPS/Priced。`routes.Apply`（routes.go:165）校验排列合法性，panic 或非法则保持原序（fail-open）。

### 2.4 Candidate / planned / Weighed / Route / Try（`internal/gateway`）

- **`candidate`**（fallback.go:42-90）：一次可尝试的对象。字段：`p provider.Provider`（带单 key 的副本或带账号的副本）、`model`、`rest`（rest 标识：provider id，或多 key 时 `id#<KeyID>`，账号时 `id@user`）、`effort`/`levels`（组成员固定档/可选档）、`fast`/`ultra`、`rank`（用户排序位）、`capped *capHold`（usage cap 暂扣）、`noURL`、`seats []wrrSeat`（加权轮中的位置）、`firstToken`、`patience`（继承最内层组）、`member`/`place`（在组里的成员名与位次，用于 standsInFor）、`reserve`（MaxAccounts 后备标记）。
  - `who()`（fallback.go:165）：key 为 `id#<KeyID>`、账号为 `id@user`——并发/限流以它为键。
  - `restKey()`（fallback.go:124）：账号恒为 `id@user`（不受代理切换账号影响）。
  - `restID()`（fallback.go:146）：`restKey()+"/"+model`——模型级 rest（OpenRouter 免费池、pooled 配额、modelRefused、remote-magpie 模型、ZCode trial、modelCooledDown、PaidBesideFree）。
  - `vendor()` / `sameVendor()`（fallback.go:200-213）：remote-magpie 下为 `who()+">"+上游provider id`，亲和按 vendor 记。
- **`planned`**（trace.go:489-505）：计划结果。`order []Weighed`（按序）、`left []Weighed`（被留下的）、`held []candidate`（因不花 credits 被扣）、`capped []candidate`（usage cap 被扣）、`unlisted`/`unlistedW`（账号清单缺该模型，留作最后手段）。
- **`Weighed`**（trace.go:275-388）：轨迹里的一行“被称重者”：`ID`（rest key）、`Provider`、`Who`、`Kind`（account/key/provider）、`Used`、`Renews`、`Pace`、`Tokens`、`Free`、`LateTTFT/LateTPS`、`Price`、`Turn`（rotate 轮到）、`Fit`（keyFit）、`Rest`、`Window`（roomy 留下的窗口）、`Capped`、`CapBack`、`NoCredits`、`Unread`、`Credits`、`CreditsLast`、`CreditsFirst`、`Busy`、`Rank`、`Aside`、`Reserve`、`Via`、`Sunk`、`Fallback`、`Unlisted`、`Barred`、`Held/HeldBy`、`Balance` 等。
- **`Route`**（trace.go:48-175）：一次请求的完整轨迹：`Model`（agent 所问）、`Provider`、`Group *GroupRef`、`Rule *RuleHit`、`Nested`、`Affinity`、`Order`/`Left`、`Tries []Try`、`Moved *ModelMove`（替身改写）、`Subagent`、`Compaction`、`Pinned`、`Served`/`Swapped`/`Routed`、`Prompt`、`Folder`、`Archive` 等。
- **`Try`**（trace.go:391-477）：一次尝试：`ID`、`Model`、`Effort`、`Fixed`、`RuleEffort`、`Fast`、`Status`、`Millis`、`TTFT`、`Fail`（失败分类）、`Rest`（之后 rest 信息）、`Again`（同一 candidate 重试等待 ms）、`Replan`/`Patience`、`Queued`（等槽 ms）、`Reset`（自动花了 Codex reset）、`Credits`（creditsAgain 重问）、`Auto`（Copilot Auto 的挑选链）、`Earlier`（最终返回给用户的是更早的非配额错误）。
- **`Rest`**（routing.go:658-713）：一条 rest：`Why`（failCredit/failQuota/failRate/failOther/failAuth/failVerify/failRestricted/failSlow…，routing.go:373-444）、`Status`、`Until`、`By`（"credit"/"retry-after"/"resets"/"window"/"quota"/"cooldown"/"backoff"/"restricted"/"verify"/"off"）、`Failures`、`Key`、`Model`、`Link`、`Probed`、`Error`，及内部字段 `agent/user/said/hold/first/askAt/there`。
- **`Affinity`**（affinity.go:99-132）：轨迹的亲和记录：`Mode`、`Turn`、`Within`、`Last`（上次应答者）、`CacheRead/CacheWrite`、`Kept`、`Why`（"session"/"turn"/"cache"/"off"/"first"/"new-turn"/"no-cache"/"cold"/"resting"/"spent"/"gone"/"main"/"order"/"back"/"credits"/"busy"/"strict"/"reserve"）、`Main`（strict 下本可回主力）。

---

## 3. 模型解析流程（serve 如何把模型名解析成 provider/model/group）

主入口：`Handler` 路由到 `handle(proto)`（gateway.go:1790）→ `serveAgent`（middleware.go:23，JS 中间件链 `gwmw`）→ `serve`（gateway.go:1980）。解析顺序（gateway.go:2018-2262）：

1. **预处理**：`offAsNone`（"off"→"none"）、`heicAsJPEG`、`withWires`（读 `settings.Wires()`：用户自定义 `ModelIDs`/`ModelWires` 的线上名映射）。
2. **model:effort 后缀**：`askedAt`（gateway.go:2020）拆出 `<model>:<level>`，改写 body 的模型名。
3. **替身链**（顺序执行，命中即止）：
   - `classifierStandIn`（Claude Code auto 模式分类器跟随主模型）；
   - `claudeTierStandIn`（Claude 层级 opus/sonnet/haiku/fable 替身）；
   - **`keyGroupFor` / `provider.AutoStandIn`**（组名直查与 auto 替身；gateway.go:2156-2166 `grouped()`）；
   - `standIn(agent, asked)`（agent 配置的替身，如 Codex config.toml 的 `model`，WSL 下按发行版；之后再过一遍 `grouped` 使裸组名可达）；
   - `codexMemoryStandIn`（Codex 记忆/auto-review 调用回落到该会话上一轮所用模型）。
   - 每次替身改写都记 `Route.Moved{To,Why}`（`move()`，gateway.go:2111-2115）。
4. **桌面/子代理改写**：`desktopMove`（Claude Desktop 各场景替身）、`scienceTurn`（Claude Science）、`codexSubagentPick`/`desktopSubagentPick`（子代理模型替换，`Route.Subagent`）。
5. **`provider.Resolve(asked)`**（models.go:3145）：
   - 若是组引用（`IsGroupRef`：`group/<id>` 或 `auto/<model>`，经 `CanonicalRef` 归一到 `group/auto-<slug>`）：在组条目里找第一个可用成员。
   - 否则 `resolveIn`（models.go:3165）：①精确匹配目录 id；②`provider/model` 形式且 provider 开着；③裸模型 id——取 `All()` 顺序里第一个暴露它的 provider（无 fallback）；④只有一个 provider 列出（未暴露）时用之。
   - **裸 id 多 provider 服务时**：serve 在 Resolve 前已用 `GroupFor`（group.go:876）查过——`groupForIn` 依次试 `lower(id)`、`Slug(sameModel(id))`、`auto-<slug>`（自动发现组），再 `groupNamed`（唯一组名匹配）；找到则整个请求走组（组开着 found groups 时；关掉则 `AutoStandIn` 直接给单 provider）。
   - **`auto/<model>` 拼写**：`CanonicalRef`（autoref.go:70）把 `auto/<model>[:level][1m]` 归一为内部 id `group/auto-<slug>`；`PublicRef`（autoref.go:136）反向；拼写来源 `spelledIn`（autoref.go:174，优先 catalog.Spelling）。
   - **flat `~` 拼写**：`FlatID`/`Unflat`（flat.go:15-42）：MiniMax Code 的 `custom_provider:magpie` 下单斜杠限制，`bb-codex~gpt-6.1-sol` 可还原。
   - **未知模型**：404。区分：provider 被关（`SwitchedOff`）、组被关（`DisabledGroup`）、决策组（`Decides`→400 指向 `/v1/systemone`）、空组（`emptyGroupError`）。
6. **决策模型闸门**：`p.DecidesModel(model)` → 400 `decisionModelRefused`（gateway.go:2256-2262）。
7. **禁用模型**：`idOff` → 403（gateway.go:2220-2225）。
8. **组的成员展开**：`provider.FindGroup(asked)`（group.go:825 → `GroupFinder`）→ `membersFound`（group.go:1004）递归遍历嵌套组（`maxNest` 上限、去环），每个成员 `resolveIn` 解析，`unpicked`（group.go:1056，用户在 provider 模型清单里取消勾选）剔除，重复 provider+model+effort 合并进 `Also`。`g.Live()` 应用 Manual。
9. **脱敏**：`redactedHits`（fails closed；`maskRouteFor` 可把“会被改写”的请求整体改投用户指定的免脱敏模型）。
10. **gateway key 限制**：`keyHolds`/`groupAllowed`（组）、`modelAllowed`（模型），不符 403（gateway.go:2309-2317）。
11. **暂停规则**：`provider.PausedOut`（pause 规则按 hours/days/agents/folders 剔除成员；全暂停 503）。
12. **sealed task 闸门**：Codex 子代理密封任务只允许到能读密封的账号（leadProvider/leadFirst）。
13. **组规则与分类器**：`ruleFor`（rules.go，turn 开始时判定一次并保持整轮；分类器经 `classify.go` 或决策模型 `askJev`，超时 `classifyTimeout()` 默认 8s，全请求共享一次等待 `askWaited`）。

---

## 4. 候选与计划（plan / candidates / Fallback / planGroup / planLevel）

### 4.1 单 provider：`plan` → `planPaused`（fallback.go:490-556）

- `perKeyBarred`（fallback.go:240-243）= `perKeyEvery` + `keepAtOnce(p.AtOnce())`。
- **`perKeyEvery`**（fallback.go:305-438）按序产出候选：
  - 模型被用户禁用（`ModelDisabled`）→ 空。
  - 订阅账号：signed-in 账号（除非 `OwnPaused`）+ `AlsoOn()` 的其它账号；`LoginRanks` 重排；`AccountServes`（账号级模型黑名单，`AccountModels`）剔除到 `barred`；`capHeld`（usage cap / Codex no-credits 100%）剔除到 `barred`（记入 `c.capped`）；账号自己的模型清单 `Account.Lists(ownModelID)` 分出 `out`/`left`（`left`=unlisted，最后手段）。
  - key：`KeysOn()` 遍历，`p.WithKey(k)` 生成单 key 副本；协议无 URL（`Speaks()` 空且非决策模型）跳过并记 `noURL`；`AccountServes(KeyID)`、`PlanServing`（key 专属套餐缺模型→planless）、`Serving`（该 key 的清单）分出 out/unlisted；`keyFit`（0 恰好/1 需翻译/2 协议不合）排序后，与首 key 同 `KeyProtocol` 的进 `pool`，其余进 `aside`（其它协议的 key 排后）。out 全空时依次回退 unlisted→planless→barred→整 provider（带 noURL 标记）。
- **`keepAtOnce`**（fallback.go:260-301）：MaxAccounts>0 时按用户 rank 保前 N（优先非 resting），其余置 `reserve` 标记排到 aside；reserve 只在保有者全失败或 rest 后用到（laneMate/waitMate 不外溢到 reserve，`dropReserves`）。
- **`weigh`**（routing.go:2027）对每个 provider 的候选排序（见 §5）。
- **Fallback 链**：`p.Fallback` 逐项：
  - 组引用：`provider.FallbackGroup`（group.go:833）→ `planFallbackGroup`（fallback.go:562-586）：组成员按组自己的 routing/sink 计划，trace 行标 `Fallback` 且 `Via` 前置组 id；**不应用组的规则/分类器**（计划在 primary 尝试前做出）。
  - 模型引用：`provider.ResolveRoute`（group.go:1064，会跳过 unpicked）→ 递归 `add()`。
  - 去重：`seen` 按 `providerID/model`，同一 provider/model 每轮只试一次（防组包含本 provider 造成循环；组成员自己的 fallback 不跟进）。
  - `CapStop`：所有主账号被 cap 扣留且开 `CapStop` 时直接 429，不进 fallback。
- **`restLast`**（fallback.go:1111-1144）：把候选分四档排序——ready、busy（`busyOf`：插件上报负载 > `settings.BusyLoad`）、resting（`restingOf`）、creditsLast（Codex credits-last 且窗口已用尽）；单候选时即使 resting 也照试。

### 4.2 组：`planGroup` / `planLevel`（fallback.go:600-846）

- `planGroup`（fallback.go:600-615）→ `planLevel` → 可能 `sinkPlanned`（组自身 Sink）→ 追加 asides → `placed`（记录每个 candidate 的成员位次）→ `restLast`。
- **`planLevel`**（fallback.go:657-846）按 `routingOf(g.Routing)` 分两路：
  - **非 order**（smart/rotate/usage/pace/free/fast/cheap/drain/插件）：把成员组织成 unit——嵌套组递归 `planLevel` 自成一 unit（按 `firstAwake` 的头一个代表参与外层称重）；`ownRouting`（provider 自定义 routing 且与组不同）的 provider 整体成一 unit；`byWeight` 时每成员一 unit 过 `weightedUnits`（加权轮）。其余成员的每个 key/account 单独成 unit。之后用组的 routing 对 heads 统一 `weigh`，展开为最终顺序；trace 行记 `Routing`/`Via`/`Turn`。
  - **order**：逐成员按出现顺序展开；嵌套组同样递归并按自己的 routing/sink。
- **Manual**：`routing` 归一为 `""`，被选成员的 keys/accounts 按 smart 排（fallback.go:792-795）。
- **`sinkOwn`**（fallback.go:854-859）：嵌套组自己的 Sink 先在自己序内生效，再轮到外层。

### 4.3 组内组的亲和与“顶替”

- `placed` 给每个 candidate 记录 `place`（成员在顶层组的位次）。`standsInFor`（affinity.go:751）：某候选应答时，位次在前的成员处于 resting 或用量≥`SpentShareOf` 且无 CreditsFirst，则它是“顶替者”，记入 `stick.stoodIn`；新 turn 时 `backFromStandIn`（affinity.go:737）把会话还给位次更前的就绪成员（Why="back"）。
- order 组：`backToFirst`（affinity.go:719）：新 turn 时若首个就绪候选属于位次更前的成员（不同 member id），归还（Why="order"）。
- provider fallback：`backToMain`（affinity.go:709）：`order[at].Fallback && !order[0].Fallback && order[0].Rest==nil` → Why="main"。

### 4.4 计划后的全局过滤（serve 中，gateway.go:2562-2821）

依次为：`sealedReaders`（sealed task）、`checkpointReaders`/`compactionCarriers`（原生 compaction 只去封存者）、`allowedCandidates`（gateway key 的模型/账号限制）、`strictKeep`（strict 会话把被 cap 扣的账号拉回，或整体 held 429）、`autoReset`（全部被扣且有不花 credits 的账号时自动花一次 reset）、`affine`（亲和，见 §6）、`leadFirst`（sealed task 的 lead 账号提前）、`ruleOver`/`nestedRules`（规则命中成员提前）、图像过滤（text-only 成员排除或图片描述）、`pinTo`（X-Magpie-Account 固定，不存在→403/404）、`roomy`（窗口装不下请求的 member 移入 `small`，仅当所有有窗口的候选都 quota/credit 用尽才作为最后手段问）、`behindOverflow`（本会话曾被拒过长的 member 排后）、`probeOne`（多个 resting 只保留最早回来的一个参与本轮；孤例保留）。

---

## 5. 策略矩阵

| 路由名 | 显示名 | 策略实现 | 指标（Seat 字段） | 标志位 | 语义要点 |
|---|---|---|---|---|---|
| `""` (smart) | Smart | `routes.Smart`（strategies.go:130-180） | Used/Soon/Lapse/Learns/Left/Credits/First | — | 分三档：tier0 有房间、tier1 低量（≥`LowShare`=90%）、tier2 将尽（≥`SpentShare`=98%）。tier0 内按窗口重置时刻逐窗口比较（截断到小时，大窗口优先），重置早者先（先用掉将作废的余量）；重置并列比 `Lapse`；再并列比钱包余额 `byLeft`；tier1 内按 Used 升序；tier2 内 `spentBefore`（Credits 账号优先，再 Used 升序）。`Learns`（Claude 未知账号）排最前一次以便被测量。 |
| `order` | In order | `routes.Ordered`（strategies.go:193） | — | Exact | 恒等排列；第一个直到不能用。 |
| `rotate` | In turn | `routes.Rotate`（strategies.go:264-270） | — | — | 每 pool 一个计数器，每请求前进一格。新会话由 `leastHeld`（affinity.go:796）挑“近 30 分钟持有会话最少”的（`heldFor`=30min）；turn 内由 `after`（affinity.go:935）转到下一个非 resting 非 fallback 非 busy 非 creditsLast。 |
| `usage` | Least used first | `routes.LeastUsed`（strategies.go:283-338） | Used/Tokens/Left/Credits | ByUse | Learns 最前；然后“有份额记录且有房间”的（Used>0 或 Soon 非空），按 Used 升序，并列按钱包余额、再按 Tokens（1h 半衰）；未知份额者排在其后（避免免费烧付费，#4535）；低量/将尽按 tier+spentBefore。 |
| `pace` | Use before expiry | `routes.Pace`（strategies.go:359-411） | Pace/Used/Credits/Left/Tokens | ByUse | 按“窗口剩余/距重置小时数”（Pace，未知账号记整周 `FreshPace`=100/168，routing.go:550）分档（相差 10% 内同 band 保持原序）；tier 规则同 Smart。 |
| `weight` | By weight | `routes.Weighted`（strategies.go:416-422）+ `Round`（round.go:27-62） | Weight/Resting | — | NGINX 平滑加权轮询；resting 临时移出轮；亲和留住时 `MoveRound`/`weightedKept` 把份额记到实际留住者（weighted.go:59-70）。组级按成员 `Group.Weights`。 |
| `free` | Free first | `routes.FreeFirst`（strategies.go:452-462） | Free | ByFree | Free 的席位（`provider.FreeModel`：CountFree 或清单标 Free 或价格为 0）整体在前，各自保持列出顺序；不引入 Smart 的 tier。 |
| `fast` | Speed first | `routes.Fastest`（fastest.go:60-125） | TTFT/TPS/Resting/Used | BySpeed | 按估算时延 `Estimate(TTFT,TPS,mid)`=TTFT+200*1000/TPS（`RefTokens`=200，fastest.go:39）升序；band 10%（`alike`=1.1）内保持原序；未测速者排后，但每 `ProbeEvery`=5 个请求给一个探测位（轮流）；resting 在 ready 后、spent 最后。速度来自 `speed.go`（半衰 20min，过期 1h）。 |
| `cheap` | Cheapest first | `routes.Cheapest`（strategies.go:478-494） | Priced/Cost/CacheCost | ByPrice | 仅组可用（`provider.normalize` 会把 provider 上的 cheap 去掉）。按请求时刻价格 `PriceAt`（含高峰/费率窗口）的输入+输出百万 token 成本升序，并列比 CacheCost，`sameCost`（1e-6 容差）保原序；无价者最后。 |
| `drain` | Until used up | `routes.Drain`（strategies.go:236-256）+ `Drained` | Spent/Used/First/Known | Exact | 每 pool 记住当前席位；当前席位 spent（`Seat.Spent` 或 `Used≥SpentShareOf(Drain)=100` 且非 First）或被 `routes.Drained(key)`（quota/credit rest 时由 restAfter 调用，routing.go:1573）标记后，顺序移到下一位并停在那里；全 spent 时原地不动。 |
| 插件 `<plugin>/<name>` | JS 策略 | `internal/middleware/points.go` 注册 | seatJSON（key/account/known/learns/used/soon/pace/weight/resting/tokens） | — | `strategies[name]` 函数或 `{order}` 对象，20ms 限时，fail-open 保持原序；路由名 `<shortName>/<name>`（points.go:182-196）；providers.json 中保留（`provider.PluginRouting` 正则，routing.go:56-62），插件关掉时按列出顺序。 |
| `manual`（仅组） | Manual | planLevel 特判（fallback.go:792-795） | — | — | 组只发 `Pick`（或第一个）成员；其 keys/accounts 按 Smart 排。 |

公共规则：

- **tier 阈值**：`LowShare`=90（strategies.go:17），`UsedShare`=`provider.SpentShare`=98（codex_switch.go:116）；`SpentShareOf(routing)` 对 order/drain 返回 100，其余 98（codex_switch.go:122-127）。
- **窗口比较**：Smart 只看“计入该模型的窗口”，大窗口（如周）优先，重置时间截断到小时（weighRouted 中 `Allowance.Renewal` → `Seat.Soon`）。
- **`weighRouted`**（routing.go:2056-2175）：先为每个 candidate 读 allowance（账号 `Allowances(agent)`、key `keyAllowance`），填 `left{used,renews,fullest,soon,lapse,pace,due,amount,of,unit,credits,first,bal,balIn}`；不足两个候选直接返回。然后按策略标志位补 Tokens（`routed.used[restKey]`，1h 半衰）、Free、Speed（`speedOf`，按 restID）、Price（`PriceAt`，含 `priceClock` 与 peak），构造 `routes.Seat` 调 `routes.Apply`；非 Exact 且纯 key 池再做一次 `keyFit` 稳定排序。
- **`routingOf`**（routing.go:2040）：路由名未被任何策略服务（rotation 插件关了）→ 视为 `order`；`""`/`manual`/已注册保持原名。
- **`goesRound`**（routing.go:2050）：rotate/weight 为 true——trace 首位标 `Turn`，且 sink 不适用。

---

## 6. 亲和（Affinity）

- **存储**：`affinity.json`，与 `providers.json` 同目录（affinity.go:283 `sticksPath`）；内存 `sticks.m` + 落盘 `savedStick{Rest,Who,Model,Effort,Turn,At,CacheRead,CacheWrite,CacheHour,CacheHourAt,StoodIn}`；最多 `sticksKept`=512 条会话、每条保鲜 `stickKeep`=24h（affinity.go:79-82）；写入经 `noteSticks`/`saveSticks` 延迟合并（生产 2s，`sticksFlushAfter`）。
- **会话键**：`stickKey`（affinity.go:524-530）= `scope + "|" + conversationID(in, body)`；`scope` 为 `providerID/model` 或 `group/<id>`（组）；Claude Code 子代理追加 `|agent:<x-claude-code-agent-id>`，使子代理与主会话分开记。`conversationID`（gateway.go:8542-8585）：先查 `sessionHeaders`（`x-opencode-session`、`x-session-affinity`、`x-session-id`、`session_id`、`session-id`、`x-claude-code-session-id`、`x-conversation-id`、`x-deepseek-harness-session-id`，gateway.go:8464-8468），否则取首个 user 消息的 sha256 前 12 字节（`magpie-<hex>`）。
- **模式**（`provider.Affinities`，group.go:54-65）：`""`(auto)、`session`、`turn`、`off`、`strict`。Provider 和 Group 各有 `Affinity` 字段。
- **`affine`**（affinity.go:539-702）：三级匹配（who+model+effort → who+model → sameVendor，即 remote-magpie 下同 key 同上游 provider），然后按优先级判定 Why：
  - off / first（无记录）/ gone（候选中找不到）/ strict / reserve（MaxAccounts 后备不粘性）/ resting / credits（creditsLast 排后）/ busy（插件忙）/ main（fallback→主力就绪）/ order（order 组前成员就绪）/ back（非 order 组顶替者归位）/ spent（≥SpentShareOf 且非 CreditsFirst）/ session / turn（within-turn）/ new-turn（turn 模式或 rotate）/ no-cache（cacheRead+cacheWrite < `cacheWorth`=1024）/ cold（超过 `coldAfter` 且无 hourCache）/ cache。
  - Why ∈ {session, turn, cache} → 把候选提到第一（`weightedKept` 同步加权轮）。
  - strict：`strictOnly`（affinity.go:489）只留该 who 的候选（同 provider 的其它 model 保留给规则）；失败照样 rest（`strictAlone`）。
  - new-turn + rotate → `after`；first + rotate → `leastHeld(false)`；first + spread（`spreadsSessions`：session/strict 模式 × smart/usage/pace 路由）→ `leastHeld(true)` 或子代理 `withLead`。
- **auto 模式的缓存判据**：`cacheWorth`=1024 tokens；`coldAfter`：默认 `cacheCold`=5min，ChatGPT 账号 `chatGPTCold`=1h，DeepSeek 官方 API `deepseekCold`=1h（affinity.go:47-95）；Anthropic 1 小时 cache 写入（`CacheWrite1h`）经 `hourCache` 续命至 `cacheHour`=1h。OpenAI/Gemini 不报 cache 时以 Input 充当 write（affinity.go:967-971）。
- **解除**：应答在 200 后中断（`unanswered`，非 strict 且非规则 compaction）→ 删除 stick；`answered`/`answeredStanding` 在成功时写入。
- **Codex credits 系列**：`creditsLast`（restLast 排最后，Why="credits"）、`CreditsFirst`（`Seat.First`，tier 恒 0，不被 spent 挪后）、`creditsAgain`（onCredits 的 Codex 账号被 429 quota 时同请求内立即重问一次）。

---

## 7. 失败转移与健康（rest/重试/重排）

### 7.1 每轮一次、holdWriter 与转移条件

- 候选循环（gateway.go:2976-…）：每个 candidate 每轮只试一次（去重在计划阶段完成）。
- `holdWriter`（fallback.go:2189+）：在确定回复“通过”前扣住字节；失败且未发送任何字节 → 可转移到下一候选；已开始发送则只能以流内错误结束（`streamError`/`failTo`）。
- `passing(status, header, body, again)`（fallback.go:1792-1812）：同一 candidate 的“将过性”失败（408/500/502/503/504/529，非 didNotStream；429 且 failRate 且非 credit/walled）允许原体重试：`lastRetries`=2 次（普通）/`rateRetries`=3 次（限流），等待 `retryPause`（1s）翻倍，封顶 `longestPause`=8s（取 Retry-After 更小者；超过则交给 patience/replan 或报错）。
- **Patience / replan**（patience.go）：所有成员都因 busy（408/500/502/503/504/529 且 failOther 非 protection/didNotStream，或 429 failRate 非 credit/walled）失败且未发字节时，按 `replan` 重排：等待 `retryPause<<(round+1)` 封顶 `replanLongest`=30s，且不小于最近 Retry-After；轮数上限 `replanRounds`=12；每成员预算 `waitsOf(c.patience, fallback)`（`PatienceDefault`=60s、组 `Waits()`、provider 默认 `singlePatience`=60s；`PatienceOff` 跳过重排）；内层组设置优先（`innerPatience`）。
- **lanes/RPM**：`lanes.take`（concurrency.go:147-…）按 `who()` 限并发（`LaneLimit`：MaxConcurrency 或 AccountConcurrency 覆盖），队列超 `QueueLimit`/`QueueWait` → 429 不 rest；`laneMate`/`waitMate` 把满载者的请求让给同 provider/组内其它有空位且非 resting 的候选（`errMoved` 重排）；reserve 不接盘（`dropReserves`）。RPM 由 `rpms.wait` 在发送前按 `MaxRPM` 等待。

### 7.2 失败分类（`failure` / `failureOf`，routing.go:484-607）

状态码+正则（`creditWords`、`usedUpWords`、`rateWords`、`plannedWords`、`minuteWords`、`longWords`、`quotaCodes`、`resetsWhen`、`resetsInWords`、`quotaResetAfter`、`zhipuResets` 等）把失败分为：

`failCredit`（402、含余额/欠费/insufficient_quota；ChatGPT 账号特殊处理）、`failQuota`（429 计划/配额词、5xx+quotaCodes、resetsWhen 带时间的 429；402 只提用量不提钱也算 quota——Factory 的池级配额经 `pooled` 只 rest 模型）、`failRate`（429 且 minuteLimit/backSoon/rateWords 非 plannedWords；DashScope 的“配额”实为 TPM/TPS 限速）、`failOther`、`failAuth`（Claude sign-in 被拒/订阅被吊销）、`failVerify`（Google 验证）、`failRestricted`（WorkBuddy 11140/ZCode 3012）、`failSlow`（组 FirstToken 超时）、`failRefused`（安全过滤）、`failShape`（请求形状不被接受）、`failPrompt`（系统提示被拒，如 WorkBuddy unapproved channel）、`failEffort`（套餐不支持该推理档）、`failProxy`、`failOffline`、`failOverflow`、`failRoom`、`failLoop`（loopGuard 检测到循环）、`failCut`（插件主动截断）、`failForeign`（外域密封推理被拒）、`failFloor`、`failSight`、`failUpdate`、`failCanceled`。

### 7.3 rest 时长（`restAfter`/`restAfterMarked`，routing.go:1193-1586）

| 类别 | 规则 |
|---|---|
| failCredit | 普通 key：`creditLadder` = 30min → 2h → 8h → 24h → 3d → 7d（routing.go:230），按连续失败次数爬梯（`creditBackoff`，routing.go:247）；命名了 reset/Retry-After/`creditSoon` 的只 rest `creditRest`=30min 不爬梯；ChatGPT 账号：`chatgptCreditRest`=5h 或拒绝中指明的 reset/window。 |
| failQuota | 拒绝中指明时间（resets/resetsNoted/clineBackIn）→ 到该时刻；账号窗口已知满 → 到窗口重置（"window"）；否则 `quotaRest`=15min 或 `Provider.QuotaRest`/`QuotaResetAt`；月度配额（monthlyWords）上限 `longestMonthly`=35d，其余上限 `longestQuota`=8d。 |
| failRate | `rateRest`（routing.go:1709）：默认 1min，同 rest 结束后 `rateForget`=30min 内再次限流则翻倍，封顶 `RestCapOr(longestRateRest=30min)`；vendor 给了更长的 Retry-After/resets 则用之（上限 `longestWait`=1h）。同时按 Sink 规则 `sink(restKey/restID)` 沉底。 |
| failOther（普通失败） | `fallbackCooldown`=1min 起，连续失败翻倍，封顶 `RestCapOr(longestRetry=10min)`（routing.go:1457）；模型级计数（`routed.failures[restID]`）可能更高（modelBackoff）；账号窗口已知满 → "window" 至窗口重置（上限 longestQuota）。 |
| vendorOverloaded（529/5xx overloaded） | `overloadRest`=30s，给了 Retry-After 封顶 `overloadRestMost`=1min；rest 记到 restID（模型级）。 |
| failRestricted | `restrictedRest`=2h。 |
| failVerify | `verifyRest`=30min，`verifyHold`=1min 内直接回答不重试。 |
| failSlow | 连续 `slowRests`=2 次慢起后 rest 模型（restID），1min 起翻倍封顶 RestCap；`slowForget`=30min 清零。 |
| failRefused/failShape/failPrompt/failCut/failProxy/failOffline/failEffort/failLoop/政策拒绝/ZDR/OpenCode bare 400 | 不 rest（`By:"off"`）。 |
| `LimitRestOff`（provider 设置） | quota/credit/rate/429 不 rest。 |
| `NoFailRest()`（RestCapOff=-1） | failOther 不 rest。 |

- **rest 键**：默认 `restKey()`；下列情况降级为 `restID()`（模型级）：pooled 配额、unserved（modelRefused/codeAssistModelMissing/zcodeTrialBlocked）、modelBackoff、overloaded、resetsWhen+otherModels、clineModelLimited、zcodeTrialRefused、modelCooledDown、PaidBesideFree、remoteModelRest（remote-magpie 或带 `X-Magpie-Answered-There` 的应答；此时 `Rest.there`=true）。
- **rests.json**：只有 creditLadder 上的 key rest 持久化（`ladderRest`，rest_store.go:34），`saveRests`/`loadRests`，`restsKeep`=35d。
- **更长者胜**：同 key 已有更长 rest 时不被新的短 rest 覆盖；vendor 后来命名的准确时间可覆盖 magpie 猜测（routing.go:1562-1567）。
- **renewed/restarted**：`provider.OnRenewed`/`OnStarted` 回调——allowance 读数发现窗口不再满时，lift 该账号 quota/window 类 rest（`liftRests`）；`renewed` 只 lift 无 vendor 时间的，`restarted`（窗口真重置或花了 reset）连 "resets"/"retry-after" 也 lift。
- **quotaHeld**（fallback.go:1237）：所有剩余候选都是“因 quota/credit 被 rest 的纯 key（非账号、非 remote）、拒绝有原文、且猜测时长的 rest 已被拒过两次以上（`first`/`askAt`）”时，不再实际询问，直接回放第一次的拒绝并带 Retry-After（#1772）。
- **probeOne**（fallback.go:1173）：多个 resting 候选只派“最早回来”的一个试探。
- **rest_probe**（rest_probe.go）：纯 key 的 backoff rest（5xx/408/无连接，无 Retry-After，provider `ListAsks`）后台按 1/2/5/10min 节奏请求模型清单，成功即提前 lift（`probeable`/`probed`）；30min 内再失败则标记 `probeMissed` 不再探测。

### 7.4 Sink（sink.go）

`Provider.Sink`/`Group.Sink`：被 429 但窗口未满的 key/账号沉到队尾（`sink(restKey)` 或模型级 `sink(restID)`），直到手动 `Unrest`/`UnrestAll` 或重启；嵌套组 `sinkOwn` 先组内沉。rotate/manual 不适用（`sinks`）。

### 7.5 连接层健康

- 每个 vendor host 共享一条 HTTP/2 连接；20s 无帧发 ping，10s 无应答断开（`netproxy.HTTP2Health`，upstream_transport.go:66-77）；大请求体（≥64KB 或长度未知，uploadBytes）在丢 ping 后换无 ping 的 `uploadTransport` 重发一次，之后 30min（`slowFor`）大请求都走该通道。
- `replayable`：标记 `X-Idempotency-Key` 使 Go 在 reused 连接 EOF/broken pipe 时立即换连接重发；新连接写入即被关（`closedBeforeSent`）再重试最多 2 次（`unsentAgain` 250ms/1s），仍失败记 `errClosedUnsent` → failProxy 不 rest。

---

## 8. 中间件策略（P3 router strategy）

- **注册链**：`plugins/middleware` 的 `Apply` 注册 `gwmw.Use(middleware.Chain{})` 与 `usage.Observe(middleware.Points())`（plugins/middleware/middleware.go:28-32）。`Points()`（internal/middleware/points.go:361）在 `Serve()` 期间每秒检查已加载模块，把每个 `strategies[name]` 通过 `routes.Register(<plugin>/<name>, strategy)` 挂上（points.go:483-498 swap）。
- **路由名**：`<shortName>/<name>`——包取 `plugin.ShortName`（去 scope、去 `agent-`/`tools-`/`page-` 前缀），文件取 basename 去 `.js/.mjs/.cjs/.middleware/.gateway` 后缀，非法字符转 `-`（points.go:182-196）。
- **JS 侧契约**：`strategies[name]` 为 `(pool, seats, options) => order` 或 `{order: fn}`；`seats` 是 camelCase JSON（`key/account/known/learns/used/soon`(unix ms)`/pace/weight/resting/tokens`，points.go:201-212）；返回 seats 下标排列。
- **限制**：`strategyLimit`=20ms（含等运行时）；超时/异常/返回非排列 → fail-open 保持列出顺序并计失败。每模块一个 runtime，策略与 `onUsage` 共享（一次一个调用）。
- **持久化**：`provider.PluginRouting`（routing.go:56-62 正则）允许 providers.json 保存 `<plugin>/<name>`；插件缺席时该池按列出顺序尝试（`routingOf`→order）。
- **选择器**：GUI 的 routing picker 从 `jsRoutings`（settings state）列出当前服务的 JS 策略；CLI `magpie group set <id> routing=<plugin>/<name>` 同样接受。

---

## 9. remote-magpie（多 magpie）

- **识别**：`Provider.Preset == "remote-magpie"`（provider/remote_magpie.go:69-72）。`remoteMagpieEndpoints`（provider/remote_magpie.go:77-109）把用户给的任意形式地址归一为 Chat/Responses=`<root>/v1`、Anthropic=`<root>`、Decide=Chat。模型清单请求带 `X-Magpie-Drawers`/`X-Magpie-Videomakers`/`X-Magpie-Prices`/`X-Magpie-Deciders`（拿到图像/视频/价格/决策模型及远端计价）。
- **作为候选**：远端 magpie 在本地就是一个 provider（可有多把 gateway key 作为 keys）。它的模型 id 形如 `<远端provider>/<model>` 或 `group/<id>`（远端的组），本地按普通 provider 计划、并发、限流（lanes/RPM 按本地 key）。
- **vendor 概念**：`remoteUpstream`（fallback.go:181-193）取模型 id 第一段（`anthropic/claude-…` → `anthropic`）或整个 `group/<id>`；`candidate.vendor() = who() + ">" + upstream`。亲和（stick）、严格模式、外域密封记忆都按 vendor 记——远端每个 provider 有独立的 cache 和密封。
- **rest 降级**：`remoteModelRest`（routing.go:1624-1629）——远端 quota/credit/rate/restricted/verify 只 rest 那一个模型（restID），因为远端自己对各模型独立 rest；但本地 key 被拒（`remoteKeyLimitHeader`/`keyLimitThereHeader`）仍整 key rest。手工添加的 OpenAI 兼容 magpie 也算（凭 `X-Magpie-Answered-There`/`providerHeader` 识别）。
- **组保持**：请求带着 `X-Magpie-Response-Model: vendor`、`X-Magpie-Agent`/`X-Magpie-Via`、`X-Magpie-Session` 与原生 session 头（`passOnCaller`，remote_magpie.go:132-157）转发；远端按 `callerFrom`/`callerOf` 记录来源 agent。`Try.Routed` 标记“Model 是远端组、Served 是其成员”（`usage.GroupRouted`）。
- **决策/检索/媒体**：`serveRetrieval`、`transcribe`、`speech`、`draw`、`video` 都支持远端转发（remote_magpie.go 头部注释与 gateway-routing.md 的 Remote magpie APIs 一节）。

---

## 10. 可观测性

- **回复头**：每次成功/失败的尝试由 `noteMember`（member.go:57-71）在响应上写 `X-Magpie-Provider: <provider id>` 与 `X-Magpie-Model: <provider>/<model>`；`X-Magpie-Response-Model`（asked/member/vendor）控制回复体里 model 字段写法（`memberWriter`，settings.ReplyModel 全局默认）。
- **路由轨迹**：`trace.beginFor` 在 serve 中建立 `Route`（含 Order/Left/Rule/Affinity/…），`trace.update` 在每次尝试与结束时补 `Try`。`Route.Folder` 只保留工作目录最后两级（`shortFolder`）。
- **历史**：routing history 落盘保留 30 天/200MB（history.go）。
- **HTTP API**：
  - `GET /v1/magpie/route?session=<id>[&after&wait]`（session_route.go）：某会话最新路由（长轮询最多 60s）。
  - `GET /v1/magpie/trace`（仅 loopback）：完整轨迹；`GET /api/gateway/trace`（GUI）。
  - `GET /v1/magpie/route-events/snapshot` 与 `GET /v1/magpie/route-events?after=<cursor>`（route_events.go，SSE）：面向展示设备的脱敏事件流（无 session/账号/folder/prompt），ring 512 条，含 attempt/TTFT。
  - `GET /v1/magpie/concurrency`、`/v1/magpie/quotas`、`/v1/magpie/limit`、`/v1/magpie/usage*`。
- **usage 记录**：一次请求一条最终记录（`usage.Record`），`Tries>1` 标记；中间失败但已计费的尝试另写 `HandedOn` 记录（gateway.go:3610-3633 的 `failedRow`）；`Totals.count` 跳过无计费失败行。
- **Prompt 分解**：`inspectPrompt` 记录 prompt 构成（名称与大小，不含正文；正文仅内存留最近 `promptBodies`=10 条、每条 ≤32MiB、共 ≤64MiB，脱敏后供 `/api/context/text`）。

---

## 11. 选路全流程（POST /v1/chat/completions → upstream）

1. `Handler` 匹配路由 → `handle(provider.Chat)`（gateway.go:1790）：`requestBody` 读体（`readBoundedRequestBody`，gzip/zstd 解码、512MB 上限）、`requestModel` 取模型名 → `serveAgent`（middleware.go:23）。
2. `serveAgent` 建立会话身份，跑 Bun 异步 hooks，然后对注册的 `gwmw.Agent`（JS 中间件链）`Begin`→`Request`（可改体/拒绝）；无中间件直接 `serve`。
3. `serve`（gateway.go:1980）：
   a. `offAsNone`/`heicAsJPEG`/`withWires`；`askedAt` 拆 `:effort`。
   b. 替身链：classifierStandIn → claudeTierStandIn → grouped（keyGroupFor/AutoStandIn/GroupFor）→ standIn（agent 配置）→ codexMemoryStandIn；Desktop/Science/subagent 改写；记 `Route.Moved`。
   c. `provider.Resolve` → (p, model)；禁用/关闭/未知 → 403/404；决策模型 → 400。
   d. 组：`provider.FindGroup` + `Live()`；脱敏（fails closed；maskRouteFor 改投）；`heldByGroup`；gateway key 模型/账号限制（403）。
   e. `PausedOut`（pause 规则）；sealed task 闸门（leadProvider/leadFirst）；组规则 `ruleFor`（分类器，共享一次 `askWaited` 超时）。
   f. 图像能力过滤（text-only 摘除历史图或图片描述）。
   g. careful provider 用量重读（`readCareful`）。
4. 计划：
   - 组：`planGroup`（planLevel 递归嵌套、ownRouting、加权轮、sinkOwn）；
   - 单 provider：`planPaused`（perKeyBarred→weigh→fallback 链（含 fallback 组 planFallbackGroup），seen 去重）；
   - `restLast`（busy/creditsLast/resting 分档）。
5. 过滤与调整：`sealedReaders`/`checkpointReaders`/`allowedCandidates`/`strictKeep`/`autoReset`（全扣时自动花 reset）；全被 cap 扣 → 429 `cappedError`；`affine`（亲和/leastHeld/withLead/after）；`ruleOver`/`nestedRules`；图像成员过滤；`pinTo`；`roomy`（窗口）；`behindOverflow`；`probeOne`（resting 只留最早回来的一个）。
6. `trace.beginFor` 落 Route；`noteMaskedUntil`；`inspectPrompt`。
7. 候选循环（gateway.go:2976 起）对每个 candidate：
   a. `laneMate`（满载让位）/`quotaHeld`（全 key 配额尽直接回放）/`accountSkipped`（JS onAccount skip）/`resetBeforeCredits`；
   b. 组装 attemptBody：`withoutRefused`（外域密封）、图像处理、`triedOn`（JS onProvider）、webp 转换、placeholder 注记、max output 收口、effort 系列（fixed/ruled/agentPick/组 Levels 边界/quiet/reasonless）、fast/ultra tier、effort update（保缓存）；
   c. `lanes.take`（并发槽；等位时可 `waitMate` 易主 errMoved）→ `rpms.wait`（RPM）→ `attempt`（gateway.go:4688：协议修整 joinToolRuns/chatCallIDs、`p.Via` 代理、Claude 订阅桥、Copilot Auto、relay vs translate 判定（`usable`/`fits` 记忆 unfit）、`passthrough` 或 `translate` → `forwardOnce`/`forwardTranslated` 发上游）。
   d. `holdWriter` 判定：通过（passing）/失败（未发字节）/已开始发送。
   e. 失败处理链（按序）：slow（FirstToken，restSlow）→ reloop（推理循环原地重问一次）→ Copilot repick → foreign reasoning（去密封重问，记 refusedSeals/refuseCarried）→ effort update 被拒（回退并记忆）→ image refused（描述后重问）→ token floor → reply room（窗口剩余重问）→ refusal（安全过滤，换候选，matesFirst，记 RefusalRetries）→ turnedAway（Antigravity 系统提示，matesLast）→ promptRefused（WorkBuddy，matesLast）→ effortRefused（matesFirst）→ overflow（withRoom 换有窗口的；noteOverflow）→ webp/GLM（转 JPEG 重问）→ shapeRefused → failProxy/offline/failAuth（不 rest）→ protection/zdr/noRoom（elsewhere/notMates）→ spentAfter（其余全配额尽时原体重试）→ codexReset → 摘要截断/空回复（不 rest）→ plainForbidden 协同（forbiddenAlike 撤销同伴 rest）→ creditsAgain → unlisted（模型被拒后问清单外账号）→ failCut（原地重问）→ 普通失败 `restAfterMarked` 记 rest → Patience replan（busyNow/replan/pauseAlive）→ passing 原体重试（最后候选）→ autoReset（最后且 quota）→ small（窗口不足者最后手段）。
   f. 成功：`servedCandidate`（清 rest、记 tokenUse、noteSpeed）；亲和 `answered`/`answeredStanding`（记 stick/stoodIn）；`MoveRound`（亲和留住时修正加权轮）。
8. 结束：写最终 usage 记录（含 tries）、`trace.update`（Done/Status/Millis/Tokens/TTFT/Flow）、`FlushSticks` 延迟落盘 affinity.json。

---

## 12. 关键参数与默认值

| 参数 | 值 | 位置 |
|---|---|---|
| 默认地址 | `127.0.0.1:3425` | gateway（DefaultAddr） |
| affinity 默认模式 | `""`（auto：within-turn 恒保持；跨 turn 看缓存） | provider/group.go:54 |
| cacheWorth | 1024 tokens | affinity.go:51 |
| cacheCold / chatGPTCold / deepseekCold / cacheHour | 5min / 1h / 1h / 1h | affinity.go:52-77 |
| stickKeep / sticksKept | 24h / 512 会话 | affinity.go:79-82 |
| heldFor（leastHeld 会话持有窗口） | 30min | affinity.go:777 |
| usageHalfLife（LeastUsed 的 Tokens 半衰） | 1h | routing.go:37 |
| speedHalfLife / speedStale | 20min / 1h | speed.go:24-25 |
| Fastest：RefTokens / ProbeEvery / alike | 200 / 5 / 1.1 | fastest.go:39-47 |
| LowShare / SpentShare(=UsedShare) | 90 / 98（order、drain 为 100） | strategies.go:16-19、codex_switch.go:116-127 |
| FreshPace（未知账号 pace） | 100/(7*24) | provider/routing.go:550 |
| fallbackCooldown（普通失败/限流起步） | 1min | fallback.go:40 |
| creditRest / quotaRest / chatgptCreditRest | 30min / 15min / 5h | routing.go:171-179 |
| creditLadder | 30m,2h,8h,24h,3d,7d | routing.go:230 |
| longestRetry（失败 backoff 封顶） / longestRateRest | 10min / 30min（可被 Provider.RestCap 覆盖） | routing.go:184-194 |
| longestWait（信任 vendor Retry-After 上限） / longestQuota / longestMonthly | 1h / 8d / 35d | routing.go:173、183、1133 |
| overloadRest / overloadRestMost | 30s / 1min | routing.go:189-190 |
| verifyRest / verifyHold / restrictedRest | 30min / 1min / 2h | routing.go:200-213 |
| slowRests / slowForget | 2 次 / 30min | routing.go:217-218 |
| lastRetries / rateRetries / retryPause / longestPause | 2 / 3 / 1s / 8s | fallback.go:1769-1786 |
| PatienceDefault / PatienceLongest / replanRounds / replanLongest | 60s / 240s / 12 / 30s | provider/group.go:146-148、patience.go:42-52 |
| FirstToken（组首内容超时） | Group.FirstToken，默认 0=无限 | group.go:200、gateway.go:3091-3095 |
| classifyTimeout（分类器默认） / classifyRest / classifyKeep | 8s（可设 1-60s） / 30s / 10min | rules/classify（settings.ClassifyTimeout） |
| body 上限 defaultBodyLimit | 512MB | request_bounds.go |
| upstreamHeaderWait / HTTP2 ping / uploadStall / uploadBytes / slowFor | 10min / 20s+10s / 1min / 64KB / 30min | upstream_transport.go |
| lanes：QueueLimit/QueueWait/MaxRPM 默认 | 0=不限 | provider.go |
| restProbeSteps / restProbeWait / restProbeMiss | 1,2,5,10min / 20s / 30min | rest_probe.go:32-39 |
| JS 策略限时 strategyLimit / onUsage 限时 / 队列 | 20ms / 250ms / 1024 | middleware/points.go:46-53 |
| turns（rotate 计数）/ drains / probes / rounds 状态 | 内存 per-pool | routes（Forget 清零） |

---

## 13. 证据索引（文件:行号）

- 策略库：`internal/routes/routes.go:22-187`（Seat/Strategy/Register/Find/Apply）、`strategies.go:130-518`（Smart/Ordered/Drain/Rotate/LeastUsed/Pace/Weighted/FreeFirst/Cheapest/Builtin）、`fastest.go:39-148`、`round.go:27-102`（Round/MoveRound/Forget）。
- 排序与 rest：`internal/gateway/routing.go:37`（usageHalfLife）、`:170-230`（rest 时长常量与 creditLadder）、`:373-444`（fail* 分类）、`:484-607`（failure/failureOf）、`:658-727`（Rest/asksAt）、`:1193-1586`（restAfterMarked）、`:1645-1689`（restSlow）、`:1709-1746`（rateRest）、`:1939-2175`（route/weigh/weighRouted/routingOf/goesRound）。
- 候选与计划：`internal/gateway/fallback.go:42-104`（candidate/capHold）、`:165-213`（who/restKey/restID/vendor/sameVendor/remoteUpstream）、`:240-438`（perKeyBarred/keepAtOnce/perKeyEvery/keyFit/modelFamily）、`:483-615`（candidates/plan/planPaused/planFallbackGroup/planGroup/placed）、`:657-846`（planLevel/sinkOwn/firstAwake/ownRouting）、`:889-1103`（capHeld/cappedError/creditsHeld/barredOf/asideOf）、`:1111-1204`（restLast/busyOf/probeOne）、`:1211-1331`（restingOf/quotaHeld/soonestBack/spentAfter/spentModel）、`:1653-1812`（retryable/tooLongUnsaid/passing）、`:2189+`（holdWriter）。
- 亲和：`internal/gateway/affinity.go:47-95`（常量/coldAfter）、`:99-149`（Affinity/stick）、`:262-375`（savedStick/stickOf/saveSticks）、`:425-530`（strictKeep/strictOnly/stickKey）、`:539-702`（affine）、`:709-772`（backToMain/backToFirst/backFromStandIn/standsInFor）、`:796-951`（leastHeld/readyForNew/standsIn/withLead/spreadsSessions/after）、`:961-1021`（answered/unanswered）。
- 主流程：`internal/gateway/gateway.go:1790`（handle）、`:1980-2539`（serve 前段：替身/Resolve/脱敏/key 限制/规则）、`:2540-3099`（serve 中段：计划/过滤/亲和/trace/候选循环开始）、`:3100-3659`（尝试与 attemptBody/泳道/attempt 调用）、`:3660-4539`（失败处理链与结尾）、`:4688-4885`（attempt：relay/translate 判定）、`:6635-6669`（fits/markUnfit/usable）、`:8464-8585`（sessionHeaders/conversationID）。
- 组与解析：`internal/provider/group.go:38-65`（GroupPrefix/Affinities）、`:98-134`（routes/Live/Within/pools）、`:142-161`（Patience 常量/Waits）、`:168-323`（Group/Member）、`:825-1081`（FindGroup/FallbackGroup/GroupFor/groupNamed/GroupFinder/membersFound/unpicked/ResolveRoute）、`models.go:3145-3199`（Resolve/resolveIn）、`autoref.go:34-233`（AutoPrefix/CanonicalRef/PublicRef/spelledIn）、`flat.go:15-68`。
- 远程：`internal/provider/remote_magpie.go:53-185`、`internal/gateway/remote_magpie.go:27-157`、`internal/gateway/routing.go:1606-1629`（remoteModelRest）。
- 中间件策略：`internal/middleware/points.go:45-282`（限制/seatJSON/strategy/order）、`:356-522`（Points/Serve/swap/Routings）、`plugins/middleware/middleware.go:22-33`、`plugins/rotation/rotation.go:19-49`、`plugins/groups/groups.go:17-29`、`internal/provider/group_serve.go`（GateGroups/ServeGroups，见 plugin-runtime.md:216）。
- 并发/探测/耐心：`internal/gateway/concurrency.go:29-180`、`rest_probe.go:29-263`、`patience.go:13-202`。
- 观测：`internal/gateway/member.go:46-118`、`trace.go:43-505`、`session_route.go:20-175`、`route_events.go`（见 gateway-routing.md 的 Route events 一节）、`speed.go:19-116`。
- GUI：`internal/gui/groups.go:469-559`（GET/POST /api/groups）。
- 文档：`docs/subsystems/gateway-routing.md`（权威总表，含 Model resolution/Candidates/Order of keys/Affinity/One try/Trace 等条目）、`gateway-middleware.md`（JS 策略注册与限制）、`plugin-runtime.md:189-217`（P3 router strategy 与 groups）、`provider-plugins.md`。

---

## 14. 未确认项

1. **`routes_test.go`/`drain_test.go`/`freefirst_test.go`/`wallet_test.go` 中的边界语义**（如 wallet 在 Drain/Fastest 下的表现）只读了实现与 cheapest_test，未逐条核对全部测试断言；如与实现注释冲突，以测试为准。
2. **`decide.go` 中 RouteDecider/postDecide 的“逐跳剥 provider 前缀”**：gateway-routing.md 提到 System One 用 `RouteDecider`/`postDecide` 每一跳剥一个 provider 前缀；本次只确认了 `provider.RouteDecider` 调用点（decide.go:564）与 decision group 的失败转移（decide_group.go），未细读多跳转发实现。
3. **`internal/gateway/routing.go` 中 `sinks`/`sinkLast`/`sinkPlanned` 的具体沉底算法**（sink.go，4.4KB）未逐行读；语义以 gateway-routing.md 的 Sink 条目与调用点为准。
4. **`classify.go` 的意图分类器提示词与 `ruleFor` 的完整判定顺序**（rules.go 只读了头部 RuleHit/Classified 结构）；规则匹配细节（intent/tokens/images/effort/folder/agent/hours/days/pause）未全部展开。
5. **`provider.Allowance` 的 `For/Pace/Renewal/Lapse/Left/Full/CapHeld/Restarts` 各方法的精确取数逻辑**（account_caps.go、ratewindow.go 等）未逐行核实；Seat 字段的来源以上述调用点为准。
6. **`plugins/routing`（Routing 页面 client.js）如何展示策略**未读；仅确认其 Manifest 与 `InUse`（plugins/routing/routing.go）。
7. **`MaxAccounts` 插件声明值 `atOnceOf`**（provider.AtOnce 读取插件 maxAccounts）的实现位置在 plugin host（host.js），Go 侧只确认调用点。
8. 报告基于静态阅读，未运行 magpie 做行为验证。
