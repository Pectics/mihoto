github.com/spencerwooo/mihoro
这是一个运行于 linux、用户级实例的 mihomo 内核管理工具

作者最后维护时间是两个月前，但已经积攒了大量的 issue 和部分核心 feature 的 PR 没有处理，
其中包括：[feat: add system-level service option and update related logic
](github.com/spencerwooo/mihoro/pull/200) 这个系统服务级别选项的功能追加。

我在尝试了一段时间使用裸的 mihomo 内核管理代理之后，意识到其在订阅配置管理方面的能力缺陷，但此类订阅管理功能的确不适合作为 mihomo 的 feature 进行追加，于是我在搜索后找到了 mihoro 这个项目。

在我实际使用过程中，发现 mihoro init 只会在 ~/.local/bin、~/.config/mihoro.toml、~/.config/mihomo/config.yaml 等用户级别的地方安装 mihoro 管理工具和 mihomo 内核服务，同时其注册的 mihomo.service 也是用户级别的。

我过去常常使用的是 TUN 虚拟网卡模式的代理，在尝试使用 mihoro 管理以前的系统级别的 mihomo 内核时发现 mihoro.toml 里面的配置并未对 `tun:` 进行支持。

所以我打算基于 mihoro fork 开发出一个 mihoto 项目（此处的 `t` 可理解为 `tun`），但在此之前我需要你为我做一次完整的深入研究，以便决定我的开发方向和具体路线。

目前还有一些其他线索如下：
- mihoro 的 PR 部分除了上述 system-level 的 feature 更新以外，还存在两个有关 cron enable/disable 和 DNS override settings 的 PR，同时 issue 区也存在一些大大小小的问题或者功能建议。我认为在我的 mihoto v1.0 需要将这部分已存在的问题中优先级较高的部分给完成。
- 后续 mihoto 的重构和开发推进我打算直接在我 fork 的 repo 中添加 issue 并做优先级评级，与 mihoro repo 中已有 issue 或者 PR 相关联的内容应当对来源进行引用，最后按 issue 中的开发优先级来逐步推进 v1.0 版本。