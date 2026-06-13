<div align="center">

# FreeCowork

### Your AI coworker, one hotkey away.

**Press `Alt+H` · say what you need · it gets done — free, no API key, zero config.**

[![Release](https://img.shields.io/github/v/release/jaylooloomi/FreeCowork?label=download&style=flat-square)](https://github.com/jaylooloomi/FreeCowork/releases/latest)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6?style=flat-square)](#system-requirements)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%20v2-24C8DB?style=flat-square)](https://tauri.app)
[![License: MIT](https://img.shields.io/badge/license-MIT-green?style=flat-square)](#license)

</div>

---

> FreeCowork 是一個 Windows 系統匣常駐小工具。按 `Alt+H` 叫出輸入框,用講的或打字說出需求,AI 就**直接在你電腦上動手做**——整理檔案、改名、查資料、開程式都行。可用**免費**的 Ollama 雲端開源模型(只需到 ollama.com 免費登入一次,免 API key),或接你自己的 Anthropic Claude 帳號。設計理念:**越無腦越好**,讓完全不懂技術的人也能享受 AI 幫忙幹活。

---

## The problem

AI can do real work now — but for most people, the power is locked behind a wall:

- **The capable tools demand technical setup.** Command-line AI agents and APIs mean terminals, API keys, billing dashboards, and config files. Most people quit before the first prompt.
- **Chat assistants can talk, but they can't *act*.** ChatGPT or Claude.ai will *tell* you how to rename 200 files — but you still do it by hand. The assistant lives in a browser tab, not on your desktop.
- **Cost and keys are a hard gate.** "Enter your credit card / paste your API key" stops non-technical users cold.
- **Constant context-switching.** Leave your work → open a tool → describe the problem → copy the answer back → do it yourself.

The result: the people who would benefit most from an AI assistant are the ones least able to set one up.

## The solution

**FreeCowork collapses all of that into a single hotkey.**

```
Alt+H  →  "organize my desktop into folders by file type"  →  Enter
                         ↓
        the AI actually does it, on your machine, live.
```

No terminal. No API key. No reading docs. The app auto-installs everything it needs on first run, connects to **free** open-source models (or your own Claude account), and **executes** your request — then shows you what it did, in real time. It's the difference between an assistant that *answers* and a coworker that *gets it done*.

---

## Key features

- 🎯 **One global hotkey, plain language.** `Alt+H` from anywhere → a PowerToys-style box. Type or speak what you want; press Enter.
- 🆓 **Free by default — no API key.** Runs on Ollama's cloud open-source models; the only setup is a one-time free login at ollama.com. Or flip a switch to use **your own Anthropic Claude account** for top-tier quality.
- 🤖 **It does the work, not just describe it.** Files get organized, renamed, searched, apps get opened — the AI operates your machine directly (safely; see [Security](#privacy--security)).
- 📡 **Live results, streamed in.** Watch the AI think and act in real time, right in the palette — tool calls and the final answer, as they happen.
- 🎙️ **Voice input** (`Alt+J`) — speak instead of type. 📸 **Screenshot to context** (`Alt+K`) — snip a region of your screen and it's instantly attached, so you can ask "what's this error?".
- ✅ **A task queue that remembers.** Fire several requests; they run in order. Finished tasks become a tidy checklist you tick off — expand any one to re-read what it did.
- 🪟 **Native, lightweight, beautiful.** Acrylic-glass UI, ~3 MB installer, no Electron. Traditional Chinese + English.
- 🔒 **Private by design.** Zero telemetry. Everything runs locally; nothing about your usage is collected.

---

## Why FreeCowork

|  | Command-line AI agents | Chat assistants (web) | **FreeCowork** |
|---|---|---|---|
| **Setup** | API key + terminal + config | Account, often a subscription | **One free login, zero config** |
| **How you ask** | Typed commands | Type, in a browser tab | **Hotkey + natural language / voice** |
| **Acts on your PC?** | Yes, but manual & technical | No — it can only talk | **Yes, automatically** |
| **Cost to start** | Pay-per-token API | Monthly subscription | **Free tier** (or bring your own account) |
| **For non-technical users** | ❌ Out of reach | ⚠️ Talks, can't do | ✅ **Effortless** |
| **Footprint** | — | Browser | **~3 MB native tray app** |

**The wedge:** FreeCowork is the only one of these a non-technical person can install and use in two minutes — *and* the only one that actually carries out the task on their computer.

---

## How it works

```
┌──────────────────────────────────────────────────────────────┐
│  System tray (always on)                                       │
│        │  Alt+H                                                │
│        ▼                                                       │
│  ┌─────────────────────────────┐   natural-language request    │
│  │  Palette (Svelte 5 + glass) │ ──────────────┐               │
│  └─────────────────────────────┘               │               │
│        ▲ live stream-json (results, tool calls) │               │
│        │                                        ▼               │
│  ┌──────────────────────────────────────────────────────────┐ │
│  │  Rust core (Tauri v2): task queue · process mgmt · IPC    │ │
│  └──────────────────────────────────────────────────────────┘ │
│        │ spawns                                                 │
│        ▼                                                        │
│   Claude Code  ──►  ┌─ free Ollama cloud open-source models     │
│                     └─ or your own Anthropic Claude account     │
└──────────────────────────────────────────────────────────────┘
```

On first run the app silently installs **Ollama** and **Claude Code** (no admin rights), guides you through a one-time browser login at ollama.com, then runs every request through Claude Code — pointed either at Ollama's free cloud models or, if you choose, your own Claude account. Output comes back as a structured `stream-json` event flow that the palette renders live.

---

## Install

> **First-time note:** the installer isn't code-signed yet, so Windows SmartScreen may warn you → click **More info → Run anyway**.

**1. Download the installer (easiest)**
Grab the latest `*-setup.exe` from the [Releases page](https://github.com/jaylooloomi/FreeCowork/releases/latest) and double-click (no admin needed).

**2. One-line install (PowerShell)**
```powershell
irm https://raw.githubusercontent.com/jaylooloomi/FreeCowork/main/install.ps1 | iex
```

**3. winget** *(planned)*
```powershell
winget install jaylooloomi.FreeCowork
```

---

## Usage

1. After install, FreeCowork lives in your system tray and starts with Windows.
2. Press **`Alt+H`** to summon the input box.
3. Type (or speak) a request, e.g.:
   - `organize my desktop into folders by type`
   - `rename the PDFs in Downloads to start with their date`
   - `summarize the latest 5 tech news and save it to a note`
4. Press **Enter**. Watch it work in the result panel.

| Shortcut | Action |
|---|---|
| `Alt+H` (global) | Open / close the palette |
| `Alt+J` (palette open) | Voice input |
| `Alt+K` (palette open) | Region screenshot → attach |
| `Enter` | Submit |
| `Esc` | Close dropdown / palette |
| `↑` / `↓` | Browse history |

**First run** launches a setup wizard (no admin rights): it installs Ollama and Claude Code and walks you through the ollama.com browser login, then runs your request automatically.

---

## Settings

Tray icon → **Settings** (auto-closes after save).

| Option | Description |
|---|---|
| Language | Traditional Chinese (default) / English |
| Hotkey | Default `Alt+H`; any combo (e.g. `Ctrl+Alt+Space`) |
| Voice / Screenshot hotkeys | Default `Alt+J` / `Alt+K`; both configurable |
| Model | Pick from the live Ollama cloud catalog, or your Claude account |
| Cautious mode | AI asks before risky actions, instead of full autonomy |
| Background mode | Stream results into the palette instead of a terminal (default) |
| Assistant personality | Advanced: customize the system prompt |
| Working directory | Default folder for tasks (blank = home) |
| Launch at startup | Run on login (default on) |

---

## Privacy & security

- **Zero telemetry.** No usage data is collected or transmitted.
- **Safe command execution.** Every subprocess is spawned with an argument array — never shell-string interpolation — so there is no command-injection surface.
- **Autonomy is a choice.** By default the AI acts without per-step confirmation (`--dangerously-skip-permissions`) so it's truly effortless; turn on **Cautious mode** to gate risky operations.

---

## Engineering highlights

Built to be small, fast, and correct — not a webview wrapped in a browser:

- **Tauri v2** — Rust core + **Svelte 5 (runes)** frontend, WebView2. **~3 MB** installer, no Electron.
- **Concurrency-safe task queue** — single-source-of-truth state machine with poison-tolerant locking, so a panic can never deadlock the queue.
- **Live `stream-json` pipeline** — child stdout is parsed line-by-line and streamed to the UI; failures are classified (subscription / quota / auth) from the log tail.
- **Robust Windows integration** — global hotkeys, acrylic vibrancy, AppUserModelID-based toast icons, NSIS per-user installer, autostart.
- **Tested & reviewed** — 114 Rust unit tests; type-checked frontend; changes hardened via adversarial review.

---

## Roadmap

- [ ] Code signing (remove the SmartScreen warning)
- [ ] winget listing
- [ ] Auto-update (Tauri updater)
- [ ] macOS / Linux

## Known limitations

- **Free-tier quota.** Ollama's free cloud has a GPU-time quota; heavy use may need to wait for a reset (per-account, switching models doesn't help).
- **Open-source model quality.** The free path uses Ollama cloud open-source models, not Anthropic's commercial Claude — capability and consistency differ. Use your own Claude account for top quality.
- **Windows only** (10 22H2+ / 11) for now.
- **No auto-update yet** — re-download to upgrade.
- The **first** task in a new working directory triggers Claude Code's own folder-trust prompt once (press Enter; it's Claude Code's safety mechanism, not bypassed).

---

## Development

```
launcher/
├── src/            ← frontend (Svelte 5 + TypeScript)
│   └── lib/        ← UI components & API wrappers
└── src-tauri/      ← Rust backend
    └── src/        ← feature modules
```

```powershell
cd launcher && npm install      # install frontend deps
npm run tauri dev               # dev mode (hot reload)
cd src-tauri && cargo test      # Rust unit tests
npm run tauri build             # production installer → src-tauri/target/release/bundle/nsis/
```

## System requirements

- Windows 10 22H2+ or Windows 11
- Internet connection (first-time install and cloud model calls)

---

## Disclaimer

FreeCowork is an independent, open-source project and is **not affiliated with, endorsed by, or sponsored by Anthropic or Ollama**. "Claude" and "Claude Code" are trademarks of Anthropic; "Ollama" is a trademark of its respective owner. This tool orchestrates those products under your own accounts/usage.

## License

MIT
