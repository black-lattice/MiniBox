# MiniBox TODO

## MVP 目标

目标能力：

- 输入一个 Clash 订阅链接
- 解析其中可支持的 Trojan 节点和组
- 启动本地 SOCKS5 / HTTP CONNECT 监听
- 通过订阅中的 Trojan 节点访问外网
- 在订阅失败时回退到 last-known-good 缓存

非目标：

- 完整 Clash 规则兼容
- 全量 Clash 节点协议兼容
- TUN / 透明代理
- 动态控制面

## 目标判定

当前结论：

- [x] 主目标已完成到 MVP 实现层
- [x] G2“可验证、可直接交付”的收口工作已基本完成
- [x] G3“结构化日志初始化 + 最小运维面”的发布前收口已完成

判定依据：

- [x] 给定 Clash 订阅链接，程序可加载远程订阅
- [x] 订阅中的 `trojan` 节点和组可进入内部配置模型
- [x] 启动时可将订阅内容与本地监听器模板合并成单一激活快照
- [x] 本地 SOCKS5 入口已可通过 Trojan 节点出站
- [x] 本地 HTTP CONNECT 入口已可通过 Trojan 节点出站
- [x] 订阅翻译失败时可回退到 last-known-good 缓存
- [x] 当前自动化回归测试通过
- [x] `cargo fmt --all --check` 通过
- [x] `cargo clippy --all-targets --all-features -- -D warnings` 通过
- [x] `cargo test` 通过

未完成但不阻塞主目标的事项：

- [ ] 当前运维指标仍保持最小集合，后续可能需要按部署反馈扩展

## 当前状态

已经具备：

- 本地 SOCKS5 / HTTP CONNECT 监听与握手
- direct TCP relay 数据面
- Clash 文档解析与基础翻译链路
- Trojan 节点进入内部配置模型
- Trojan TLS 出站拨号与握手
- 订阅启动时的本地监听器模板合并
- `http://` 和 `https://` 远程订阅加载
- 订阅缓存与回滚
- 默认本地 SOCKS5 / HTTP CONNECT 双监听模板
- 启动输出中的监听地址、激活来源、缓存回退状态、target 解析摘要
- Trojan 会话级端到端测试和 TLS 失败路径测试
- `rustfmt.toml`、`clippy.toml`、GitHub Actions CI

当前缺口：

- 运维面目前只覆盖最小 probe 和 metrics，还不是完整运营面

## P0: 目标对齐

- [x] 明确 MVP 目标是“Clash 订阅 + Trojan 节点出网”
- [x] 确认当前 direct-only 语义不满足目标
- [x] 明确订阅启动模式

验收标准：

- 明确程序如何同时获得“订阅节点”和“本地监听器”
- 明确 MVP 仅承诺 Trojan，不承诺其他节点协议

## P1: 启动与配置模型

目标：让运行时拥有完成 MVP 所需的最小配置语义。

- [x] 为内部节点模型加入 `DirectTcp`
- [x] 为内部节点模型加入 `Trojan`
- [x] 为 Clash `trojan` 节点补解析与翻译支撑
- [x] 在配置层提供 `listener target -> concrete node` 的解析能力
- [x] 增加 group 环检测
- [x] 设计并实现“订阅来源 + 本地监听器模板”的启动配置
- [x] 允许本地静态监听器配置与远程订阅节点合并成一个激活快照

验收标准：

- 给定本地监听器配置和订阅内容，可以生成单一 `ActiveConfig`
- listener 可以稳定解析到 Trojan 节点或 Trojan 组

## P2: Trojan 出站拨号器

目标：实现最小可用的 Trojan runtime outbound。

- [x] 新增 Trojan outbound dialer 模块
- [x] 建立到 Trojan 服务器的 TCP 连接
- [x] 建立 TLS 会话
- [x] 实现 Trojan 握手
- [x] 将目标地址按 Trojan 请求格式发送到上游
- [x] 在握手成功后接入现有 relay pipeline
- [x] 为 TLS / 握手失败映射明确的运行时错误

验收标准：

- 对单个 Trojan 节点可成功建立上游连接
- 握手成功后可通过该节点转发 TCP 流量
- 错误日志能区分 TCP 失败、TLS 失败、Trojan 握手失败

## P3: 路由与会话集成

目标：让真实会话路径开始通过 Trojan 节点出网。

- [x] 在会话上下文中携带已解析的 concrete node
- [x] 引入显式 route 规划层
- [x] 在 route 层区分 `DirectTcp` 与 `Trojan`
- [x] 当 listener target 为 Trojan 节点时，走 Trojan dialer
- [x] 当 listener target 为 group 时，先解析到具体节点再决定出站类型
- [x] 为当前未支持的节点类型保留显式错误

验收标准：

- SOCKS5 请求可以通过 Trojan 节点出网
- HTTP CONNECT 请求可以通过 Trojan 节点出网
- 组引用到 Trojan 节点时也能成功建立会话

