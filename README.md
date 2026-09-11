---
title: "Yatagarasu planning server"
audience: "developer"
intent: "reference"
owner: "MeiSei"
status: "active"
source_of_truth: "README.md"
last_verified: "2026-09-11"
---

# Yatagarasu 八咫烏 — planning layer of Meisei

> **Meisei** 明晰 (“clarity”) is an open pipeline that carries raw intent through
> understanding → decision → plan → action to a finished result.

[![Meisei](https://img.shields.io/badge/meisei-明晰-1f2937.svg)](https://meisei.ru)
[![License: Apache-2.0 WITH Commons-Clause](https://img.shields.io/badge/license-Apache--2.0%20WITH%20Commons--Clause-blue.svg)](https://github.com/tupical/yatagarasu/blob/main/LICENSE)

<sub>
torii · satori · enma · <b>yatagarasu</b> · fujin · daruma
&nbsp;—&nbsp; intake · sensemaking · decisions · <b>planning</b> · actions · execution (terminal)
</sub>

## What it is

Yatagarasu is the **planning** layer of the Meisei pipeline: it turns decisions
into plans. It owns the AI planning operations — `plan_ai` (decision context →
`PlanBrief`), `decompose_task` (a task → at least two sub-task drafts),
`scope_task` (broaden/narrow a task by rewriting its title and description) — plus the deterministic plan readiness check (`check_readiness`).
Domain primitives stay storage-agnostic; the server persists plan briefs. The
crate has no dependency on daruma or sibling layers; adapters live only inside
the host.

Batch task complexity analysis is implemented by Daruma server AI routes and
the `task_complexity_hints` projection. The unused duplicate scorer and prompt
were removed here. `decompose` and `scope` remain standalone, reachable planning
operations; neither directly mutates Daruma tasks.

## Repository layout

- `src/` — the `yatagarasu` library: `PlanBrief`, decompose/scope
  operations, task drafts, prompt engine, error types.
- `server/` — `yatagarasu-server`, a thin, independently-deployed HTTP/MCP
  wrapper over the library (the axum/tokio scaffold comes from
  `layer-kit`).
- `deploy/` — release `build.sh` (stamps the git SHA into `/healthz`) and a
  systemd user unit.

## Build & run

```sh
cargo run -p yatagarasu-server
# GET  /healthz   — open liveness/version probe
# POST /v1/mcp    — platform-token gated MCP surface:
#                   yatagarasu.plan, yatagarasu.decompose,
#                   yatagarasu.scope,
#                   yatagarasu.read
```

For production builds use `deploy/build.sh` so `/healthz` reports the real git SHA
instead of `"dev"`.

## Configuration (env)

| Variable | Default | Purpose |
| --- | --- | --- |
| `YATAGARASU_PORT` | `8093` | HTTP listen port |
| `YATAGARASU_PLATFORM_SECRET` | unset | HMAC key; if unset, `/v1/mcp` is closed |
| `YATAGARASU_VERSION` | crate version | Version reported by `/healthz` |
| `YATAGARASU_DB` | `./yatagarasu.db` | SQLite store path (`layer_kit::store::Store`) |
| `OPENAI_API_KEY` | unset | Optional AI provider for `yatagarasu.plan` / `decompose` / `scope`; without a key they answer `ai_not_configured` (503) |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | Base URL of the OpenAI-compatible API |
| `OPENAI_MODEL` | `gpt-4.1` | Model used by the AI operations |

## Docs

Pipeline canon and layer contracts: https://meisei.ru/docs

## License

Apache-2.0 WITH Commons-Clause — see [LICENSE](https://github.com/tupical/yatagarasu/blob/main/LICENSE) and
[LICENSE.commons-clause.md](https://github.com/tupical/yatagarasu/blob/main/LICENSE.commons-clause.md).
