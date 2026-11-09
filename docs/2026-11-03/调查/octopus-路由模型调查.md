# Octopus 路由模型与请求路由机制调查

- 调查对象：`D:\CodeWorkspace\octopus`
- 调查方式：只读 Go 源码；未修改被调查仓库，未运行会改变其状态的命令。
- 调查日期：2026-11-03
- 说明：下文路径均相对于 `D:\CodeWorkspace\octopus`。行号用于定位本次工作区源码，后续代码变化可能使其漂移。

## 1. 概览

Octopus 的推理请求选路不是单一“负载均衡函数”，而是以下连续层级：

1. **客户端模型名 → Group**：先应用全局 `ModelMapping`，再按 `EndpointType + Group.Name` 精确查找，未命中再按 `Group.MatchRegex` 查找；必要时回退到原模型名所在分组，最后可应用分组作用域映射。见 `internal/relay/model_identity.go:28-66`、`internal/op/group/group.go:657-677`。
2. **Group → 有序 GroupItem 候选**：`GroupMode` 决定 `GroupItem{ChannelID, ModelName, Priority, Weight}` 的顺序；API Key 黑名单、一次性渠道优先和粘性渠道提前在 `Iterator` 中统一处理。见 `internal/relay/balancer/iterator.go:48-107`。
3. **GroupItem → Channel + 实际上游 model**：逐项检查渠道存在/启用、模型非空、渠道类型与请求能力兼容；`identity.candidate(item)` 决定最终上游模型名。见 `internal/relay/relay.go:1693-1729`、`internal/relay/model_identity.go:57-67`。
4. **Channel → ChannelKey 或 PoolAccount**：`PoolID==0` 时使用 inline `ChannelKey`，按模型支持、排除 ID、按模型冷却过滤后，以 cost/availability/speed/priority 策略选 Key；`PoolID>0` 时改用 `poolscheduler.SelectAccountLease`，账号 ID 被复用为尝试上下文里的 `usedKey.ID`。见 `internal/model/channel.go:491-544`、`internal/relay/relay.go:1748-1802`。
5. **出站协议适配器链**：`Group.OutboundFormat + Channel.Type + 入站格式` 决定 Chat/Responses/Messages 等 adapter 顺序；只有可换候选且尚未写响应的失败才尝试下一种 adapter。见 `internal/transformer/outbound/register.go:126-175`、`internal/relay/relay.go:94-115`。
6. **失败反馈与重试**：错误被分为不换路、同渠道换 Key/账号、换下一候选、全终止四种 scope；外层还有路由轮次、总决策上限、冷却、失败提示和熔断。见 `internal/relay/type.go:305-373`、`internal/relay/relay.go:1610-2035`。

需要特别区分：

- **GroupMode** 排序的是“渠道+模型”候选，不直接选 Key。
- **Channel.KeySelectionStrategy** 选择的是同一渠道内 inline Key。
- **AccountPool.Strategy** 选择的是账号池账号，和前两套策略相互独立。
- **PriceCategory/PriceSchedule** 主要用于计费；只有 Auto 策略开启 `auto_strategy_price_weight` 时，基础输入价才参与渠道评分。
- **model_normalize** 主要用于模型广场/名称归并，不是请求时 `ModelMapping` 的同义词。

## 2. 选路全流程：从 `/v1/chat/completions` 到 upstream

以下以普通对话请求为主；媒体端点差异见第 11 节。

1. **注册入口与认证**。`POST /v1/chat/completions` 经 `APIKeyAuth()` 后调用 `relay.Handler(EndpointTypeChat, InboundTypeOpenAIChat, c)`。中间件从 `Authorization: Bearer` 或 `x-api-key` 取 Key，校验前缀、启用、过期、费用/Token 上限和 IP，并写入 `api_key_id`、`supported_models`、`allowed_group_categories`、配额与 `excluded_channels`。证据：`internal/server/handlers/relay.go:14-48`、`internal/server/middleware/auth.go:57-127`。
2. **解析入站协议**。`parseRequest` 有界读取 body，使用 inbound adapter 转成 `InternalLLMRequest`，执行 `Validate`，保存 query/path，并由 `populateRelayRequestSessionFields` 保存原始 body、ConversationID 和重连序号。证据：`internal/relay/relay.go:600-630`、`internal/relay/request_session.go:12-49`。
3. **路由前插件与 API Key 模型权限**。`applyBeforeRoutePlugin` 可改请求；随后 `apiKeyAllowsModel` 对客户端原始模型名做精确逗号列表检查。证据：`internal/relay/relay.go:194-225`、`:165-175`。
4. **模型映射并命中 Group**。`resolveRelayModel` 处理 `zen/...` 特例和全局映射，调用 `GroupGetEnabledMapByEndpoint`；命中后校验 API Key 的 `AllowedGroupCategories`。证据：`internal/relay/model_identity.go:28-54`、`internal/relay/relay.go:227-247`。
5. **Group 查找顺序**。对对话端点按 `conversationEndpointLookupOrder` 尝试端点类型；每个类型先查精确名称，再按 Group ID 顺序查已编译正则，最后尝试 `EndpointTypeAll=="*"`。`finalizeMatchedGroup` 预先去掉不存在或禁用的渠道项。证据：`internal/op/group/group.go:289-344`、`:391-430`、`:657-677`。
6. **条件路由门禁**。对已经选中的 Group 执行 `condition.Evaluate`；JSON 无效或结果为 false 都返回 404。它是“选中分组后的过滤”，不是在多个分组之间按条件择优。证据：`internal/relay/relay.go:244-247`、`internal/relay/condition/evaluator.go:29-47`。
7. **配额与可恢复流会话**。预留 RPM/TPM 配额；如果流式请求带 ConversationID，则获取/恢复 `relayStreamSession`。已有会话直接重放/订阅，不再重新选路。证据：`internal/relay/relay.go:249-288`、`internal/relay/stream_session.go:173-227`、`:687-778`。
8. **候选空检查、Provider 兼容改写与 metrics**。`HasCandidates` 先考虑 API Key 渠道黑名单；`rewriteConversationRequestByProvider` 可按 openai/deepseek/mimo 删除不兼容 reasoning 字段；之后创建请求级 `relayRequest`。证据：`internal/relay/relay.go:290-320`、`:1544-1590`、`internal/relay/balancer/group_rotation.go:21-28`。
9. **缓存短路**。语义缓存命中可直接返回而不访问上游；非流式安全请求还可能经 singleflight 共享另一个在途请求结果。证据：`internal/relay/relay.go:322-362`、`internal/relay/retry_cache.go:333-365`。
10. **读取重试预算**。`relay_retry_count` 转为每候选 `N+1` 次 Key 尝试，默认 3→4 次；`relay_route_retries` 默认 2 轮；`ratelimit_cooldown` 默认 300 秒；`relay_max_total_attempts<=0` 时回退 64 次总决策上限。证据：`internal/relay/type.go:36-44`、`:57-95`。
11. **每轮重建候选 Iterator**。`executeRelay` 每个 route round 先运行路由过滤插件，再 `balancer.NewIterator`。Iterator 依 GroupMode 排序，剔除 API Key 黑名单，稳定前置 Disposable 渠道，最后把有效 sticky 渠道移到第一位。证据：`internal/relay/relay.go:1645-1676`、`internal/relay/balancer/iterator.go:51-107`。
12. **逐个候选检查**。加载 Channel，跳过不存在/禁用/上游模型为空/出站类型不支持/embedding 或 chat 能力不兼容/`zen` 偏好不符的项；随后将 `internalRequest.Model` 改为本候选的实际上游模型。证据：`internal/relay/relay.go:1678-1729`。
13. **选择 inline Key 或池账号**。inline 首次用 `GetChannelKeyWithCooldown`，重试用 `GetChannelKeyExcludingWithCooldown(failedKeyIDs,...)`；池模式用 `SelectAccountLease` 并解密账号凭据，账号的 BaseURL/Proxy/Platform/Type 写入尝试上下文。证据：`internal/relay/relay.go:1730-1802`、`internal/op/pool/credentials.go:31-49`。
14. **硬隔离检查**。inline Key 先查短期 failure hint，再查 `(channel,key,model)` 熔断；跳过不消耗 Key 轮次，并把 Key 加入排除集合，防止确定性选 Key 造成死循环。池账号不走这套渠道熔断，因为池调度器自行做健康反馈。证据：`internal/relay/relay.go:1792-1827`。
15. **执行 adapter 尝试**。创建 `relayAttempt`，带入首 Token 超时、整次尝试超时、渠道/Key/池信息，调用 `attempt→forward`。若前一 adapter 返回 `ScopeNextChannel` 且尚未写客户端，可继续下一 adapter；中间 adapter 失败暂缓健康记账。证据：`internal/relay/relay.go:1833-1872`、`:94-115`、`internal/relay/type.go:375-380`。
16. **构造并发送 upstream 请求**。`forward` 可设置 `AttemptTimeOut` context；除 passthrough/raw 外，执行渠道 rewrite、`param_override`、Group ThinkingMode 和端点元数据处理；Pool BaseURL 覆盖渠道 BaseURL，adapter 用最终 Key 构建请求，再调整池鉴权头、复制自定义头、运行 upstream 前插件。证据：`internal/relay/relay.go:633-717`、`internal/relay/rewrite.go:12-51`。
17. **实际网络/节点传输**。普通渠道先 SSRF 校验，再按“账号代理优先、渠道代理回退”选择 HTTP client；NodeID 渠道走 `nodetransport.Client` 且禁止池代理。证据：`internal/relay/relay.go:987-1034`、`internal/helper/channel.go:40-53`。
18. **响应与超时处理**。非 2xx 保存响应头和前 2KiB 错误体证据；流式响应启动 `FirstTokenTimeOut`，并依 `reasoning_buffer_strategy` 决定 reasoning-only chunk 是缓冲还是立即写客户端。证据：`internal/relay/relay.go:696-705`、`:828-853`、`:1036-1146`、`:1192-1204`、`:1261-1267`、`:1361-1388`。
19. **成功反馈**。成功后记录 Key 成本、渠道/模型统计、熔断成功、Auto 成功率/总时延/TTFT、Key 可用度和 TPS，并写入 sticky；池账号另 `ReportResult(success)` 且释放 lease。证据：`internal/relay/relay.go:502-545`、`:1873-1882`。
20. **失败分类与转移**。`ClassifyRelayError` 给出 scope；executeRelay 据此终止、同渠道重选 Key/账号或 `break keyAttempts` 换下一候选。全部候选后可进入下一 route round，但若上一轮没有任何真实转发则快速失败；最终返回 `all channels failed`。证据：`internal/relay/type.go:406-580`、`internal/relay/relay.go:1925-2035`。

