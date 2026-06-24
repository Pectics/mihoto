# mihoto 开发方向研究结论

## 一、结论

**建议 fork，并把 mihoto 视为架构分叉，而不是 mihoro 的临时补丁集合。**

但不建议将项目定位为简单的“mihoro + TUN”。Linux 侧已经存在支持 TUN、多订阅、mixin、systemd、doctor 等能力的 CLI、TUI 和安装脚本；单独增加 `tun:` 字段不足以构成长期差异。([GitHub][1])

更合适的定位是：

> **mihoto：面向 Linux 的、无界面、事务化、可审计、最小权限的 Mihomo 部署与订阅配置管理器，提供一等 TUN 支持。**

其中真正的产品价值不是“能生成 TUN 配置”，而是：

1. 订阅更新不会破坏当前可用配置。
2. 用户级和系统级部署具有明确、一致的权限模型。
3. TUN 启用失败时不会造成不可恢复的网络中断。
4. 订阅原始配置、用户覆盖配置和最终生效配置彼此分离。
5. 每一次配置变更都能检查、比较、验证和回滚。

因此，开发顺序必须是：

> **安全修复 → 配置分层 → 事务化更新 → 服务后端 → TUN/DNS → 调度和迁移 → v1.0**

而不是先合并 #187 和 #200。

---

## 二、mihoro 当前状态的判断

截至 **2026 年 6 月 23 日**，mihoro 的 `main` 最后提交和最新 `v0.14.0` release 都停留在 **2026 年 4 月 25 日**。更准确的描述是“上游 review 和维护停滞约八周”，目前还不足以认定项目已经废弃。([GitHub][2])

当前实现明确以用户级部署为中心：

* 管理程序安装到 `~/.local/bin`；
* 管理配置位于 `~/.config/mihoro.toml`；
* Mihomo 配置位于 `~/.config/mihomo`；
* systemd unit 位于用户级 systemd 目录；
* `systemctl` 调用被固定为 `--user`。([GitHub][3])

所以你观察到的行为并非遗漏，而是当前架构的基本假设。

上游自己的 v1.0 讨论也已经意识到配置来源、配置分层、失败隔离和迁移等基础问题；TUN、root/system service 和 OpenRC 原本被安排在这些基础改造之后。这个依赖关系是正确的。mihoto 可以把 TUN 提升到自己的 v1.0 范围内，但不能颠倒依赖顺序。([GitHub][4])

---

## 三、当前最严重的技术问题

| 领域     | 当前行为                                              | 风险                                        | mihoto 的处理                            |
| ------ | ------------------------------------------------- | ----------------------------------------- | ------------------------------------- |
| 订阅更新   | 下载内容直接写入生效配置路径，Base64 解码和 override 也在原文件上进行       | 下载中断、解码失败或解析失败可能先破坏当前可用配置                 | 临时文件、候选配置、核心验证、原子切换、失败回滚              |
| 配置覆盖   | 被建模字段直接覆盖，`Option::None` 表示删除，而不是继承订阅值            | 新字段越多，越容易无意删除远端配置                         | 三态覆盖：继承、设置、删除                         |
| 原始配置   | 下载的订阅同时充当工作文件和最终文件                                | 无法重新渲染，覆盖行为会累积                            | source、overlay、effective、last-good 分离 |
| Cron   | enable 会用单行内容替换整个 crontab；disable 使用 `crontab -r` | 直接删除用户原有定时任务                              | P0 修复；长期改为 systemd timer              |
| 服务作用域  | systemd user 被硬编码                                 | 无法可靠支持 host-level TUN 和 boot-before-login | 显式、持久化的 service backend               |
| 控制 API | 默认 `0.0.0.0:9090` 且没有 secret                      | 局域网或其他接口可能直接访问控制 API                      | 默认回环；非回环必须认证                          |
| 文件权限   | 没有显式收紧配置权限                                        | 常见 umask 下可能产生其他本地用户可读的文件                 | 配置和凭据 0600，敏感目录 0700                  |

