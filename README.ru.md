# Yatagarasu 八咫烏 — planning-слой Meisei

> **Meisei** 明晰 («ясность») — открытый конвейер, который проводит сырой замысел
> через понимание → решение → план → действие к готовому результату.

[![Meisei](https://img.shields.io/badge/meisei-明晰-1f2937.svg)](https://meisei.ru)
[![License: Apache-2.0 WITH Commons-Clause](https://img.shields.io/badge/license-Apache--2.0%20WITH%20Commons--Clause-blue.svg)](LICENSE)

<sub>
torii · satori · enma · <b>yatagarasu</b> · fujin · daruma
&nbsp;—&nbsp; intake · осмысление · решения · <b>планирование</b> · действия · исполнение (терминальный слой)
</sub>

## Что это

Yatagarasu — **planning**-слой конвейера MeiSei: превращает решения в планы.
Владеет AI-операциями планирования — `plan_ai` (контекст решения →
`PlanBrief`), `decompose_task` (задача → минимум два черновика подзадач),
`scope_task` (расширить/сузить задачу переписыванием заголовка и описания) —
плюс детерминированная проверка готовности плана (`check_readiness`). Доменные
примитивы не зависят от хранилища; брифы планов персистит сервер. Крейт не
зависит от daruma и соседних слоёв; адаптеры живут только внутри host.

Пакетная оценка сложности задач реализована AI-маршрутами сервера Daruma и
проекцией `task_complexity_hints`. Дублирующий оценщик и его промпт отсюда
удалены. `decompose` и `scope` остаются самостоятельными, доступными
операциями планирования; ни одна из них не изменяет задачи Daruma напрямую.

## Структура репозитория

- `src/` — библиотека `yatagarasu`: `PlanBrief`, операции
  decompose/scope, черновики задач, движок промптов, типы ошибок.
- `server/` — `yatagarasu-server`, тонкая независимо развёртываемая HTTP/MCP
  обёртка над библиотекой (axum/tokio-каркас — из
  [`layer-kit`](../layer-kit)).
- `deploy/` — release-`build.sh` (прошивает git SHA в `/healthz`) и systemd user unit.

## Сборка и запуск

```sh
cargo run -p yatagarasu-server
# GET  /healthz   — открытая проба живости/версии
# POST /v1/mcp    — MCP-поверхность под платформенным токеном:
#                   yatagarasu.plan, yatagarasu.decompose,
#                   yatagarasu.scope,
#                   yatagarasu.read
```

Для продовых сборок используйте `deploy/build.sh`, чтобы `/healthz` отдавал
реальный git SHA, а не `"dev"`.

## Конфигурация (env)

| Переменная | По умолчанию | Назначение |
| --- | --- | --- |
| `YATAGARASU_PORT` | `8093` | HTTP-порт |
| `YATAGARASU_PLATFORM_SECRET` | не задан | HMAC-ключ; если не задан, `/v1/mcp` закрыт |
| `YATAGARASU_VERSION` | версия крейта | Версия, отдаваемая `/healthz` |
| `YATAGARASU_DB` | `./yatagarasu.db` | Путь к SQLite-хранилищу (`layer_kit::store::Store`) |
| `OPENAI_API_KEY` | не задан | Опциональный AI-провайдер для `yatagarasu.plan` / `decompose` / `scope`; без ключа — ошибка `ai_not_configured` (см. [Ошибки через MCP](#ошибки-через-mcp)) |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | Базовый URL OpenAI-совместимого API |
| `OPENAI_MODEL` | `gpt-4.1` | Модель, используемая AI-операциями |

## Ошибки через MCP

`POST /v1/mcp` принимает только MCP (JSON-RPC 2.0). Упавший инструмент — это
результат инструмента, а не HTTP-ошибка: `tools/call` отвечает HTTP 200 с
`result.isError: true`, объектом ошибки в `result.structuredContent` (поле
`error` — машиночитаемый код) и тем же JSON текстом в
`result.content[0].text`. Внутренний статус метода — например, 503 у
`ai_not_configured` — на провод не попадает. Вызов `yatagarasu_plan` без
AI-провайдера возвращает:

```json
{"jsonrpc": "2.0", "id": 1, "result": {
  "content": [{"type": "text", "text": "{\"detail\":\"…\",\"error\":\"ai_not_configured\"}"}],
  "structuredContent": {"detail": "OPENAI_API_KEY not set; yatagarasu-server has no AI provider", "error": "ai_not_configured"},
  "isError": true}}
```

Сбои протокола — объекты JSON-RPC `error`, тоже с HTTP 200 (`-32601` —
неизвестный метод, `-32602` — `tools/call` без `name`). HTTP-коды остаются за
транспортом: 401 — нет или неверный платформенный токен либо поверхность
закрыта (`auth_disabled`), 400 — тело не является запросом JSON-RPC 2.0,
202 с пустым телом — notification. Конверт реализован один раз в
`layer-kit` (`src/serve.rs`).

## Документация

Канон конвейера и контракты слоёв: https://meisei.ru/docs

## Лицензия

Apache-2.0 WITH Commons-Clause — см. [LICENSE](LICENSE) и
[LICENSE.commons-clause.md](LICENSE.commons-clause.md).