## 3. 数据模型与关系

### 3.1 核心关系图

```text
APIKey ──支持模型/分类/渠道黑名单/配额──┐
                                      ▼
ModelMapping(可选) → Group(EndpointType, Name/MatchRegex, Mode, Condition)
                         │ 1:N GroupItem
                         ▼
               GroupItem(ChannelID, ModelName, Priority, Weight)
                         │ N:1
                         ▼
              Channel(Type, BaseUrls, PoolID, NodeID, ...)
                    ┌────┴───────────────┐
                    │PoolID=0            │PoolID>0
                    ▼                    ▼
             ChannelKey:N          AccountPool ──1:N── PoolAccount
                    │                    (Strategy)       │
                    └──── 最终上游 URL + model + 凭据 ────┘
```

`Channel.GroupID` 指向的是另一实体 `ChannelGroup`（渠道管理分组），不是推理路由的 `Group`；推理请求实际通过 `GroupItem.ChannelID` 找渠道。`ChannelGroup` 只有 ID/Name/IsDefault/时间字段，见 `internal/model/channel_group.go:1-11`。

### 3.2 Group / GroupItem

`Group` 关键字段（`internal/model/group.go:38-80`）：

| 字段 | 作用 |
|---|---|
| `ID`, `Name`, `Category` | 分组标识与 API Key 分类权限；`(EndpointType,Name)` 有联合唯一索引。 |
| `EndpointType`, `EndpointProvider` | 端点能力分类与提供方兼容策略。 |
| `OutboundFormat` | 出站 adapter 选择；支持 auto/chat/responses/messages、`*_only`、passthrough、raw。 |
| `Mode` | `GroupModeRoundRobin=1`、Random=2、Failover=3、Weighted=4、Auto=5。 |
| `MatchRegex` | 模型名到分组的正则匹配；不是 GroupItem 的条件字段。 |
| `Condition` | 选中分组后执行的条件路由 JSON。 |
| `FirstTokenTimeOut` | 流式首 Token 超时秒数，0 不启用。 |
| `AttemptTimeOut` | 单次完整转发尝试超时秒数，0 不启用；HTTP+响应读取均受其 context 约束。 |
| `SessionKeepTime` | inline/组级 sticky TTL 秒数，0 禁用。 |
| `ReasoningBufferStrategy` | 空值继承全局；`buffer`/`immediate`。 |
| `ThinkingMode` | auto/off/on；控制出站思考参数，不决定候选顺序。 |
| `LastTest*` | 探测展示状态，不参与运行时选路。 |

`GroupItem` 含 `GroupID`、`ChannelID`、`ModelName`、`Priority`、`Weight`，三者的组合有唯一索引。Priority 数字越小越前；Weight 在加权模式中越大占比越高。缓存归一化会把 Weight<=0 改为 1，并按 Priority/ChannelID/ModelName/ID 稳定排序，见 `internal/op/group/group.go:346-388`。

### 3.3 Channel / ChannelKey

`Channel` 核心字段见 `internal/model/channel.go:82-129`：

- 身份与协议：`ID`、`Name`、`Type(outbound.OutboundType)`、`Enabled`。
- 上游地址与模型：`BaseUrls[]{URL,Delay,SuffixMode}`、`Model`、`CustomModel`。`GetBaseUrl` 选 Delay 最小的非空 URL，而不是在多个 BaseUrl 间轮询，见 `internal/model/channel.go:321-342`。
- 凭据：`Keys []ChannelKey`；`PoolID==0` 用 inline keys，`PoolID>0` 用池账号；`NodeID` 选择节点隧道。
- Key 策略：`KeySelectionStrategy`，空值继承全局，最终默认 cost。
- 路由辅助：`Disposable`（候选中绝对优先）、`ExpireAt`、`AutoGroup`、`MatchRegex`（用于模型获取/自动分组辅助，不是 Group.MatchRegex 的运行时查找）。
- 请求整形：`CustomHeader`、`ParamOverride`、`RequestRewrite`、代理配置、限速同步与巡检字段。

`ChannelKey` 字段见 `internal/model/channel.go:142-160`：`Enabled`、`ChannelKey`、`StatusCode`、`LastUseTimeStamp`、`TotalCost`、`Priority`、`SupportedModels`、`Managed` 等。`SupportedModels` 非空时按 trim 后精确匹配目标模型；`Managed` 用于站点投影 diff，避免删除手工 Key。

inline Key 的硬过滤顺序是 Enabled → excludeKeyIDs → SupportedModels → `(channel,key,model)` 冷却；之后才按策略选最优，见 `internal/model/channel.go:491-544`：

- `cost`：`TotalCost` 最小。
- `availability`：可用度最高分；全部 <=0 时回退 cost。
- `speed`：EMA TPS 最高；全部无样本时回退 cost。
- `priority`：Priority 数字最大，同优先级比 TotalCost，再按原候选顺序。
- 渠道策略为空时读全局 `key_selection_strategy`，仍为空则 cost，见 `internal/model/channel.go:546-628`。

### 3.4 AccountPool / PoolAccount / PoolUnschedRule

