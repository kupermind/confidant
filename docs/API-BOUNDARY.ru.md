# Граница: `backend/` ↔ пользовательский слой

Состояние на 2026-10-05, после PR #11. Документ передачи между двумя разработчиками: кто за что
отвечает, с чем именно говорит UI, что он обязан подготовить и как меняется контракт. Если страница
расходится с кодом, прав код — страницу правят в том же PR.

## 1. Кто за что отвечает

| Сторона | Отвечает за | Каталоги |
|---|---|---|
| **Крипто/ZK-лид** | Rust-ядро; **кворумные сервисы**: узел верификатора (`confidant-node`), сайдкары подписантов, координатор, скрипты настройки пула и запуска; CLI-инструмент; оба e2e-харнеса; аудиторский цикл | `core/`, `backend/`, `e2e/testnet/`, `docs/audit/`, `docs/*.md` |
| **API/XRPL-лид** (владелец репозитория) | **пользовательский слой**: UI, UX локального кошелька, депозитный Payment с собственного XRPL-аккаунта пользователя, ссылки на explorer, предзаполнение пула, демо-сценарий и питч | `frontend/`, `scripts/` |

Шов — HTTP. UI является **клиентом координатора** (один процесс, ключей не держит). На стороне UI
нет ключей пула и подписантов, состояния верификатора и логики проверки депозитов; на стороне
бэкенда нет пользовательских ключей трат и никакого рендеринга.

## 2. Контракт, который потребляет UI — координатор

Базовый URL: `http://127.0.0.1:7200` при запуске через `backend/quorum/start-all.mjs` (настраивается).
Всё в JSON. Ошибки — `{ "error": "<сообщение>", "details"?: {…} }` со статусами:

| Статус | Смысл | Типичные сообщения |
|---|---|---|
| 400 | кривой запрос или лимит ядра | нулевая сумма, слишком много выходов, плохой hex, нет полей |
| 409 | отказ верификаторов | nullifier уже потрачен (повтор / двойная трата), несовпадение memo, неизвестный якорь, `only 1/3 signers endorsed (quorum 2)` |
| 422 | леджер не подтверждает утверждение | депозит не validated / не в пул / не та сумма / нет memo `confidant/shield`; анкорная транзакция не несёт этот корень |
| 502 | недоступен RPC леджера или узел | `txnNotFound <hash>` для выдуманного хеша депозита, `fetch failed` |

### Эндпоинты

| Вызов | Тело | Ответ |
|---|---|---|
| `GET /state` | — | `PoolState` |
| `GET /log?from=N` | — | `{ records: BundleRecord[], …PoolState }` — зеркало, с которого синхронизируется кошелёк; `records` сохранить как `log.json` |
| `POST /submit/shield` | `{ record, depositTxHash }` | `Accepted` + поля кворума |
| `POST /submit/transfer` | `{ record }` | `Accepted` + поля кворума |
| `POST /submit/unshield` | `{ record, destination }` | `Accepted` (с `payout`) + поля кворума, включая `payoutTx` / `payoutBlob` |
| `POST /audit/reveal` | `{ viewingKey }` | `RevealReport` |

Формы (на проводе camelCase):

```jsonc
PoolState   { root, anchoredRoot | null, leafCount, nullifierCount, bundleCount, consumedDeposits }
Accepted    { bundleIndex, firstLeaf, newRoot, bundleHash, nullifiers[], commitments[], payout?: { destination, amount } }
кворум      { memoHex, anchorTx, payoutTx | null, payoutBlob | null, payoutError | null, signers: [rAddr…], refusals: [string…] }
RevealReport{ balance, notes: [{ direction: "incoming" | "outgoing", amount, leafIndex, bundleIndex, timestamp, spent, recipient? }] }
```

- `anchorTx` / `payoutTx` — хеши транзакций XRPL: ссылка `https://testnet.xrpl.org/transactions/<hash>`.
- Submit **блокируется до валидации анкора (и выплаты) в XRPL**: 5–15 с. Показывать прогресс.
- `payoutBlob` — мультиподписанная выплата. Если установлен `payoutError`, нота уже потрачена, а
  пользователь не оплачен; UI (или кто угодно) может переотправить блоб командой XRPL `submit` до его
  `LastLedgerSequence` либо просто повторить `POST` с теми же `{record, destination}` любому
  координатору (`/endorse` идемпотентен, сайдкары подпишут заново). Не показывать «оплачено», пока
  нет `payoutTx`.
- Суммы — в **дропах** (числа в `Accepted.payout.amount`, строки в полях XRPL).
- **Ключ просмотра — это авторизация для `/audit/reveal`**, он раскрывает всю историю держателя
  (только чтение, ничего более). UI отправляет его только тому координатору, которого выбрал
  пользователь, по настроенному пользователем URL, и предупреждает перед отправкой; никогда не
  хранит и не пересылает его. CORS стоит `*`, потому что у координатора нет ничего, что мог бы
  забрать чужой origin — ключ остаётся риском пользователя.

Полный клиент этого контракта со всеми негативными случаями — `e2e/testnet/run-services.mjs`.
Копировать оттуда.

## 3. Что обязан подготовить UI — `record`

`record` — это `BundleRecord` ровно в том виде, в каком его записал кошелёк (поля не трогать):

```jsonc
{ index, op: "shield" | "transfer" | "unshield", timestamp, external, anchor, flags, valueBalance,
  firstLeaf, actions: [{ nullifier, rk, cmx, … }], proof, bindingSignature }
```