订阅更新的数据安全问题尤其严重。当前下载函数会截断目标文件后直接写入；随后还可能在同一路径上解码和覆盖。它不具备候选文件、事务边界或 last-known-good。([GitHub][5])

当前 override 实现也不是“只修改用户声明的字段”。它会把支持字段直接赋值到订阅配置，未设置的 `Option` 可能意味着删除。将 DNS、TUN 等大型嵌套对象继续塞入这套模型，会放大这一语义问题。([GitHub][6])

Cron 则属于明确的数据破坏缺陷：当前 enable 写入一个只包含 Mihoro 项目的临时 crontab，再用其覆盖现有 crontab；disable 直接移除整个 crontab。该问题必须在任何 mihoto 公共版本发布前解决。([GitHub][7])

---

## 四、现有 PR 和 issue 的处理决策

### PR 处理矩阵

| 上游项                                           | 建议                       | 原因                                                                           |
| --------------------------------------------- | ------------------------ | ---------------------------------------------------------------------------- |
| `spencerwooo/mihoro#197` Cron enable/disable  | **优先移植并补强**              | 修复方向正确，已经改为读取和保留现有 crontab，也附带测试；但仍应增加稳定 marker、错误分类和备份机制。([GitHub][8])      |
| `spencerwooo/mihoro#200` system-level service | **保留需求，重写实现**            | 当前方案主要增加 `--system` 和 unit 路径切换，但部署路径、作用域检测、服务身份、迁移和权限模型没有完整解决。([GitHub][9]) |
| `spencerwooo/mihoro#187` DNS override         | **拆分 DNS/TUN 后重写**       | PR 实际同时加入 DNS 和 TUN；默认值会主动启用 DNS，并继续使用有缺陷的 `Option` 覆盖语义。([GitHub][10])      |
| `#172/#168/#175` 配置来源和覆盖                      | **作为架构基础纳入 v1**          | 这些问题才是 TUN/DNS 能否可靠实现的前置条件。([GitHub][4])                                     |
| `#198` User-Agent/订阅格式                        | **纳入 profile/source 模块** | 一些订阅服务会根据 UA 返回不同格式；错误响应需要被明确识别，不能只显示 YAML 解析失败。([GitHub][11])               |
| `#190` TUN                                    | **纳入 v1**                | 这是 mihoto 的核心能力，但必须依赖新 overlay 和 system deployment。([GitHub][12])            |
| `#180` OpenRC                                 | **推迟到 v1.1+**            | 会扩大服务后端测试矩阵，不是首个稳定版本的必要条件。([GitHub][13])                                     |

### 对 #197 的具体处理

不要直接不加审查地合并。应在其基础上增加：

* 每条托管任务使用稳定标记，例如
  `# mihoto-managed:update:<profile>`；
* 不能用简单 substring 判断某行是否属于 Mihoto；
* 区分“用户没有 crontab”和“crontab 命令执行失败”；
* 写入前保存备份；
* enable/disable 后重新读取并验证；
* 保证其他行字节级不变；
* 正确处理路径中的空格和 shell quoting。

### 对 #200 的具体处理

#200 中有价值的是“作用域应成为显式概念”，但实现不应继续沿用。

仅为解决注销后用户服务退出，可以使用 systemd linger，使用户 manager 在注销后继续存在。因此系统级服务不应该只是“解决 logout”的替代开关；它应服务于 boot-before-login、全机 TUN、专用服务账户和受控网络权限。([自由桌面][14])

#200 当前实现还存在这些结构性问题：

* `--system` 主要存在于 init 阶段；
* 后续命令依赖 unit 文件是否存在来猜测作用域；
* 核心二进制和配置路径仍然依赖用户 home 路径；
* 没有完整的 user → system 迁移流程；
* 没有专用服务用户；
* 没有能力集最小化和服务沙箱；
* 用户 unit 与系统 unit 的区别基本只剩安装目标。

这会产生一个危险的“看似系统服务，实际上只是 root 环境下的用户布局”。