`AccountPool`：`Name`、`Strategy`（ewma/round_robin/random/least_loaded）、`DefaultConcurrency`、`CooldownBaseSec`、`Enabled`。见 `internal/model/pool.go:9-23`。

`PoolAccount` 可分为（`internal/model/pool.go:25-75`）：

- 凭据/目标：`Platform`、`Type`、`Models`、`Credentials`（加密 JSON）、`BaseURL`、`Extra`。
- 调度资格：`Status`、`Schedulable`、`Priority`、`Weight`、`Concurrency`、`LoadFactor`。
- 并发/代理：`ProxyConfigID`、`ProxyFallbackOriginID`、Extra.BackupProxyConfigID。
- 健康窗口：`RateLimitResetAt`、`OverloadUntil`、`TempUnschedUntil/Reason`、`AuthErrorCount/WindowStart`、`TokenExpiresAt`、`ExpiresAt`。
- 统计：`TotalRequests/Errors/Tokens`、`LastUsedAt`、`ErrorMessage`。

`IsSchedulable` 同时要求 active+schedulable，并排除限流、过载、临时禁用、账号过期和 OAuth access token 即将过期（提前 60 秒），见 `internal/model/pool.go:202-230`、`:266-276`。

`PoolUnschedRule` 以可选 HTTP 状态码或错误体关键字匹配，`DurationMinutes` 决定临时不可调度时间，`SortOrder` 小的启用规则先命中，见 `internal/model/pool.go:140-163`、`internal/op/pool/rules.go:65-107`。

注意主 relay 调用 `SelectAccountLease(..., fallback=1, ...)`，见 `internal/relay/relay.go:1761`；该主路径没有读取 `AccountPool.DefaultConcurrency` 作为 fallback。账号 `LoadFactor>0` 时优先，否则 `Concurrency>0`，再否则实际为 1，见 `internal/model/pool.go:252-287`、`internal/relay/poolscheduler/lease.go:40-45`。`SelectAccountLease` 的账号查询也未检查 `AccountPool.Enabled`，其 SQL 资格过滤以 PoolAccount 字段为准，见 `internal/op/pool/pool.go:256-274`。

### 3.5 Model / ModelMapping / Price / Endpoint / Node

- **LLMInfo/LLMPrice**：模型名主键及 input/output/cache_read/cache_write 四价；`PriceManual` 防止手工价格被同步覆盖。见 `internal/model/llm.go:5-22`。
- **ModelMapping**：`Pattern`、`MatchType(exact/wildcard/regex)`、`TargetModel`、`Priority`、`Enabled`、`ScopeGroupID`。优先级高者先匹配，同优先级 ID 小者先；ScopeGroupID 为空是全局规则，非空只作用指定 Group。见 `internal/model/model_mapping.go:5-27`、`internal/op/modelmapping/model_mapping.go:25-50`、`:177-200`。
- **ModelPriceCategory**：无精确有效价时的分类兜底，支持 exact/prefix/contains，`SortOrder` 小者优先。见 `internal/model/price_category.go:5-32`。
- **ModelPriceSchedule**：峰谷价、OffPeakMul、WeekendOffPeak、两个北京时间半开窗口、SortOrder/Enabled。见 `internal/model/price_schedule.go:5-36`。
- **EndpointType**：包括 `*`、chat/deepseek/mimo/responses/messages、embeddings、rerank/moderations、image/audio/video/music/search。空值归一为 chat；对话家族为 chat/deepseek/mimo/responses/messages。见 `internal/model/endpoint_type.go:5-64`。
- **Node**：仅 ID/Name/Enabled/TokenHash/时间；Channel.NodeID 建立绑定。见 `internal/model/node.go:5-13`。
- **WSResponseAffinity**：设计上保存 Responses response id  hash 对应的 Group/Channel/Key/UpstreamModel/ExpiresAt；运行时接线情况见第 8 节。见 `internal/model/ws_affinity.go:5-19`。

## 4. 分组匹配、GroupMode 与分组字段语义

### 4.1 Group.Name / MatchRegex / EndpointType 查找

缓存发布时，Group 先按 ID 排序，精确名称写入 `groupMap`，正则使用 regexp2 ECMAScript 编译且 MatchTimeout=250ms，见 `internal/op/group/group.go:48-105`。每次查找：

1. 先 `groupMap[endpointType+"\x00"+name]` 精确命中；
2. 再按顺序返回第一个正则命中的 Group；
3. 对话类型有固定 lookup order，例如 Chat 为 chat→deepseek→mimo→responses→messages；模型名含 deepseek/mimo 会把相应类型提前；
4. 最后尝试 `*` 分组；
5. 返回前过滤掉指向缺失/禁用渠道的 GroupItem。

证据：`internal/op/group/group.go:289-344`、`:391-430`、`:657-677`。

### 4.2 五种 GroupMode

| 模式 | 实现 | 精确语义 |
|---|---|---|
| round_robin | `RoundRobin.Candidates`；正常 Group.ID>0 走 `groupRotationCandidates` | 每个分组独立维护 next 游标，从当前偏移开始环绕排列完整候选；游标随请求推进。见 `internal/relay/balancer/group_rotation.go:30-60`。 |
| random | `Random.Candidates` | 对完整候选副本 `rand.Shuffle`，首个是随机起点，后续候选仍可用于失败转移。见 `internal/relay/balancer/balancer.go:62-76`。 |
| failover | `Failover.Candidates→sortByPriority` | Priority 小者先，Priority<=0 视为最大整数排最后；失败后顺序降级。见 `internal/relay/balancer/balancer.go:78-86`、`:238-252`。 |
| weighted | `smoothWRRSequence + groupRotationCandidates` | Weight<=0 按 1；生成 Smooth WRR 周期序列并按分组游标旋转，权重高者在一个周期内出现次数更多且平滑穿插。序列硬上限 1000，过大权重反复约半压缩。见 `internal/relay/balancer/weighted.go:9-135`。 |
| auto | `Auto.Candidates + autoPostSortShuffle` | 样本不足优先探索；样本充足按成功率、延迟/TTFT、可选价格评分排序；全熔断渠道降到最后但不从列表删除。见 `internal/relay/balancer/balancer.go:98-208`。 |

未知 Mode 默认 RoundRobin，见 `internal/relay/balancer/balancer.go:28-44`。Group.ID<=0 的兼容路径使用包级 RoundRobin/Weighted counter；正常持久分组使用每分组 rotation。

Auto 具体评分：

- `minSamples` 默认 10；时间窗默认 300 秒；ring buffer 默认 100 条。
- 探索阶段：未达到 minSamples 者在前，样本少者更前，再按 Weight 降序/Priority 升序。
- 利用阶段基础分：`successRate*(1-latencyWeight)+latencyScore*latencyWeight`，latencyWeight 默认 0.3，`latencyScore=max(0,1-latencyMs/5000)`。
- 若 TTFT 权重大于 0且该渠道有 TTFT EMA，则以 TTFT 替代总延迟作为 latencyMs；代码用“是否大于 0”作为启用判断，不是再按 latencyWeight×ttftWeight 做一次独立混合。见 `internal/relay/balancer/balancer.go:116-156`。
- 若 priceWeight>0 且有价格：`score=score*(1-priceWeight)+priceScore*priceWeight`；`priceScore=0.5-log10(inputPrice)/6`，被限制到 (0,1]。见 `internal/relay/init_hooks.go:42-60`。
- 全部进入利用阶段后，默认将评分差 0.05 内的相邻候选桶内洗牌；`exploreRate>0` 时还可按概率做 temperature=1 的 softmax 不放回重排。见 `internal/relay/balancer/auto_shuffle.go:15-40`、`:53-120`。

### 4.3 其他分组字段

