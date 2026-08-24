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
`scope_task` (расширить/сузить задачу переписыванием заголовка и описания) и
`analyze_complexity_batch` (пакетная оценка сложности задач плана для fan-out
декомпозиции) — плюс детерминированная проверка готовности плана
(`check_readiness`). Доменные примитивы не зависят от хранилища; брифы планов
персистит сервер. Крейт не зависит от daruma и соседних слоёв; адаптеры живут
только внутри host.

## Структура репозитория

- `src/` — библиотека `yatagarasu`: `PlanBrief`, операции
  decompose/scope/complexity, черновики задач, движок промптов, типы ошибок.
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
#                   yatagarasu.scope, yatagarasu.analyze_complexity,
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
| `OPENAI_API_KEY` | не задан | Опциональный AI-провайдер для `yatagarasu.plan` / `decompose` / `scope` / `analyze_complexity`; без ключа — ответ `ai_not_configured` (503) |
| `OPENAI_BASE_URL` | `https://api.openai.com/v1` | Базовый URL OpenAI-совместимого API |
| `OPENAI_MODEL` | `gpt-4.1` | Модель, используемая AI-операциями |

## Документация

Канон конвейера и контракты слоёв: https://meisei.ru/docs

## Лицензия

Apache-2.0 WITH Commons-Clause — см. [LICENSE](LICENSE) и
[LICENSE.commons-clause.md](LICENSE.commons-clause.md).