### 对 #187 的具体处理

可以借鉴：

* 字段命名；
* 嵌套未知字段的 `serde(flatten)` 思路；
* 已添加的测试样例。

不能继承：

* DNS 默认启用；
* DNS 默认监听 `0.0.0.0:5353`；
* TUN/DNS 未声明字段被删除的语义；
* 在当前生效文件上直接修改；
* 把 DNS 和 TUN 作为同一功能提交；
* 试图用有限 Rust struct 覆盖不断扩展的 Mihomo schema。

官方 TUN 配置已经包含 device、stack、dns-hijack、auto-route、auto-redirect、strict-route、MTU、接口/UID/路由包含排除等大量字段，且仍在演进。mihoto 不应通过每次新增 Rust 字段来追赶完整 schema。([虚空终端][15])

---

## 五、推荐的目标架构

### 1. 配置必须分为四层

#### 用户级

```text
~/.config/mihoto/config.toml
~/.config/mihoto/profiles/<profile>/overlay.yaml

~/.local/share/mihoto/profiles/<profile>/source.yaml

~/.local/state/mihoto/profiles/<profile>/
├── generations/
│   └── <generation-id>/
│       └── effective.yaml
├── active
└── last-good
```

#### 系统级

```text
/etc/mihoto/config.toml
/etc/mihoto/profiles/<profile>/overlay.yaml

/var/lib/mihoto/profiles/<profile>/
├── source.yaml
├── generations/
├── active
└── last-good

/run/mihoto/
```

各层语义：

* `source.yaml`：订阅或本地来源的原始配置，不修改。
* `overlay.yaml`：用户明确声明的修改。
* `effective.yaml`：source 与 overlay 合并后的候选结果。
* `active`：当前激活 generation。
* `last-good`：最近通过启动和健康检查的 generation。

`mihoto.toml` 只负责 Mihoto 自身的管理设置，不应继续承载整个 Mihomo schema。

### 2. 更新流程必须事务化

建议固定为：

```text
获取进程锁
  ↓
下载到同文件系统临时文件
  ↓
检查 HTTP 状态、大小、响应格式
  ↓
必要时进行 Base64 解码
  ↓
解析 source YAML
  ↓
应用 overlay
  ↓
生成 candidate generation
  ↓
调用受管理 Mihomo 核心的配置测试模式
  ↓
展示或记录语义 diff
  ↓
原子切换 active
  ↓
重启或 reload
  ↓
控制 API / 进程健康检查
  ↓
成功：更新 last-good
失败：恢复旧 generation 并重启
```

Mihomo 本身提供配置测试模式，可在替换当前配置前用候选文件执行检查。([GitHub][16])

还应包含：

* 下载超时和最大响应体限制；
* HTML、空响应、V2Ray JSON、Base64 和 Mihomo YAML 的明确识别；
* 同一 profile 的进程锁；
* 配置无语义变化时不重启；
* source URL、token、Cookie、Authorization 的日志脱敏；
* 网络失败不能改变 active；
* reload 或重启失败必须回滚。

### 3. 覆盖模型采用三态语义

当前 `Option<T>` 不足以表达：

1. 沿用订阅值；
2. 设置新值；
3. 删除订阅字段。

内部可以建模为：

```rust
enum Override<T> {
    Inherit,
    Set(T),
    Delete,
}
```

用户侧建议使用通用 YAML overlay：

```yaml
tun:
  enable: true
  stack: mixed
  auto-route: true
  auto-detect-interface: true

external-controller: 127.0.0.1:9090

external-ui: !delete
```

规则应固定为：

* overlay 中缺失：继承 source；
* 标量或对象：设置或递归合并；
* `!delete`：显式删除；
* 数组默认整体替换；
* 不进行隐式 append、去重或排序；
* 未知字段始终保留。

数组合并、rules 插入、proxy-provider 合并等高级操作可以以后增加显式操作符，但不应在 v1 中暗中推断。