- **Disposable 优先**：Iterator 在策略排序后稳定地把 disposable=true 渠道提前；优先级为 sticky > disposable > 普通策略顺序。见 `internal/relay/balancer/iterator.go:60-99`。
- **Condition**：见第 9 节。
- **FirstTokenTimeOut**：只在流式读取中启动，触发后关闭 response.Body 并作为 timeout 换候选；见 `internal/relay/relay.go:1136-1146`、`:1261-1267`。
- **AttemptTimeOut**：每次 forward 的派生 context 超时，覆盖请求和响应处理；媒体路径也单独实现。见 `internal/relay/relay.go:633-644`、`internal/relay/media_relay.go:591-635`。
- **ThinkingMode**：auto 保留客户端参数；on/off 克隆请求、清理冲突 reasoning 字段，并分别将 ReasoningEffort 规范为 high/none，adapter 再翻译成上游协议。见 `internal/transformer/model/thinking.go:8-42`、`internal/relay/rewrite.go:42-50`。
- **ReasoningBufferStrategy**：buffer 在未出现可见内容前不写 reasoning-only chunk，允许空输出安全重试，软上限 8MiB；immediate 实时写出，但空输出不能安全重试。分组值优先于全局，最终默认 immediate。见 `internal/relay/type.go:29-34`、`:108-128`、`internal/relay/relay.go:1074-1077`、`:1361-1388`。

## 5. 候选选择与均衡：`internal/relay/balancer` 文件职责

| 文件 | 职责与关键参数 |
|---|---|
| `balancer.go` | Balancer 接口、五种策略入口、RR/Random/Failover/Auto 基础实现；Auto 成功率/延迟/价格评分和全熔断降权。 |
| `weighted.go` | Smooth WRR 序列、有效权重、1000 长度上限和权重压缩、全局兼容 counter。 |
| `group_rotation.go` | 正常 Group 的每分组 rotation 状态；Weighted 序列生成后按组推进；超过 1024 个状态时清理一小时未用项。 |
| `iterator.go` | 策略排序、API Key 渠道黑名单、disposable/sticky 提前、候选遍历、skip/熔断/真实尝试日志；每轮最多保留 200 条 skip 明细，日志总量另有上限。 |
| `auto_shuffle.go` | Auto 利用阶段同分桶洗牌和 softmax 探索；bucketTolerance 默认 5 评分点、exploreRate 默认 0。 |
| `stats.go` | `(channel,model)` 的成功率滑窗、总时延与 TTFT EMA(alpha=0.3)、快照和空闲清理；模型名小写归一。 |
| `persistence.go` | Auto/Circuit 状态在 Redis（有实时值时优先）与 DB 间保存/恢复；HalfOpen 持久化为 Open；TTFT 不持久化。对应结构见 `internal/model/runtime_state.go:1-26`。 |
| `circuit.go` | `(channel,key,model)` 的 Closed/Open/HalfOpen；阈值、指数冷却、单 HalfOpen 试探和丢失试探超时。 |
| `key_cooldown.go` | `(channel,key,model)` 错误冷却，Redis TTL 优先、内存回退；400/422 不冷却，默认 300 秒，0 关闭。 |
| `key_availability.go` | Key 可用度 0–100，成功+5、每分钟懒恢复+1；401/403扣100、429扣15、5xx/408扣10、其他>=400扣10，400/422不扣。虽然声明了网络扣分常量，但 `penaltyForStatusCode` 对 statusCode=0 当前返回 0，非 HTTP 错误按现有实现不因该函数扣分。 |
| `key_speed.go` | 成功请求 outputTokens/attemptSeconds 的 TPS EMA(alpha=0.3)，供 speed 策略查询。 |
| `session.go` | API Key+路由键的 sticky Channel/Key/UpstreamModel，TTL 懒过期与低成功率逃生。 |
| `key_builder.go` | 构造 `channel:key:model`、`apiKey:routeKey` 等 map key 及清理前缀。 |

### 5.1 熔断器细节

- Closed 连续失败达到阈值（默认 5）→ Open；Open 在冷却期内硬跳过。
- 冷却：`base*2^(tripCount-1)`，默认 base=60s、max=600s；Open 状态不会因继续收到失败而刷新 LastFailureTime。
- 冷却到期后的第一次 `IsTripped` 将 Open→HalfOpen 并放行一个探测；HalfOpen 中其他请求跳过。
- 探测成功→Closed 并清零；失败→Open，TripCount++，冷却加倍。
- HalfOpen 默认 60 秒探测超时，防止客户端断连等未记账路径使渠道永久卡死；0 禁用此保护。
- Auto 只用只读方式判断“该 channel+model 的所有启用 Key 是否均熔断”，用于降权，不提前触发 HalfOpen。

证据：`internal/relay/balancer/circuit.go:63-171`、`:173-287`。

### 5.2 冷却、失败提示和熔断不是同一层

- **Key cooldown**：在选 Key 前就排除，按模型隔离，默认 300 秒。
- **failure hint**：401/403 默认 10 秒、429 默认 5 秒、网络错误默认 2 秒的短期跳过；429 还与 ratelimitCooldown 取较小值。可写 Redis，见 `internal/relay/retry_cache.go:22-41`、`:157-207`、`:217-304`。
- **circuit breaker**：连续失败达到阈值后的状态机，时长指数退避。
- 三者的 map key 都是 `(channelID,keyID,modelName)`，但触发条件与持续时间不同。

## 6. 账号池与调度

### 6.1 `SelectAccountLease` 选择顺序

`internal/relay/poolscheduler/lease.go:75-127`：

1. 若 `sessionHash` 非空，先查 `(poolID,sessionHash)` sticky；账号被排除、不可调度、模型不匹配、EWMA 触发 escape 或槽位不足则继续普通选择。
2. `pool.ListSchedulableAccounts` 读取 active+schedulable 账号，并由 `IsSchedulable` 做时间窗口过滤。
3. 依次按 excludeIDs、Models 精确列表、可选分层 `priority>=pool_min_priority` 过滤。
4. 过滤当前并发达到 EffectiveLoadFactor 的账号；若没有候选，尝试为匹配的过期 OAuth 账号异步触发刷新，返回 `ErrNoAvailableAccount`。
5. 按 AccountPool.Strategy 选一个，使用原子 CAS 获取槽位；失败则移除该账号继续尝试。
6. 成功时建立 sticky（发生 sticky escape 时不覆盖旧绑定），返回带 `sync.Once` 的 `AccountLease`，确保释放一次且释放的是实际获取的计数器。

### 6.2 四种池策略

实现位于 `internal/relay/poolscheduler/scheduler.go:649-817`：

- `round_robin`：每池原子 counter 对候选数取模。
- `random`：`rand.IntN`。
- `least_loaded`：当前占用槽位 / EffectiveLoadFactor 最小，平局取首个。
- `ewma`（默认）：成功/失败以 alpha=0.3 更新 errorRate，TTFT 同样 EMA；基础分 `0.7*errorRate + 0.3*(ttftMs/10000)`，分数越低越优。再减 `Weight*0.001` 和 `Priority*0.001`，所以权重/优先级高者占优。
- 可选因子：`pool_scheduler_weight_reset` 对较近 ExpiresAt/RateLimitResetAt 给予减分；`pool_scheduler_weight_quota` 按额度快照剩余比例减分。默认都为 0，不启用，也不会解密额度快照。

`ReportResult` 同步更新内存 EWMA，DB 累计通过 1024 队列+4 个 worker 异步写；队列满会丢弃 best-effort DB 计数并记录 dropped 数，内存评分已先更新。见 `internal/relay/poolscheduler/scheduler.go:47-58`、`:106-205`。

### 6.3 失败健康反馈

主对话 relay 在失败后（`internal/relay/relay.go:1884-1923`）：

