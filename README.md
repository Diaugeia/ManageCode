# MinionsCode

A native macOS terminal app focused on managing Claude Code sessions. Real shell + Claude Code superpowers in one window.

## Features

- **Native terminal** — SwiftTerm-backed, real `$SHELL` by default
- **Claude Code session tracking** — every active `claude` process is auto-detected from `~/.claude/sessions/` and `~/.claude/projects/`
- **Token + cost dashboard** — input / cache read / cache write / output broken out separately, with the "saved by cache" delta
- **AI-aware search** — literal filter first, falls back to a one-shot Haiku query when the literal filter misses
- **Session grouping by directory** — sessions grouped by `cwd`, expandable
- **Custom names** — override the auto-derived name (Claude's `ai-title` from JSONL)
- **Quick actions** — new shell, new claude, resume existing session, run claude inside an open shell
- **NotchAgent black/gold theme**

## Build

```bash
swift build -c release
./install.sh        # installs to ~/Applications/MinionsCode.app
open ~/Applications/MinionsCode.app
```

Requires macOS 14+ and an Apple Silicon Mac.

## Stack

- Swift 6.3, SwiftUI + AppKit
- [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) for terminal rendering
- Reads Claude Code's local files directly — no API calls, no daemon

## Pricing

Costs are computed using public Anthropic pricing for Claude 4.x:

| Model | Input | Cache read | Cache write | Output |
|-------|-------|------------|-------------|--------|
| Opus 4.7 | $15/MTok | $1.5/MTok | $18.75/MTok | $75/MTok |
| Sonnet 4.6 | $3/MTok | $0.3/MTok | $3.75/MTok | $15/MTok |
| Haiku 4.5 | $0.8/MTok | $0.08/MTok | $1/MTok | $4/MTok |