Typed Rust struct 仍有价值，但只能用于：

* Mihoto 自身配置；
* 常用字段校验；
* TUN/DNS preflight；
* preset 生成；
* 错误提示。

它不应再充当完整 Mihomo 配置 schema。

---

## 六、服务作用域设计

建议建立持久化枚举：

```rust
enum ServiceBackend {
    SystemdUser,
    SystemdSystem,
}
```

作用域应写入 deployment 配置，所有命令统一解析。不要根据 `/etc/systemd/system/mihomo.service` 是否存在进行猜测。

### systemd-user

适用于：

* HTTP/SOCKS/Mixed 代理；
* 单用户工作站；
* 不要求登录前启动；
* 用户愿意通过 linger 让服务跨注销继续运行。

### systemd-system

适用于：

* host-level TUN；
* boot-before-login；
* 多用户机器上的统一代理；
* 需要受控 `CAP_NET_ADMIN`；
* 需要独立服务账户和系统状态目录。

建议使用：

```text
/usr/local/libexec/mihoto/mihomo
/etc/mihoto/
/var/lib/mihoto/
/run/mihoto/
/etc/systemd/system/mihoto-mihomo.service
```

不要直接注册成通用的 `mihomo.service`，否则可能与发行版包或用户已有 unit 冲突。

系统服务至少应包括：

```ini
User=mihoto
Group=mihoto

NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes

CapabilityBoundingSet=CAP_NET_ADMIN
AmbientCapabilities=CAP_NET_ADMIN

DevicePolicy=closed
DeviceAllow=/dev/net/tun rw
```

实际能力集必须通过集成测试确认。Linux 创建和连接 TUN 设备需要 `CAP_NET_ADMIN`；只有在核心实际需要时，才追加 `CAP_NET_RAW`，绑定低端口时才考虑 `CAP_NET_BIND_SERVICE`。([Linux内核文档][17])

systemd 的设备控制可以对 `/dev/net/tun` 进行专门允许，而不是开放整个设备命名空间。([自由桌面][18])

建议的 v1 策略是：

* system backend：完整支持 TUN；
* user backend：默认只保证普通代理模式；
* user backend TUN：仅当 `doctor` 检测到已有合法能力时允许，不自动执行 `setcap`；
* 不为 Mihomo 或 Mihoto 安装 setuid root；
* 不在 v1 中引入长期运行的 root helper daemon。

---

## 七、TUN 和 DNS 的产品边界

### TUN

Mihoto 并不需要自行实现 TUN 设备或路由逻辑。Mihomo 核心负责：

* 创建 TUN；
* 设置 auto-route；
* DNS hijack；
* 路由和接口处理。

Mihoto 负责：

* 渲染配置；
* 准备权限；
* 启动前检查；
* 启动后健康验证；
* 失败回滚；
* 网络恢复。

建议增加：

```text
mihoto doctor tun
mihoto apply --dry-run
mihoto recover-network
```

`doctor tun` 至少检查：

* `/dev/net/tun` 是否存在和可访问；
* 服务身份是否拥有必要 capability；
* 当前默认路由和出站接口；
* `ip` 等必要系统工具；
* DNS 监听端口冲突；
* controller 是否可访问并已配置认证；
* Docker、Podman、虚拟机网桥和 LAN 地址段；
* TUN route include/exclude 是否可能切断 SSH；
* 当前 candidate 是否能通过 Mihomo 配置测试。

`recover-network` 应能够：

1. 停止当前失败实例；
2. 将 active 切回 last-good；
3. 必要时临时禁用 TUN overlay；
4. 恢复核心；
5. 输出恢复结果和失败原因。

### DNS

DNS 必须是独立、显式 opt-in 的能力。

不应：

* 因为增加 DNS struct 就默认 `enable: true`；
* 默认绑定 `0.0.0.0`；
* 自动覆盖订阅已有 DNS；
* 将 DNS 与 TUN 生命周期强耦合。

可以提供显式 preset，例如：