- `ReportResult(false)` 并无条件释放 lease。
- 429：优先 `Compute429Reset` 解析上游头；没有可信证据时用 `AccountPool.CooldownBaseSec`（默认 300 秒）。OpenAI x-codex 窗口只有 used_percent>=100 才信 reset，其后依次 Retry-After、anthropic unified reset，最长 clamp 366 天。见 `internal/relay/poolscheduler/ratelimit_headers.go:27-158`。
- >=500：OverloadUntil=当前+60 秒。
- OpenAI 403：180 分钟窗口计数，达到 3 次设 `status=error`；未满通常临时禁调 10 分钟。非 OpenAI 403 默认临时禁调 10 分钟。
- OAuth 401：无 refresh_token 直接 error；有 refresh_token 临时禁调，留刷新窗口。非 OAuth 401 直接 error。
- `PoolUnschedRule`：403 计数后、默认冷却前匹配；其他状态码只有命中规则才处理。关键字大小写敏感，使用错误体前 2KiB。见 `internal/relay/pool_auth_error.go:36-134`、`internal/op/pool/rules.go:65-89`。
- 代理拨号失败：如账号配置了 BackupProxyConfigID，则保存原 ProxyConfigID 到 ProxyFallbackOriginID 并切备用代理；没有主代理/备份时不自动直连。见 `internal/relay/pool_proxy_fallback.go:12-65`、`internal/op/pool/proxy_fallback.go:19-72`。

补充：`internal/relay/pool.go` 不是账号池调度实现，它只是 `strings.Builder` 和 `bufio.Reader` 的 sync.Pool；真正调度在 `poolscheduler` 与上述 relay pool 文件中。

## 7. 重试、错误分类与失败转移

### 7.1 循环层级

`executeRelay` 的结构是：

```text
routeRound（默认最多 2 轮）
  └─ GroupItem（GroupMode 决定的顺序）
       └─ keyRound（默认最多 4 次）
            └─ outbound adapter（由 OutboundFormat 决定）
```

- `relay_retry_count=0` 仍有 1 次 Key 尝试；设置注释中的“重试次数”不包含第一次。
- 同一候选内 `failedKeyIDs` 同步用于 inline Key 或 PoolAccount 排除。
- failure hint/熔断跳过通过 `keyRound--` 不消耗 Key 预算，但仍受总决策上限约束。
- `relay_max_total_attempts` 统计真实转发+skip+熔断决策，默认 64；不是只统计网络请求。
- 某 route round 若完全无真实转发，下一轮直接快速失败；最终日志最多再截到 256 条 attempts。证据：`internal/relay/relay.go:1617-1638`、`:1657-1664`、`:2011-2035`、`internal/relay/metrics.go:25-30`。

### 7.2 错误分类表

`ClassifyRelayError` 见 `internal/relay/type.go:406-580`：

| 情况 | Scope | 行为 |
|---|---|---|
| 2xx 且无错误 | ScopeNone | 成功结束。 |
| 400 | ScopeNone | 不重试；尽量把上游状态与错误体回给客户端。 |
| 401 / 403 / 429 | ScopeSameChannel | 同候选排除当前 Key/账号后重选。 |
| 空输出 | ScopeSameChannel | 换 Key；不计渠道故障；受 `retry_empty_output` 控制。 |
| 404 | ScopeNextChannel | 换下一 GroupItem。 |
| 500 / 502 / 503 / 504 | ScopeNextChannel | 换下一 GroupItem。 |
| 2xx 但 transformer 出错、其他 HTTP 状态 | ScopeNextChannel | 保守换候选。 |
| 超时 / 网络 / 其他非 HTTP 错误 | ScopeNextChannel | 换候选；同 host 换 Key无意义。 |
| 已写流式响应后失败 | ScopeAbortAll | 停止重试，避免重复写下游。 |
| 客户端断连 / 关键词拦截 / 本地插件失败 | ScopeAbortAll + SkipFailureAccounting | 终止，且不当成渠道健康证据。 |
| Node execution uncertain | ScopeAbortAll + SkipFailureAccounting | 为避免非重放隧道产生重复执行，不重试。 |

inline 渠道只在最终结果为 ScopeNextChannel 或 ScopeAbortAll 且未豁免时调用 `RecordFailure/RecordAutoFailure`；因此 401/403/429 的 SameChannel 决策本身主要影响 cooldown/availability/pool EWMA，不直接累计该熔断器。见 `internal/relay/type.go:363-373`、`internal/relay/relay.go:1925-1938`。

### 7.3 429 hold 与 adapter fallback

- 默认 429 立即换 Key/渠道；启用 `rate_limit_hold_enabled` 后，在同渠道等待 `rate_limit_hold_interval`（默认 10 秒），总预算 `rate_limit_hold_max_wait`（默认 60 秒），hold 不消耗 keyRound，并清除刚写的 inline Key cooldown；预算耗尽换下一候选。见 `internal/relay/rate_limit_hold.go:12-89`、`internal/relay/relay.go:1951-1997`。
- adapter fallback 只在 `ScopeNextChannel`、未写响应且还有 adapter 时发生；SameChannel 和 ScopeNone 不换 adapter。中间 adapter 失败不立即写冷却/可用度/渠道失败，最终 adapter 才记账。见 `internal/relay/relay.go:94-115`、`:1833-1872`、`:1925-1938`。

## 8. 会话保持与亲和

### 8.1 Group sticky（balancer/session.go）

- key 不是单纯 `apiKeyID:model`，而是 `apiKeyID:stickyRoutingKey`；`stickyRoutingKey` 包含 Group.ID、EndpointType、客户端模型、Group generation、ModelMapping generation，避免配置变化后继续粘旧路由，见 `internal/relay/model_identity.go:14-19`。
- `SessionKeepTime>0` 时 Iterator 查 sticky，命中后把 Channel 提前；SessionEntry 同时保存 ChannelKeyID 和 UpstreamModel，但 Iterator 只用 ChannelID 做提前，Key 仍按当前选择/健康规则确定。见 `internal/relay/balancer/iterator.go:82-99`。
- 成功时 `SetStickyModel` 更新 Timestamp。TTL 过期懒删除；样本达到 Auto minSamples 且成功率<0.3 时删除 sticky，回到正常策略。见 `internal/relay/balancer/session.go:25-104`。
- sticky 仍可能被 API Key 黑名单、渠道禁用、Key 冷却、熔断等硬规则拦截，并非强制无条件使用。

### 8.2 Pool sticky

- 主 relay 的 sessionHash 是 `apiKeyID:requestModel`，pool sticky key 再加 poolID；和 Group sticky 是两套独立 map。
- 没有 Group.SessionKeepTime 那样的业务 TTL 判断；用 LastActivity 供后台清理，命中时续期。
- 可选 `pool_sticky_escape_enabled`：TTFT 超阈值（默认 15000ms）或 errorRate 超阈值（默认 0.5）时临时绕开旧账号但保留旧绑定；有替代账号才排除 escape 账号，无替代仍可回退。见 `internal/relay/poolscheduler/lease.go:75-127`、`internal/relay/poolscheduler/scheduler.go:443-547`。

### 8.3 可恢复 stream session（不是选路 sticky）

`request_session.go + stream_session.go` 处理的是同一生成过程的断线重连/事件重放：

- 仅流式且带 ConversationID 时启用；ConversationID 可来自 header/query/body，最长 128 runes 且拒绝控制字符。
- session key 包含 ConversationID、API Key 和请求 hash；请求 hash 还包含 endpoint/inbound 类型，剔除 conversation/resume 字段后计算。
- 已存在会话直接 Snapshot+Subscribe，不重新执行路由；相同 Conversation 的不同活跃请求会冲突。
- 默认 TTL 30 分钟、4096 events、4MiB、512 sessions；完成会话实际保留窗口最多 2 分钟；断连后最多继续生成 30 秒。见 `internal/relay/request_session.go:114-172`、`internal/relay/stream_session.go:43-125`、`:173-227`、`:587-648`。

### 8.4 WSResponseAffinity 当前边界

`WSResponseAffinity` 表结构包含 Responses 亲和所需的完整路由信息，但全仓 Go 源码检索只发现 AutoMigrate 和 `WSResponseAffinityCleanup` 过期删除，未发现运行时创建、查询或 Iterator/relay 使用。因此**不能认定该机制当前已接入选路**。证据：`internal/model/ws_affinity.go:5-19`、`internal/op/ws_affinity.go:11-18`、`internal/db/db.go:349`。

