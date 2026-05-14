# Claude Code integration for zenv

Turn Claude Code (and any other MCP-compatible AI coding agent) into a
zenv expert in any of your projects. This integration ships as a single
markdown skill file -- no daemon, no hosted service, no telemetry, no
account.

## What it does

When installed, your AI agent will automatically reach for `zenv` whenever
it sees a `.env` file or env-var-related task in the workspace. It knows:

- which subcommand fits each situation (`check`, `scan`, `init`, `doctor`,
  `fix`, `diff`, `export`, `template`, ...)
- which flags to use for agent-friendly output (`--format json`, exit
  codes)
- which anti-patterns to avoid (e.g. never run `fix` without `--dry-run`
  first; never commit raw `.env` files)
- how to handle schema authoring, remote schemas, and security flags
  (`--verify-hash`, `--ca-cert`)

## Install

The skill is a single markdown file. Drop it into your Claude Code skills
directory:

```bash
# Linux / macOS
mkdir -p ~/.claude/skills/zenv
curl -fsSL https://raw.githubusercontent.com/zorl-engine/zorath-env/main/integrations/claude-code/SKILL.md \
  -o ~/.claude/skills/zenv/SKILL.md

# Windows (PowerShell)
New-Item -ItemType Directory -Force -Path "$env:USERPROFILE\.claude\skills\zenv" | Out-Null
Invoke-WebRequest -Uri "https://raw.githubusercontent.com/zorl-engine/zorath-env/main/integrations/claude-code/SKILL.md" `
  -OutFile "$env:USERPROFILE\.claude\skills\zenv\SKILL.md"
```

Restart Claude Code (or start a new session) -- the skill is auto-loaded
on session start and the agent will use it whenever a relevant trigger
appears.

## Prerequisite -- install zenv itself

The skill assumes the `zenv` binary is on PATH. Install via one of:

```bash
# Rust toolchain
cargo install zorath-env

# Prebuilt binaries (Linux / macOS Intel + ARM / Windows)
# Pick the right asset from:
#   https://github.com/zorl-engine/zorath-env/releases
```

## Why a skill instead of a hosted MCP server

zenv is open source and runs entirely on your machine. There is no
remote service to host, no API key to manage, and no cost to operate.
The skill approach gives the agent everything it needs to drive the
local `zenv` binary directly, without any network round-trip or vendor
dependency.

If you use multiple AI coding tools (Claude Code + Cursor + Cline +
Windsurf), a future v0.4 release will add a `zenv mcp` stdio subcommand
so any MCP-compatible client can drive zenv with the same UX. Stdio MCP
is also local-only -- still no hosting, still no cost.

## Verification

After install, in a new Claude Code session, try one of these prompts in
a project that has a `.env`:

- "Audit my .env for leaked secrets."
- "Generate a schema from my .env.example."
- "What env vars does my code use that aren't in the schema?"
- "Is it safe to commit this .env change?"

The agent should reach for zenv subcommands automatically. If it doesn't,
check that `~/.claude/skills/zenv/SKILL.md` exists and that you have
restarted Claude Code since installing.

## Contributing

The skill ships under the same MIT license as zenv. Improvements,
additional anti-patterns, and new recipes are welcome via PR against
`integrations/claude-code/SKILL.md` in the
[zorl-engine/zorath-env](https://github.com/zorl-engine/zorath-env)
repository.

## Privacy + security notes

- The skill is plain markdown. It contains no executable code and makes
  no network calls itself.
- `zenv` runs locally and does not phone home. It does NOT report
  telemetry, validation results, or schema contents anywhere.
- `--format json` output that the agent parses stays in the local
  agent <-> LLM channel. Treat that channel according to your LLM
  vendor's data handling policy.
- For maximum isolation in CI, the same binary can run without network
  access at all -- pass a local schema path and skip the remote-fetch
  pipeline entirely.