```text
mihoto preset apply desktop-tun
```

但 preset 必须：

* 生成可见的 overlay；
* 显示 diff；
* 要求用户 apply；
* 不在运行时隐藏注入配置。

---

## 八、安全基线

当前 Mihoro 默认将控制 API 绑定到 `0.0.0.0:9090` 且不设置 secret；官方示例使用回环地址，并将 secret 作为 API 访问密钥。mihoto 应直接改变这一默认值。([GitHub][6])

建议规则：

* 默认 `external-controller: 127.0.0.1:9090`；
* 非回环地址且 secret 为空时拒绝 apply；
* 仅通过显式 `--allow-unsafe-controller` 才允许绕过；
* 所有 secret、订阅 URL、Authorization 和 Cookie 在日志中脱敏；
* 用户配置和凭据文件强制 0600；
* 用户敏感目录强制 0700；
* 系统配置采用 `root:mihoto` 和 0640；
* 不在错误报告中输出完整订阅 URL。

当前代码没有显式收紧配置文件权限。在常见 umask 022 下，新建普通文件通常会变成 0644，所以不能假定 subscription URL 和 controller secret 只对当前用户可见。([GitHub][6])

当前 release workflow 还使用了可变 Action 引用，例如 `actions/checkout@master`。这不是首要运行时缺陷，但应在 v1 前改为固定版本或 commit SHA，并加入依赖审计。([GitHub][19])

---

## 九、调度系统

短期必须吸收 #197 的修复方向，解决现有 Cron 数据破坏问题。

长期应将 systemd timer 作为默认后端：

```ini
[Timer]
OnCalendar=...
Persistent=true
RandomizedDelaySec=...
```

这样可以：

* 与 user/system service backend 使用同一套作用域；
* 在错过执行时间后补跑；
* 避免所有订阅实例同时访问服务端；
* 通过 journal 统一记录结果；
* 不再解析和重写用户 crontab。([自由桌面][20])

Cron 只保留为兼容后端，不应再是默认实现。

---

## 十、v1.0 范围

### 必须包含

* URL、本地文件、现有配置三类 source；
* 多个命名 profile；
* 每个 deployment 同时激活一个 profile；
* source、overlay、effective、last-good 分层；
* 通用三态 YAML overlay；
* 事务化 fetch/render/validate/apply/rollback；
* user/system systemd backend；
* user → system 迁移；
* TUN preflight、健康检查和恢复；
* 独立 DNS overlay；
* systemd timer；
* 安全 Cron 兼容；
* controller、secret 和文件权限安全默认值；
* 配置 diff、dry-run、doctor；
* Mihoro 配置导入。

### 明确不进入 v1.0

* GUI 或 TUI；
* sing-box 等多核心支持；
* 节点测速和节点选择 UI；
* 通用订阅转换器；
* 多订阅规则自动合并；
* OpenWrt/路由器部署；
* 多个 Mihomo 实例同时运行；
* OpenRC；
* 常驻特权 helper daemon；
* 自动修改系统 DNS 管理器；
* 自动写入 nftables/iptables 规则。

多 profile 不等于多订阅合并。v1 中应保持“一 profile、一 source、一 overlay、一 effective”的确定性模型。

---

## 十一、建议直接建立的 Milestone 和 Issue

### 标签

```text
priority/P0-blocker
priority/P1-v1
priority/P2-post-v1

area/config
area/source
area/service
area/tun
area/dns
area/scheduler
area/security
area/release

type/bug
type/feature
type/refactor
type/hardening
type/docs
```

### Milestone 1：`v0.1.0 — Safety Baseline`

| 优先级 | Issue                                                                   |
| --- | ----------------------------------------------------------------------- |
| P0  | `Establish fork provenance, rename matrix and protected CI baseline`    |
| P0  | `Preserve unrelated crontab entries when enabling or disabling updates` |
| P0  | `Introduce source, candidate, active and last-good config generations`  |
| P0  | `Implement transactional config activation and automatic rollback`      |
| P0  | `Harden controller defaults, file permissions and secret redaction`     |