另外，任务中提到的 `internal/relay/session.go` 在当前工作区不存在；实际组级 sticky 文件是 `internal/relay/balancer/session.go`。

## 9. 条件路由与 AI 路由生成

### 9.1 Condition 求值

`Group.Condition` 是 `[{"key":"model","op":"contains","value":"gpt-4"}]` 形式数组，所有规则 AND，空条件/空数组通过。见 `internal/relay/condition/evaluator.go:12-47`。

可用 key：

- `model`、`api_key_id`、`api_key_name`、`content_type`；
- `hour`：当前 UTC 小时 0–23，不是本地时区；
- `header_*`：header 名转小写后查找。

可用 op：`equals`、`not_equals`、`contains`、`not_contains`、`starts_with`、`ends_with`、`regex`、`in_list`、`gt`、`lt`。equals/in_list 忽略大小写；contains/starts/ends 先转小写；regex 用 regexp2 ECMAScript，250ms 超时；未知 key/op、非法 regex、非法 JSON均不匹配。gt/lt 用 `strconv.Atoi`，解析失败按 0。证据：`internal/relay/condition/evaluator.go:49-128`、上下文构造见 `internal/relay/condition_context.go:11-21`。

### 9.2 AI 路由不是在线选路，而是离线生成 Group/GroupItem

管理 API 为 `/api/v1/route/ai-generate` 及 status/progress/result/stream，要求分组写权限；请求为 `{scope:"group"|"table", group_id, category_strategy}`。见 `internal/server/handlers/route.go:19-68`、`internal/model/ai_route.go:32-44`、`:198-220`。

执行链：

1. `StartGenerateAIRoute` 防同 scope 活跃任务冲突，持久化 `AIRouteTask`，后台总超时 30 分钟，维护进度/心跳/SSE。见 `internal/helper/ai_route.go:20-206`。
2. `GenerateAIRoute` 收集所有启用 Channel 的 Model+CustomModel，按 channelID+模型去重，形成 AI 输入与合法模型白名单。见 `internal/op/airoute/route.go:170-199`、`:445-494`。
3. 按能力类型分桶，桶过大按模型家族和 `ai_route_max_models_per_request`（默认 120）拆分；按 `ai_route_parallelism`（默认 3）并发。见 `internal/op/airoute/route.go:496-614`、`:1015-1119`。
4. 每个桶通过 OpenAI Chat Completions 调分析服务，temperature=0，首选 `response_format=json_object`；上游 400 且错误涉及 response_format 时去字段重试。Prompt 要求输出 `routes[{requested_model,items[{channel_id,upstream_model,priority,weight}]}]`，且只能使用输入 channel/model。见 `internal/op/airoute/route.go:779-890`、`:953-1013`。
5. AI 服务池从 `ai_route_services` 读取，空时回退旧版 base_url/api_key/model 三项；按 round-robin+最少 inflight 选服务，retryable 错误冷却并切换。见 `internal/op/airoute/servicepool.go:69-207`、`:230-290`。
6. 对漏归类的输入会追问一轮；结果 normalize/去重后，`validateAIRouteItems` 严格校验 channelID 和该渠道上游模型确实存在，重排 Priority 为 1..N，Weight<=0 改 100。见 `internal/op/airoute/route.go:684-710`、`:1367-1439`。
7. group scope：选目标 Group 同名 route，同步 GroupItem；不在 AI desired 集合里的旧 item 会删除。table scope：逐 route 更新已有 Group 或创建新 Group；新 Group 固定 `GroupModeRoundRobin`、FirstTokenTimeOut=0、SessionKeepTime=0。部分 route 失败时可保留已成功写入并返回 partial failure。见 `internal/op/airoute/route.go:201-277`、`:279-443`、`:1805-1883`、`:1885-1974`。

`category_strategy` 支持空、vendor、channel_tier；vendor 由 AI 标注厂商，channel_tier 由本地渠道层级解析。分类最终写到 Group.Category，影响 API Key 分类权限。见 `internal/model/ai_route.go:15-30`、`internal/op/airoute/route.go:188-198`、`:258-262`。

## 10. 模型映射、归一化、价格与模型市场

### 10.1 请求时 ModelMapping

`resolveRelayModel` 的顺序（`internal/relay/model_identity.go:28-66`）：

1. `zen/<upstream>` 为特殊形式，跳过普通 mapping；候选渠道类型还会按 upstream 名称做偏好限制。
2. 否则先用 `ResolveGlobal` 找全局（ScopeGroupID=nil）映射，用映射后的 route 名找 Group。
3. 若映射后的 Group 不存在且确实发生改名，回退用原客户端名找 Group；若成功，保留 mapped 为 `identity.override`，即“按原名称选分组、按映射名发上游”。
4. 命中后再找该 Group 专属映射；若存在，它覆盖所有 GroupItem.ModelName。
5. 没有 override 时，普通候选使用 GroupItem.ModelName；zen 使用 `zen/` 后的真实上游名。

exact 忽略大小写，wildcard 支持 `*`/`?`，regex 用 Go regexp 并在缓存初始化时编译；`ResolveGlobal` 和 `ResolveGroup` 都只接受各自 scope，不混用。见 `internal/op/modelmapping/model_mapping.go:177-255`。

### 10.2 model_normalize 的作用边界

`modelnormalize.NormalizeWithRulesDetailed`：

- 先按显式 variant→canonical 规则做全名匹配，再尝试安全基名匹配；
- 然后移除路径/路由商前缀和功能性后缀，最终小写；
- 内置前缀如 dmxapi-/agent-/openai-/anthropic-，后缀如 -cc/-free/-latest，可被设置覆盖；
- 后缀可字面或正则，显式映射可保护 -thinking 等语义变体。

见 `internal/op/modelnormalize/normalize.go:94-145`、`:182-328`、`:341-399`。

它用于模型市场去重、AI 分批和名称分析；正常 relay 的改名应使用 `ModelMapping`。`/api/v1/model/normalize/rules|generate|coverage` 管理这些规则，见 `internal/server/handlers/model.go:49-70`。

### 10.3 价格链与 Auto 成本因子

`price.GetLLMPrice` 顺序（`internal/price/price.go:106-132`）：

1. DB `LLMInfo` 非全零精确价；
2. 外部同步/内置 preset 精确价；
3. `PriceCategoryMatch` 第一条启用分类；
4. provider 前缀剥离、整词最长模型名兜底；
5. DB 全零占位行或 nil。

`PriceScheduleMatch` 命中后，计费通过 `EffectiveLLMPrice` 使用规则高峰价；不在高峰窗口则四价×OffPeakMul，WeekendOffPeak 时北京时间周末全天空闲。未命中 schedule 回退 GetLLMPrice。见 `internal/price/deepseek_schedule.go:44-97`。

计费在 `RelayMetrics.SetInternalResponse` 中按最终 actualModel 和请求开始时间计算 input/output/cache 成本，见 `internal/relay/metrics.go:70-99`。

Auto 的 `AutoPriceFunc` 调的是 **GetLLMPrice 基础价**，只取 Input，不调用 EffectiveLLMPrice，故峰谷价不直接参与 Auto 选路，见 `internal/relay/init_hooks.go:42-60`。

### 10.4 模型市场与 `/api/v1/model/*`

`/api/v1/model` 提供 list/create/update/delete、channel、market、capabilities、normalize、price-category、price-schedule 等管理接口；公开 `/v1/models` 则按 Group 列出可路由名称，并应用 API Key 的分类和模型白名单。见 `internal/server/handlers/model.go:27-138`、`:140-220`。

`/api/v1/model/market?dedupe=true|false` 汇总价格目录、渠道声明模型、启用 Key 数和 StatsModel 成功率/延迟；dedupe 默认读 `model_normalize_market_dedupe_default`（默认 true）。它既包含价格目录模型，也包含仅有渠道声明但无价格行的模型。见 `internal/op/model_market.go:66-132`、`:168-378`。模型市场是管理/展示目录，不是额外的在线选路层。

