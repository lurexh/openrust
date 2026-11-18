# openrust

[![CI](https://github.com/lurexh/openrust/actions/workflows/ci.yml/badge.svg)](https://github.com/lurexh/openrust/actions/workflows/ci.yml)

A from-scratch Rust rewrite of [opencode](https://github.com/anomalyco/opencode) —
a streaming coding agent that runs entirely on-device in your terminal.

## Features

- **Streaming agent loop** — tokens print live; the model can call tools mid-reply
- **Tools**: `bash`, `read`, `write`, `edit`, `glob`
- **Subagents** — `task` tool spawns a focused child agent (depth 1) for bigger work
- **Permissions** — interactive y/n/a gating for dangerous tools (`read`/`glob` are safe)
- **Plan mode** — `/plan` restricts the agent to read-only tools and asks it to
  propose a plan before touching anything
- **MCP client** (stdio, no SDK) — plug in any MCP server via config; its tools
  join the agent's toolset (`mcp_<server>_<tool>`)
- **Built-in knowledge-graph memory** — native Rust port of the MCP memory
  server (entities, relations, observations); persists to
  `~/.local/share/openrust/memory.json`. No node required.
- **Anthropic provider** — native Claude support via `ANTHROPIC_API_KEY`;
  any OpenAI-compatible endpoint otherwise
- **Anonymous mode** — no API key needed; uses opencode's free-tier gateway
  with 12 free models (`/models` to browse, `/model <id>` to switch)
- **Persistent sessions** — history auto-saves per project directory and resumes
  on next launch (`/reset` to clear)

## Benchmark

Measured on-device (aarch64, Termux/Android), both binaries idle waiting for input:

| Metric | openrust | upstream opencode (Bun) |
|---|---|---|
| Binary size | **5 MB** | 195 MB |
| RAM (idle) | **3.2 MB** | ~309 MB |
| CPU to boot + idle 7 s | **0.01 s** | ~8.5 s |
| Cold start to ready | **58 ms** | seconds + glibc loader wrapper |
| Runtime dependencies | **none** | libstdc++, libgcc_s, glibc |

No JS runtime, no JIT warm-up, no GC — the process uses ~0% CPU while idle and
stays out of the way of everything else on the device.

## Install

Linux (aarch64 + x86_64, including Android/Termux — static musl build, zero deps):

```sh
curl -fsSL https://raw.githubusercontent.com/lurexh/openrust/main/install.sh | sh
```

Windows (x86_64, PowerShell):

```powershell
irm https://raw.githubusercontent.com/lurexh/openrust/main/install.ps1 | iex
```

Or grab a binary from [Releases](https://github.com/lurexh/openrust/releases/latest),
or build from source below.

## Build

```sh
cargo build --release
target/release/oc-rs
```

## Config (optional) — `~/.config/openrust/config.json`

```json
{
  "model": "glm-5.3-flash-free",
  "mcp_servers": [
    { "name": "my-server", "command": ["my-mcp-server", "--flag"] }
  ]
}
```

Environment: `OPENCODE_API_KEY`, `OPENCODE_BASE_URL`, `OPENCODE_MODEL`, `ANTHROPIC_API_KEY`.

## Roadmap

- [x] provider + streaming + function calling
- [x] agent loop + builtin tools
- [x] anonymous mode
- [x] MCP stdio client
- [x] session persistence
- [x] permissions system
- [x] TUI homepage + streaming chat session
- [x] subagents (`task`)
- [x] plan mode
- [x] Anthropic provider
- [x] built-in knowledge-graph memory (no node)
- [ ] LSP integration
- [ ] plugins / custom agents config

## License

MIT