Cron issue 引用：

```text
Upstream:
- spencerwooo/mihoro#196
- spencerwooo/mihoro#197

Decision:
- Port and harden; do not blindly merge.
```

### Milestone 2：`v0.2.0 — Config Engine`

| 优先级 | Issue                                                                |
| --- | -------------------------------------------------------------------- |
| P1  | `Implement generic tri-state recursive YAML overlay engine`          |
| P1  | `Add named profiles and explicit active-profile selection`           |
| P1  | `Add URL, local-file and existing-config source adapters`            |
| P1  | `Detect subscription response formats and provide actionable errors` |
| P1  | `Support per-profile User-Agent and authenticated HTTP headers`      |
| P1  | `Add render diff, dry-run and managed-core config validation`        |

主要上游引用：

```text
- spencerwooo/mihoro#168
- spencerwooo/mihoro#172
- spencerwooo/mihoro#175
- spencerwooo/mihoro#189
- spencerwooo/mihoro#198
```

### Milestone 3：`v0.3.0 — Deployment Backends`

| 优先级 | Issue                                                                   |
| --- | ----------------------------------------------------------------------- |
| P1  | `Introduce persisted systemd-user and systemd-system backends`          |
| P1  | `Implement hardened system-level Mihomo deployment`                     |
| P1  | `Add reversible Mihoro-to-Mihoto import and migration`                  |
| P1  | `Add reversible user-to-system deployment migration`                    |
| P1  | `Implement systemd timer scheduler backend`                             |
| P1  | `Add service backend integration tests in systemd-capable environments` |

主要上游引用：

```text
- spencerwooo/mihoro#176
- spencerwooo/mihoro#199
- spencerwooo/mihoro#200
```

### Milestone 4：`v0.4.0 — First-class TUN`

| 优先级 | Issue                                                             |
| --- | ----------------------------------------------------------------- |
| P1  | `Add first-class TUN overlay with raw unknown-field passthrough`  |
| P1  | `Implement TUN capability and route preflight checks`             |
| P1  | `Add post-activation health checks and TUN rollback`              |
| P1  | `Implement recover-network emergency recovery command`            |
| P1  | `Add DNS overlay as a separate opt-in capability`                 |
| P1  | `Add privileged TUN integration tests in VM or network namespace` |

主要上游引用：

```text
- spencerwooo/mihoro#187
- spencerwooo/mihoro#190
- spencerwooo/mihoro#175
```

### Milestone 5：`v1.0.0 — Stabilization`

| 优先级 | Issue                                                               |
| --- | ------------------------------------------------------------------- |
| P1  | `Add concurrent-update locking and crash recovery tests`            |
| P1  | `Add network interruption, invalid subscription and rollback tests` |
| P1  | `Pin CI actions and add dependency/license auditing`                |
| P1  | `Document threat model and privilege boundaries`                    |
| P1  | `Publish Mihoro migration and rollback guide`                       |
| P1  | `Publish supported distributions and systemd compatibility matrix`  |
| P1  | `Complete release-candidate migration testing`                      |

### v1.1 以后

```text
[P2] Add OpenRC service backend
     Upstream: spencerwooo/mihoro#180

[P2] Add conditional HTTP requests with ETag and Last-Modified

[P2] Add explicit list merge operators for rules and providers

[P2] Support multiple simultaneously running deployments

[P2] Evaluate polkit-based privileged operations

[P2] Evaluate multi-source profile composition
```

---

## 十二、Issue 模板建议

```markdown
## Context

说明问题、风险和对用户可见的行为。

## Upstream references

- spencerwooo/mihoro#<issue-or-pr>

## Upstream decision

- [ ] Ported
- [ ] Reimplemented
- [ ] Deferred
- [ ] Rejected

Reason:

## Scope

本 issue 必须完成的行为。

## Acceptance criteria

- [ ] ...
- [ ] ...
- [ ] Tests added
- [ ] Documentation updated
- [ ] Migration impact evaluated

## Out of scope

明确本 issue 不解决的内容。

## Dependencies

- Depends on #
- Blocks #

## Security and rollback considerations

权限、敏感数据、失败恢复和兼容性影响。
```