## 11. 端点、出站格式与节点隧道

### 11.1 入站端点

- 对话：`/v1/chat/completions`、`/v1/responses`、`/v1/messages`、`/v1/embeddings`，见 `internal/server/handlers/relay.go:14-48`。
- 媒体 JSON：images/generations、audio/speech、videos/generations、music/generations、search、rerank、moderations；multipart：images/edits、images/variations、audio/transcriptions，见 `internal/server/handlers/media.go:12-103`。
- 模型列表：`GET /v1/models`。

### 11.2 EndpointType / EndpointProvider / OutboundFormat 的分工

- **EndpointType**：决定 Group 查找域、对话家族回退和日志分类；`*` 可兜底。媒体若命中 `*` 分组，还会用 `narrowGroupItemsForEndpoint` 按渠道类型和模型名关键词做启发式能力过滤，见 `internal/relay/endpoint_capability.go:12-150`。
- **EndpointProvider**：提供方兼容改写。对话 openai/deepseek/mimo 可清理 reasoning 字段；媒体另有 image/video/audio/music provider rewrite。媒体特殊值 raw 会保留原始路径和 query。见 `internal/relay/relay.go:1544-1590`、`internal/relay/media_relay.go:656-709`。
- **OutboundFormat**：只决定出站 adapter 链，不等于 EndpointType。

对 OpenAIChat/OpenAIResponse 渠道（`internal/transformer/outbound/register.go:126-175`）：

| 配置 | adapter 顺序 |
|---|---|
| auto（空或未知） | 普通入站 Chat→Responses；Anthropic 入站 Messages→Chat→Responses。 |
| chat | Chat→Responses |
| responses | Responses→Chat |
| messages | Messages→Chat→Responses |
| chat_only / responses_only / messages_only | 仅对应一种，禁止跨格式回退 |
| passthrough | 仅 Passthrough，保留原始 JSON body，禁止 adapter 回退 |
| raw | 仅 Raw，除原始 body 外还保留原始请求路径，仅改 model |

其他渠道类型直接 `[channelType]`。embedding 只允许 `OutboundTypeOpenAIEmbedding`，chat 允许类型集合见 `internal/transformer/outbound/register.go:64-89`。

### 11.3 媒体 relay 复用选路但绕过 transformer

`MediaHandler` 自己完成：提取 model → resolveRelayModel → `*` 能力过滤 → condition → quota → 同样的 route/key 循环 → 直接改 JSON/multipart 的 model 并转发。它复用 GroupMode、Iterator、inline Key/Pool lease、circuit、Auto、sticky 和 ClassifyRelayError，但不走对话 inbound/outbound adapter。见 `internal/relay/media_relay.go:68-434`。

媒体 Pool 仅支持非 OAuth 账号；账号 BaseURL/代理覆盖 Channel，cookie 转自定义头。成功/失败会 ReportResult 和释放 lease，429 使用池基础冷却；这条简化路径没有像对话 Pool 那样完整接入 429 reset header、401/403 auth counter 和 proxy fallback。见 `internal/relay/media_pool.go:26-76`。媒体 Node 渠道直接报“不支持”，见 `internal/relay/media_relay.go:651-653`、`:770-772`。

### 11.4 Node / node/v1 隧道

- `GET /node/v1/connect` 使用独立 Node Bearer token 认证并升级 WebSocket，和 API Key/JWT 无关；管理接口在 `/api/v1/nodes`。见 `internal/server/handlers/node.go:22-41`、`:165-194`。
- Channel.NodeID 非零时，持久化规范化会强制 BaseURL=`https://node.octopus.invalid/v1`、direct 无代理、单个占位 Key，禁止 Pool/Proxy；支持的渠道协议为 OpenAIChat/OpenAIResponse/Anthropic/OpenAIEmbedding。见 `internal/op/channel/node.go:21-57`。
- relay 仍复用正常选路，只是 `ChannelHttpClient` 返回 `nodetransport.Client`。协议只允许 `/v1/models` 和 chat/responses/messages/embeddings，且固定虚拟 origin；见 `internal/nodetransport/protocol.go:25-35`、`:97-120`。
- 隧道是有界、非重放协议；执行结果不确定时 relay 不重试，避免重复推理。

## 12. 远程站点与站点渠道（简述）

这里有两套不同模型：

1. `RemoteSite`（`internal/model/remote_site.go:35-59`）是 Hub 远程站点管理信息，含 SiteType、认证、余额和健康状态；relay 主循环并不直接以 RemoteSite 为候选。
2. `Site/SiteAccount/SiteToken/SiteUserGroup/SiteModel/SiteChannelBinding`（`internal/model/site.go`）是“站点同步 → 本地 Channel 投影”体系，最终生成的普通 Channel/ChannelKey 才进入路由。

同步流程：`sitesync.SyncAccount` 拉取并持久化快照后调用 `ProjectAccount`，见 `internal/sitesync/core.go:32-74`。`internal/site/service.go` 只是这些方法的门面。

`ProjectAccount` 的关键行为（`internal/sitesync/project.go:19-258`）：

- 站点或账号禁用时禁用投影渠道并清理其 GroupItem。
- 按 token group 和 RouteType 分桶，为每桶创建/更新 Channel；BaseURL 来自站点、Keys 来自 ready 且非 masked 的 SiteToken、Models 来自 SiteModel。
- `SiteChannelBinding` 保存 SiteAccount/GroupKey→ChannelID 关系；按平台决定是否拆分 outbound route。
- NewAPI/OneAPI/OneHub/DoneHub/AnyRouter/Sub2API 被视为 OpenAI-only，原生 Anthropic/Gemini/Volcengine 路由会折叠到 OpenAIChat，embedding 仍独立，见 `internal/sitesync/project.go:489-506`。
- 投影 ChannelKey.Managed=true，同步 diff 只删除 managed Key，保留手工 Key，见 `internal/sitesync/project.go:343-417`。
- RouteRawPayload 可保存 `SiteModelRouteMetadata`，其中 EnableGroups 限定模型可投影到哪些 token group；支持端点类型、推断端点类型与 UnsupportedReason 用于投影决策。见 `internal/model/site_route_metadata.go:14-78`、`internal/sitesync/project.go:559-570`。
- RouteType 变化时会重写关联 GroupItem.ChannelID，删除失效 item 并刷新 Group 缓存，见 `internal/sitesync/project.go:582-679`。

因此远程站点不是另一套运行时 balancer；它是在配置面不断生成/修订本报告前述的 Channel、Key 和 GroupItem。

## 13. 关键参数与默认值

默认值主要来自 `internal/model/setting.go:140-258`，运行兜底见对应实现。

