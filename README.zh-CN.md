# ManageCode

> **统一管理你的 AI 编程 Agent 的面板。** 一块面板集中管理本机所有 Claude Code 与
> Codex 会话——一屏掌握全局运行状态，并在不同工具、不同目录之间迁移记录与 Memory。

[English](README.md) · **简体中文**

```
┌─ ManageCode ── 49 sessions · 1 active · ▶ 2 tmux · ▷ 3 codex · $2916.45 ──┐
│ ▾ ~/Project/05_2026/MinionsCode                                ▶1  ●1   3  │
│    ▶ tmux busy   rust-tui notify integration   sonnet-4.6  $  2.41   2m   │
│    ● idle        refresh strategy notes         opus       $  0.47  14h   │
│ ▾ ~/Project/03_2026/Forecasting_Reasoning                              5  │
│    ○ ended       backtest harness               gpt-5.5    $  3.10   3d   │
│    ○             清理 Zone 的无用 file           opus       $  0.12  17d   │
└────────────────────────────────────────────────────────────────────────────┘
```

## 为什么需要 ManageCode

Vibe Coding 时代由 Agent 驱动——Claude Code、Codex 等等。人们常常**同时**开着几十个
Agent 进程，而目前唯一的「管理方式」只是一墙的 `tmux` pane，毫无全局视图。

ManageCode 正是缺失的那一层：

1. **一个平台管所有 Agent。** Claude Code 与 Codex 会话同列一表——运行状态、花费、模型、
   工作目录一目了然，告别零散、互不关联的工具。
2. **全局态势感知。** 在一块面板上掌握所有 Agent 的运行状态：谁在忙、谁空闲在等你、各自花了
   多少钱。按 `Enter`，会话就在面板内打开。
3. **随你迁移的记录与 Memory。** Agent 通常绑死在某个目录——目录改名或搬迁，Memory
   （`CLAUDE.md` / `AGENTS.md`）就丢了。ManageCode 支持 Memory 跨目录、跨工具迁移，并能在
   Claude 与 Codex 之间互转会话记录。

**Rust** 编写、**TOML** 配置，原生终端性能、现代化 TUI——单个静态二进制，无运行时依赖。

## 安装

```bash
curl -fsSL https://raw.githubusercontent.com/Diaugeia/ManageCode/main/install.sh | bash
```

脚本会下载对应平台的预编译二进制并安装到 `~/.local/bin/managecode`，无需 Rust 工具链。

**已支持：** Linux x86_64、macOS Apple Silicon（M1 及更新）。
**Intel Mac / Windows：** 请[从源码构建](#从源码构建)（Windows 需 WSL）。

之后用 `managecode --update`（或重跑安装命令）即可更新。ManageCode 启动时也会检查新版本并在
顶栏显示 `⬆` 提示；可用 `--no-update-check` 或配置 `update_check = false` 关闭。

卸载请运行：

```bash
curl -fsSL https://raw.githubusercontent.com/Diaugeia/ManageCode/main/uninstall.sh | bash
```

该脚本会从 `~/.local/bin` 删除二进制。加 `--purge` 可一并删除配置目录（`~/.managecode`）；
`~/.claude` 与 `~/.codex` 不会被触碰。

## 首次运行

```bash
managecode
```

本机所有会话都会列出——活跃的在最上，最近的在下，再往下是最近 30 天的其余会话。选中一个按
`Enter`，它就**在面板内打开**：列表收窄成侧边栏，右侧一个实时终端运行该 Agent。`Ctrl-a`
切回侧边栏（会话继续运行），换一个、稍后再回来——全程一个窗口。

任何时候按 `?` 查看完整、始终与实际绑定同步的快捷键表。

## 亮点

- **多 Agent 同列。** Claude Code（`~/.claude`）与 OpenAI Codex（`~/.codex`）并列展示，各自按
  正确价格计费。
- **持续化多会话。** 配合 `tmux`，启动跑在后台 detached 会话里，可整天来回切换——详见
  [tmux-and-pty.md](docs/tmux-and-pty.md)。
- **分组 / 树 / 平铺。** `T` 在「按 `cwd` 分组」「路径压缩的**目录树**」「平铺列表」之间循环。
- **花费一目了然。** 每会话 token 用量与美元花费、当日合计、预算提醒（`:`）、`c` 按目录/模型/
  天的花费汇总。
- **互操作。** `x` 把记录转换成另一工具的格式；`X` 把某目录的 Memory（`CLAUDE.md` /
  `AGENTS.md`）迁移到任意其他目录。
- **AI 搜索与自动命名。** `/` 过滤；无匹配时 `Enter` 触发模型搜索；`A` 给未命名会话起名。模型
  可配置。
- **鼠标友好、终端原生配色**、忙→闲时桌面通知，以及完全**可重映射的快捷键**。

## 文档

- [快捷键](docs/keybindings.md) —— 全部按键与如何重映射
- [配置](docs/config.md) —— `~/.managecode/config.toml` 参考
- [tmux vs. PTY](docs/tmux-and-pty.md) —— 启动如何运行与持久化

## 从源码构建

```bash
git clone https://github.com/Diaugeia/ManageCode.git
cd ManageCode
./build.sh        # 或：cargo build --release
```

需要较新的 stable Rust 工具链。产物在 `target/release/managecode`。

## 许可证

MIT —— 见 [LICENSE](LICENSE)。