Его выдаёт кошелёк держателя — на хакатоне CLI (`cargo build --release --example cli`), позже
локальный процесс над `confidant_core::Wallet`. Файл `*.wallet.json` содержит **ключи трат** (0600,
громкое предупреждение); он не покидает машину пользователя и никуда не отправляется.

| Сценарий | Шаги |
|---|---|
| **Синхронизация** | `GET /log?from=0` → записать `records` в `log.json`. Его принимает каждая сборка ниже. |
| **Shield** | 1. `cli wallet --file u.wallet.json build-shield <drops> shield.json log.json` → печатает `{ bundleHash, file }`. 2. XRPL-аккаунт пользователя отправляет `Payment` ровно на `<drops>` на аккаунт пула с одним memo `{ MemoType: hex("confidant/shield"), MemoData: <bundleHash hex> }`; дождаться валидации. 3. `POST /submit/shield { record: <shield.json>, depositTxHash }`. **Сначала сборка, потом депозит** — memo должно коммитить бандл; сумма должна совпасть с `delivered_amount`. |
| **Transfer** | синхронизация → `cli wallet --file u.wallet.json build-transfer <recipientAddressHex> <drops> transfer.json log.json` → `POST /submit/transfer`. В леджер попадает только анкор. |
| **Unshield** | синхронизация → `cli wallet --file u.wallet.json build-unshield <drops> <rDestination> unshield.json log.json` → `POST /submit/unshield` **с тем же destination** (его связывает подпись внутри бандла; другой адрес будет отклонён). По умолчанию — **свежий XRPL-адрес на каждый unshield** и совет вносить депозит со свежего аккаунта: адреса входа и выхода пула публичны, и их повторное использование позволяет анализу цепочки связать граничные события одного пользователя (ROADMAP §4.6). |
| **Раскрытие** | `cli wallet --file u.wallet.json info` → `viewingKey` → `POST /audit/reveal`. Показывает всю активность этого держателя и ничего более. |
| **Баланс** | `cli wallet --file u.wallet.json sync log.json` → `{ balance, synced, root }`. |

`init` создаёт файл кошелька; `info` печатает `{ address, viewingKey }` (адрес получателя = `address`, 43 байта hex).

## 4. Чем UI не занимается и чего нет в `backend/`

- UI никогда не проверяет депозиты, не вычисляет якоря, не держит ключи пула и подписантов и не
  решает, что что-то заанкорено: он передаёт `depositTxHash` и читает `anchoredRoot` / `anchorTx`.
- В `backend/` нет учётных записей, аутентификации, TLS, ограничения частоты (демо на localhost),
  хранения и бэкапа кошельков, скрипта предзаполнения (`scripts/`, владелец) и рендеринга.
- Формулировки о доверии для пользователя зафиксированы в `SPEC.ru.md` §4 и `README.md`; UI не
  должен заявлять больше (CI проверяет формулировки в спецификациях).

## 5. Запуск бэкенда для UI

```bash
cargo build --release --bin confidant-node --example cli
cd backend/quorum && npm install
SIGNERS=3 QUORUM=2 DIR=../../data/quorum npm run setup-pool        # один раз на пул (faucet testnet)
NODE_BIN=../../target/release/confidant-node DIR=../../data/quorum npm run start-all
```

| Что | Где |
|---|---|
| координатор (базовый URL для UI) | `http://127.0.0.1:7200` |
| узел координатора / узлы подписантов | `:7000` / `:7001–7003` (тот же API без полей кворума; `/log` работает на любом) |
| сайдкары подписантов | `:7101–7103` (не для UI) |
| аккаунт пула, sink, адреса подписантов, кворум | `data/quorum/config.json` (в gitignore) — UI берёт отсюда `pool` |
| ключи подписантов | `data/quorum/signer*.key.json`, 0600, в gitignore; UI их не читает |

Страницы explorer, на которые стоит ссылаться: аккаунт пула `https://testnet.xrpl.org/accounts/<pool>`
(каждый якорь — Payment на 1 дроп пул → sink с memo `confidant/anchor`), каждый депозит, каждый
`anchorTx`, каждый `payoutTx`.

Серверное развёртывание (systemd-юниты на каждый узел/сайдкар/координатор, nginx с CORS на одном
публичном порту) описано в `backend/deploy/README.md`; базовый URL и аккаунт пула общего testnet-
инстанса передаются вне репозитория (никогда не коммитятся). `backend/quorum/smoke.mjs` — полный
клиент контракта: shield, transfer, unshield, reveal против любого работающего координатора:

```bash
cd backend/quorum && POOL=<pool> COORDINATOR_URL=http://<host>:8787 CLI=../../target/release/examples/cli npm run smoke
```

## 6. Как меняется контракт

- **Добавление** поля в ответ обратно совместимо: достаточно попросить (issue или комментарий к PR),
  делается в тот же день.
- **Переименование/удаление** полей и **новые эндпоинты** — через PR, затрагивающий `backend/`; если
  меняется путь доверия (что проверяет узел, что подписывает сайдкар), сначала нужно решение аудитора
  по срезу; чистое изменение формы API — однострочное FYI аудитору.
- Контракт выше заморожен до конца хакатона, если оба лида не договорятся об ином в PR.

## 7. Ссылки

`backend/README.md` (сервисы в одном взгляде), `docs/CORE-API.md` (соответствие ядро ↔ HTTP),
`docs/E2E.md` §2b (эталонный клиент), `docs/NODE-OPERATOR.md` (запуск узла, восстановление),
`SPEC.ru.md` §6 (контракт API и формулировки о доверии). Английская версия: `docs/API-BOUNDARY.md`.