建议同时维护：

```text
docs/upstream-tracking.md
```

内容至少包括：

| Upstream   | Mihoto issue | Decision                | Commit/PR | Status  |
| ---------- | ------------ | ----------------------- | --------- | ------- |
| mihoro#197 | mihoto#…     | ported and hardened     | …         | done    |
| mihoro#200 | mihoto#…     | reimplemented           | …         | planned |
| mihoro#187 | mihoto#…/#…  | split and reimplemented | …         | planned |

---

## 十三、推荐的实际提交顺序

不要先做一次大规模 rewrite。按以下顺序拆成可审查 PR：

1. Fork 基线、保留许可证和上游历史，完成机械重命名。
2. 固定现有测试基线，避免 rename 和重构混在一起。
3. 修复 Cron 数据破坏问题。
4. 收紧 controller、secret、配置权限和日志。
5. 引入 generation store，不改变现有 CLI 行为。
6. 将下载改为临时文件和事务式激活。
7. 引入 source/overlay/effective/last-good。
8. 实现通用 YAML overlay 和 golden tests。
9. 增加 profile 和 source adapter。
10. 增加格式检测、UA、dry-run、diff、core validation。
11. 引入持久化 ServiceBackend。
12. 实现真正的 system deployment 和迁移。
13. 引入 systemd timer。
14. 实现 TUN overlay、doctor 和 recovery。
15. 单独实现 DNS overlay。
16. 补齐 VM、systemd、TUN、回滚和迁移测试。
17. 进入 `v0.9.0-rc.1`，只接受 bug、安全和文档改动。
18. 满足 release gate 后发布 v1.0。

---

## 十四、v1.0 Release Gate

只有全部满足后才应发布 v1.0：

* 不存在对 active 配置的直接下载或原地解码；
* 任意下载、解析或核心验证失败都不改变当前运行实例；
* 每次 apply 都存在可用 last-good；
* 重启或健康检查失败能够自动回滚；
* 并发 update 不会竞争写入；
* 用户现有 crontab 不会被修改或删除；
* user/system backend 的选择确定且持久化；
* user → system 迁移可以 dry-run 和回滚；
* system core 默认不以 root 身份长期运行；
* TUN 失败存在测试过的网络恢复路径；
* controller 默认仅监听回环；
* 非回环 controller 默认强制 secret；
* subscription URL 和认证信息不会出现在普通日志中；
* DNS 和 TUN 均为显式 opt-in；
* 未知 Mihomo 字段在 source 和 overlay 合并后得到保留；
* Mihoro 导入不会静默覆盖原文件；
* upstream 来源在 issue、commit 和 tracking 文档中可追踪。

---

## 十五、Fork 管理与授权

mihoro 使用 MIT License，允许 fork、修改和再分发，但应保留原版权和许可证文本。([GitHub][21])

建议：

```text
origin    -> Pectics/mihoto
upstream  -> spencerwooo/mihoro
```

并执行以下约束：

* 在 fork 点创建不可移动 tag，例如 `mihoro-v0.14.0-base`；
* 保留 Git 历史；
* 保留原 `LICENSE`；
* README 明确写明 “forked from spencerwooo/mihoro”；
* 可增加 `NOTICE.md`，列出设计和代码来源；
* 从上游 PR 使用代码时优先 `git cherry-pick -x`；
* 保留原提交作者；
* 不在同步上游的 branch 上直接开发；
* `main` 开启保护，所有功能通过 issue-linked PR；
* 机械 rename、行为修改和架构重构必须拆开。

---

## 最终判断

这个 fork 值得做，但首要任务不是 TUN。

**mihoto 的第一个版本应当是“安全分叉版”，解决 Cron 数据破坏、配置原地覆盖、不安全 API 默认值和不可回滚更新。** 在此基础上完成配置分层和 service backend，随后再引入 TUN。