| 设置/字段 | 默认 | 对路由的作用 |
|---|---:|---|
| `relay_retry_count` | 3 | 每候选额外 Key 重试 3 次，即最多 4 次；0 表示只试 1 次。 |
| `relay_route_retries` | 2 | 全部候选遍历轮数，最小 1。 |
| `relay_max_total_attempts` | 0→运行兜底 64 | 真实转发+跳过+熔断的总决策上限。 |
| `ratelimit_cooldown` | 300s | inline Key 按模型错误冷却；0 关闭。 |
| `circuit_breaker_threshold` | 5 | 连续失败熔断阈值。 |
| `circuit_breaker_cooldown` | 60s | 指数退避基数。 |
| `circuit_breaker_max_cooldown` | 600s | 熔断冷却上限。 |
| `circuit_breaker_half_open_probe_timeout` | 60s | HalfOpen 丢失探测接管时间；0 禁用。 |
| `failure_hint_ttl_unauthorized` | 10s | 401/403 短期失败提示。 |
| `failure_hint_ttl_rate_limit` | 5s | 429 短期失败提示，且与 Key cooldown 取小。 |
| `failure_hint_ttl_network` | 2s | 网络错误短期失败提示。 |
| `key_selection_strategy` | cost | inline Key 全局默认策略；渠道可覆盖；可用 cost/availability/speed/priority。 |
| `retry_empty_output` | true | 无可见内容时换 Key 重试。 |
| `reasoning_buffer_strategy` | immediate | reasoning-only 全局缓冲策略，Group 可覆盖。 |
| `rate_limit_hold_enabled` | false | 是否同渠道等待后重试 429。 |
| `rate_limit_hold_interval` | 10s | 429 hold 间隔。 |
| `rate_limit_hold_max_wait` | 60s | 429 hold 总预算。 |
| `auto_strategy_min_samples` | 10 | Auto 从探索转利用的样本数。 |
| `auto_strategy_time_window` | 300s | Auto 成功率窗口。 |
| `auto_strategy_sample_threshold` | 100 | 每 channel+model 滑窗容量。 |
| `auto_strategy_latency_weight` | 30 | 延迟评分权重 0.3。 |
| `auto_strategy_ttft_weight` | 0 | >0 且有 TTFT 样本时用 TTFT EMA 替代总延迟输入。 |
| `auto_strategy_price_weight` | 0 | 基础输入价评分权重，0 禁用。 |
| `auto_strategy_explore_rate` | 0 | 利用阶段 softmax 重排概率，0 禁用。 |
| `auto_strategy_bucket_tolerance` | 5 | 同分桶容差 0.05；0 禁用洗牌。 |
| `pool_layered_filter_enabled` | false | 是否启用账号 priority 分层过滤。 |
| `pool_min_priority` | -9999 | 分层阈值，开启后低于该值被排除且无回退。 |
| `pool_sticky_escape_enabled` | false | 是否允许池 sticky 健康逃生。 |
| `pool_sticky_escape_error_rate` | 0.5 | escape 的 EWMA 错误率阈值。 |
| `pool_sticky_escape_ttft_ms` | 15000 | escape 的 EWMA TTFT 阈值，0 关维度。 |
| `pool_scheduler_weight_reset` | 0 | EWMA reset-readiness 因子权重。 |
| `pool_scheduler_weight_quota` | 0 | EWMA 剩余额度因子权重。 |
| `AccountPool.Strategy` | ewma | 账号池选择策略。 |
| `AccountPool.CooldownBaseSec` | 300s | 池 429 无 reset 证据时的基础冷却。 |
| `Group.FirstTokenTimeOut` | 0（字段零值） | 流式首 Token 超时；0 关闭。 |
| `Group.AttemptTimeOut` | 0 | 单次完整尝试超时；0 关闭。 |
| `Group.SessionKeepTime` | 0 | Group sticky；0 禁用。 |
| `stream_session_ttl_minutes` | 30min | 可恢复流会话配置 TTL。 |
| `stream_session_max_events` | 4096 | 单会话事件缓冲上限。 |
| `stream_session_max_bytes_mb` | 4MiB | 单会话字节缓冲上限。 |
| `stream_session_max_sessions` | 512 | 全局会话数；0 在实现中表示不限制。 |
| `ai_route_timeout_seconds` | 180s | AI 分析单次 HTTP 超时。 |
| `ai_route_parallelism` | 3 | AI 分批并发。 |
| `ai_route_max_models_per_request` | 120 | AI 单批模型上限。 |
| `model_normalize_market_dedupe_default` | true | 模型市场默认归一去重。 |
| `OCTOPUS_RELAY_UPSTREAM_TIMEOUT_SECONDS` | 未设→无总超时 | 整个 relay operation 的环境级总超时。 |
| `OCTOPUS_RELAY_PERSISTENCE_TIMEOUT_SECONDS` | 10s | 日志/持久化 context 超时。 |

环境超时实现见 `internal/relay/context.go:15-47`。

## 14. 主要证据索引（文件:行号）

- Group/GroupItem：`internal/model/group.go:28-80`
- Channel/Key 与四种 Key 策略：`internal/model/channel.go:82-180`、`:439-629`
- Pool 数据与可调度性：`internal/model/pool.go:9-75`、`:202-307`
- Group 精确/正则查找与端点顺序：`internal/op/group/group.go:48-105`、`:289-430`、`:657-677`
- 五种 GroupMode：`internal/relay/balancer/balancer.go:21-252`、`internal/relay/balancer/weighted.go:9-135`、`internal/relay/balancer/group_rotation.go:21-73`
- Iterator/sticky/disposable：`internal/relay/balancer/iterator.go:48-250`
- Auto 统计/洗牌：`internal/relay/balancer/stats.go:12-243`、`internal/relay/balancer/auto_shuffle.go:15-211`
- 熔断/冷却/可用度/速度：`internal/relay/balancer/circuit.go:16-331`、`internal/relay/balancer/key_cooldown.go:16-204`、`internal/relay/balancer/key_availability.go:20-260`、`internal/relay/balancer/key_speed.go:9-166`
- 状态持久化：`internal/relay/balancer/persistence.go:19-189`、`:265-480`
- 账号池选择：`internal/relay/poolscheduler/lease.go:12-127`、`internal/relay/poolscheduler/scheduler.go:649-837`
- 池健康：`internal/relay/pool_auth_error.go:13-134`、`internal/relay/poolscheduler/auth_error_counter.go:14-250`、`internal/relay/poolscheduler/ratelimit_headers.go:13-188`
- 主 relay 流程：`internal/relay/relay.go:194-364`、`:397-598`、`:633-718`、`:1610-2035`
- 错误分类与默认预算：`internal/relay/type.go:36-128`、`:305-614`
- 重试辅助与失败提示：`internal/relay/retry_helper.go:13-112`、`internal/relay/retry_cache.go:22-304`
- 429 hold：`internal/relay/rate_limit_hold.go:12-89`
- 会话：`internal/relay/balancer/session.go:9-152`、`internal/relay/request_session.go:12-172`、`internal/relay/stream_session.go:43-227`、`:459-778`
- 条件路由：`internal/relay/condition/evaluator.go:12-128`
- AI 路由：`internal/helper/ai_route.go:20-206`、`internal/op/airoute/route.go:170-443`、`:496-890`、`:1367-1439`、`:1805-1974`
- ModelMapping：`internal/relay/model_identity.go:14-67`、`internal/op/modelmapping/model_mapping.go:157-255`
- 归一化：`internal/op/modelnormalize/normalize.go:94-410`
- 价格：`internal/price/price.go:106-227`、`internal/price/deepseek_schedule.go:44-97`、`internal/relay/metrics.go:70-99`
- 出站 adapter：`internal/transformer/outbound/register.go:17-175`
- 媒体：`internal/relay/media_relay.go:68-434`、`internal/relay/media_pool.go:26-76`
- Node：`internal/op/channel/node.go:21-57`、`internal/nodetransport/protocol.go:25-120`、`internal/server/handlers/node.go:165-194`
- 站点投影：`internal/sitesync/core.go:32-74`、`internal/sitesync/project.go:19-258`、`:489-679`
- 设置默认值：`internal/model/setting.go:16-133`、`:140-258`

## 15. 未确认项与边界声明

1. **WSResponseAffinity 未确认已生效**：只找到模型、迁移和清理，没有找到运行时使用；报告按“未接入当前选路”处理，而不推测其设计目标已经实现。
2. **未做动态运行验证**：本调查仅读源码，未启动服务、未向真实上游发请求，也未验证某个部署数据库里的实际 Group/Channel 配置。
3. **Provider 特化改写未逐 adapter 展开**：本报告覆盖其对选路/出站的影响与主入口，没有逐字展开所有 OpenAI/Gemini/Anthropic/Volcengine adapter 的协议字段映射。
4. **Pool/媒体两条路径不要混同**：对话 Pool 有完整鉴权计数、reset header 与代理回退；媒体 Pool 代码较短，本报告仅按其实际源码陈述，不将对话路径能力自动推定到媒体路径。
5. **运行时状态可能因 Redis/DB 恢复而变化**：Auto 和 Circuit 有持久化/共享状态；本次结论描述算法与默认配置，不代表任一运行实例此刻的健康评分或候选顺序。