## P4: 订阅启动可用性

目标：让“给订阅链接即可使用”真正成立。

- [x] 明确 CLI 输入形式
- [x] 支持“订阅 URL + 本地默认监听器模板”启动
- [x] 生成默认本地 SOCKS5 监听
- [x] 可选生成默认 HTTP CONNECT 监听
- [x] 启动时打印实际监听地址和当前激活来源
- [x] 在订阅翻译失败时自动回退到缓存

验收标准：

- 用户只提供订阅相关输入，就能得到可连接的本地代理端口
- 缓存命中与回退行为可观察、可解释

## P5: 端到端验证

目标：证明该项目真的具备“通过 Trojan/订阅节点访问外网”的能力。

- [x] 增加 Trojan outbound 单元测试
- [x] 增加 route 层测试：Trojan 节点、Trojan 组、DirectTcp
- [x] 增加会话级测试：SOCKS5 -> Trojan
- [x] 增加会话级测试：HTTP CONNECT -> Trojan
- [x] 增加订阅集成测试：Clash Trojan 节点 -> 激活快照
- [x] 增加缓存回退测试：Trojan 订阅失效 -> last-known-good

验收标准：

- 测试能覆盖握手成功、密码错误、TLS 失败、订阅翻译失败、缓存回退
- MVP 主路径有完整自动化验证

## 下一目标

目标：把当前 MVP 从“可直接交付”推进到“可稳定发布和运维”。

### G2.1 端到端验证补齐

- [x] 增加本地 Trojan mock server
- [x] 增加会话级测试：SOCKS5 -> Trojan -> target
- [x] 增加会话级测试：HTTP CONNECT -> Trojan -> target
- [x] 覆盖 TLS 失败
- [x] 覆盖密码错误
- [x] 覆盖 Trojan 握手失败

验收标准：

- 本地自动化测试能证明真实会话可通过 Trojan 上游建立并转发
- 失败路径可稳定复现且可断言

### G2.2 启动可用性收口

- [x] 默认模板增加 HTTP CONNECT 监听
- [x] 启动时打印实际监听地址、激活来源、缓存回退状态
- [x] 启动时打印当前 listener 绑定到的 target group/node
- [x] 明确文档中的最小启动示例

验收标准：

- 用户只靠启动输出和 README 就能配置本地客户端
- 默认启动路径不需要读源码也能确认当前代理入口

### G2.3 工程化与交付

- [x] 增加 `rustfmt.toml`
- [x] 增加 `clippy.toml`
- [x] 增加 CI：`cargo fmt --check`
- [x] 增加 CI：`cargo clippy -- -D warnings`
- [x] 增加 CI：`cargo test`
- [x] 文档化当前支持边界：仅 Trojan、无 rules、无 TUN

验收标准：

- 仓库具备稳定的最小交付基线
- 新用户按文档可跑通订阅启动和本地客户端接入

## G3: 发布前收口

目标：补齐失败路径、运维面和部署材料，把当前 MVP 推到可发布状态。

- [x] 补失败路径测试：密码错误
- [x] 补失败路径测试：Trojan 握手失败
- [x] 增加结构化日志初始化，替换当前以 `eprintln!` 为主的启动输出
- [x] 暴露最小运维面：`/healthz`、`/readyz`、Prometheus 文本指标
- [x] 补运行文档：如何用订阅启动、如何配置本地客户端、如何判断缓存回退
- [x] 补部署材料：systemd 或 launchd 示例
- [x] 做一次发布前清单：支持边界、回滚、故障排查

验收标准：

- 失败路径和运维入口有自动化或可重复验证
- 新用户按文档即可完成启动、接入、排障

当前结论：

- [x] G3 已完成

剩余发布前优化：

- [ ] 根据实际部署反馈决定是否扩展更多运行时指标

## G4: 试运行与运维增强

目标：在真实部署反馈下扩展观测面，而不是继续堆协议特性。

- [ ] 增加运行时计数器：listener accept 失败、session 失败分类、Trojan 上游失败分类
- [ ] 增加缓存与订阅指标：fresh translation、cache rollback、translation failure 计数
- [ ] 为 admin surface 增加最小访问日志或命中计数，确认 probe/metrics 实际可用性
- [ ] 审视结构化日志字段，补充 listener target、upstream kind、activation source 的固定字段约束
- [ ] 根据一轮真实试运行结果更新 README、RELEASE 和部署样例

验收标准：

- 可以从 metrics 和结构化日志里区分启动失败、运行失败、订阅回退和上游失败
- 一轮真实部署反馈后，文档和观测字段不再依赖读源码理解

## P7: MVP 后续增强

仅在 MVP 跑通后再做：

- [ ] 增加运维面：结构化日志、metrics、`/healthz`、`/readyz`
- [ ] 支持更多 Clash 节点协议
- [ ] 支持更完整的 group 选择策略
- [ ] 评估安全 reload / refresh