因此，三个现有 PR 的最终处理结论是：

* **#197：移植并补强。**
* **#200：不合并，实现层面重写。**
* **#187：拆成 DNS/TUN 两项，保留需求和测试思路，放弃默认值及现有覆盖实现。**

真正决定 mihoto 是否能够长期成立的，不是它是否多支持一个 `tun:` 节点，而是它能否保证：

> **任何一次订阅更新、权限迁移或 TUN 配置变更，都是明确的、可验证的、原子的、可回滚的。**

[1]: https://github.com/lane2077/clash-cli.rs?utm_source=chatgpt.com "lane2077/clash-cli.rs: Rust CLI for mihomo/Clash on Linux ..."
[2]: https://github.com/spencerwooo/mihoro/commits/main/ "https://github.com/spencerwooo/mihoro/commits/main/"
[3]: https://github.com/spencerwooo/mihoro "GitHub - spencerwooo/mihoro: Mihomo CLI client on Linux. Formerly `clashrup`. · GitHub"
[4]: https://github.com/spencerwooo/mihoro/issues/172 "https://github.com/spencerwooo/mihoro/issues/172"
[5]: https://github.com/spencerwooo/mihoro/blob/main/src/mihoro.rs "mihoro/src/mihoro.rs at main · spencerwooo/mihoro · GitHub"
[6]: https://github.com/spencerwooo/mihoro/blob/main/src/config.rs "mihoro/src/config.rs at main · spencerwooo/mihoro · GitHub"
[7]: https://github.com/spencerwooo/mihoro/blob/main/src/cron.rs "mihoro/src/cron.rs at main · spencerwooo/mihoro · GitHub"
[8]: https://github.com/spencerwooo/mihoro/pull/197 "fix(cron): preserve existing crontab entries on enable/disable by dongnengyu · Pull Request #197 · spencerwooo/mihoro · GitHub"
[9]: https://github.com/spencerwooo/mihoro/pull/200 "feat: add system-level service option and update related logic by Aceak · Pull Request #200 · spencerwooo/mihoro · GitHub"
[10]: https://github.com/spencerwooo/mihoro/pull/187/files "feat: add DNS override settings (listen, fake-ip-range) by zhkong · Pull Request #187 · spencerwooo/mihoro · GitHub"
[11]: https://github.com/spencerwooo/mihoro/issues/198 "https://github.com/spencerwooo/mihoro/issues/198"
[12]: https://github.com/spencerwooo/mihoro/issues/190 "https://github.com/spencerwooo/mihoro/issues/190"
[13]: https://github.com/spencerwooo/mihoro/issues/180 "https://github.com/spencerwooo/mihoro/issues/180"
[14]: https://www.freedesktop.org/software/systemd/man/latest/loginctl.html "https://www.freedesktop.org/software/systemd/man/latest/loginctl.html"
[15]: https://wiki.metacubex.one/en/config/inbound/tun/ "https://wiki.metacubex.one/en/config/inbound/tun/"
[16]: https://github.com/MetaCubeX/mihomo/issues/1054 "https://github.com/MetaCubeX/mihomo/issues/1054"
[17]: https://docs.kernel.org/networking/tuntap.html "https://docs.kernel.org/networking/tuntap.html"
[18]: https://www.freedesktop.org/software/systemd/man/systemd.resource-control.html?utm_source=chatgpt.com "systemd.resource-control"
[19]: https://github.com/spencerwooo/mihoro/blob/main/.github/workflows/release.yml "https://github.com/spencerwooo/mihoro/blob/main/.github/workflows/release.yml"
[20]: https://www.freedesktop.org/software/systemd/man/systemd.timer.html "https://www.freedesktop.org/software/systemd/man/systemd.timer.html"
[21]: https://github.com/spencerwooo/mihoro/blob/main/LICENSE "https://github.com/spencerwooo/mihoro/blob/main/LICENSE"
