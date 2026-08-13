# Vantage Box — план разработки

> Минималистичный десктопный GUI для sing-box: берёт существующий `config.json`, рулит рантаймом через Clash API (`experimental.clash_api`, `127.0.0.1:9090`). Без своего формата конфига, без магии.

## Стек

- **Tauri 2** (Rust backend + webview frontend) — бинарник ~5–10 МБ, минимум памяти, официальные плагины под всё нужное: `tauri-plugin-global-shortcut`, tray API, `tauri-plugin-autostart`, `tauri-plugin-single-instance`, `tauri-plugin-updater`.
- **Frontend**: Svelte + TypeScript + Vite (минимальный рантайм; React/Vue тоже ок, но Svelte легче). Стили — plain CSS или UnoCSS.
- **Rust-крейты**: `tokio` (async), `reqwest` (HTTP к Clash API), `tokio-tungstenite` (WebSocket для `/traffic`, `/logs`, `/connections`), `serde_json`, `notify` (watch файла настроек).

## Архитектура

```
┌─────────────────────────────────────────┐
│ Tauri app (user-level, без админа)      │
│  ├─ UI (webview): дашборд, логи,        │
│  │   selector'ы, редактор конфига       │
│  ├─ Rust core:                          │
│  │   ├─ ClashApiClient (HTTP+WS 9090)   │
│  │   ├─ ServiceController (start/stop)  │
│  │   ├─ Settings (settings.json+watch)  │
│  │   └─ Hotkeys, Tray                   │
└──────────────┬──────────────────────────┘
               │ управление процессом
┌──────────────▼──────────────────────────┐
│ sing-box (системный сервис, elevated)   │
│  └─ Clash API на 127.0.0.1:9090         │
└─────────────────────────────────────────┘
```

Ключевое разделение: **GUI всегда работает без прав админа**, привилегии нужны только процессу sing-box (TUN-интерфейс). Всё управление рантаймом идёт по localhost API — прав не требует.

## Решения по требованиям

### Админ-права без постоянных UAC-запросов

Ставим sing-box как **системный сервис**, elevation нужен один раз — при установке/регистрации сервиса.

- **Windows**: Windows Service (`sc create` или крейт `windows-service`). Установка сервиса — единственный UAC-запрос. Дальше GUI стартует/стопит сервис через Service Control Manager: даём пользователю право управления конкретным сервисом (`sc sdset` с SDDL при установке) — тогда start/stop без UAC вообще.
- **Linux**: systemd unit (system-level). Управление через `systemctl` + polkit-правило, разрешающее группе пользователя start/stop юнита без пароля. Альтернатива проще: `setcap cap_net_admin+ep` на бинарник sing-box и запуск как user-процесс.
- **macOS**: launchd daemon (`/Library/LaunchDaemons`), регистрация через `SMAppService` или один запрос пароля при установке plist. Управление — `launchctl kickstart/kill`.

Fallback-режим без TUN (только локальный прокси-порт) — вообще без привилегий, полезно для первого запуска.

### Простая установка

- **Windows** (приоритет): NSIS-инсталлер из коробки Tauri. Инсталлер: ставит приложение, скачивает/кладёт бинарник sing-box, регистрирует сервис (тот самый один UAC). Плюс portable zip. Позже — winget.
- **Linux**: AppImage + .deb; AUR позже.
- **macOS**: .dmg; brew cask позже.
- Автообновления GUI через `tauri-plugin-updater`.

### Управление бинарником sing-box

- Бинарник качаем с GitHub releases (проверка sha256), обновляем отдельно от GUI: ручная кнопка «обновить» + опциональное автообновление (в `settings.json`: `off` / `notify` / `auto`). Обновление = скачать → `sing-box check` на текущем конфиге → остановить сервис → заменить → запустить.
- Если в `settings.json` указан свой путь к бинарнику — используем его, автообновление для него не трогаем (только уведомления).
- **Матрица совместимости**: каждый релиз Vantage Box декларирует поддерживаемый диапазон версий sing-box (semver, напр. vantage-box 0.0.1 → `~1.1.1`). Версию определяем через `sing-box version`. Вне диапазона (напр. `>1.2.0`) — работаем, но показываем предупреждение в UI; автообновление никогда не ставит версию вне диапазона.

### Настройки как у VS Code (dot-files-friendly)

Один читаемый `settings.json` в стандартной директории конфигов:

- Windows: `%APPDATA%/vantage-box/settings.json`
- Linux: `~/.config/vantage-box/settings.json`
- macOS: `~/Library/Application Support/vantage-box/settings.json`

Содержимое: путь к `config.json` sing-box, путь к бинарнику sing-box (вручную; если не задан — управляемый Vantage Box бинарник), адрес API, хоткеи, автозапуск, тема, поведение трея, политика автообновления бинарника. Файл — единственный источник правды: UI настроек редактирует его, ручные правки подхватываются на лету через `notify` (file watcher). Комментарии — поддержать JSONC. Схема — публикуем JSON Schema для автокомплита в редакторах.

### Редактирование конфига

Сразу в MVP: встроенный редактор — Monaco/CodeMirror с JSON Schema sing-box (автокомплит, валидация), проверка `sing-box check` перед применением. Плюс кнопка «открыть config.json в системном редакторе» и watch файла → предложение мягкого перезапуска.

### Управление и selector'ы

- Стоп/старт/рестарт сервиса (ServiceController, см. выше).
- Мягкий перезапуск: перед рестартом снять текущие выборы selector'ов (`GET /proxies`), после старта восстановить (`POST /proxies/{tag}`). Учесть, что sing-box сам умеет кэшировать выбор через `cache_file` — использовать как первый уровень, восстановление поверх как страховка.
- Selector'ы: карточки групп, переключение одним кликом, мгновенно, без перезапуска. Latency-тест группы (`GET /group/{name}/delay`).

### Логи и статистика

- Отдельный экран логов в UI: реалтайм-лента (`/logs`, WS), фильтр по уровню (errors only), пауза, поиск, копирование/экспорт, ring-buffer в памяти (не жрём RAM).
- `/traffic` (WS) — график скорости + счётчики.
- `/connections` (WS) — таблица активных соединений: домен/IP, outbound, скорость; позже `DELETE /connections/{id}`.

### Глобальные хоткеи и трей

- `tauri-plugin-global-shortcut`: работает на всех трёх ОС. Дефолт `Ctrl+Alt+P` — попап-меню выбора прокси у трея; ещё хоткей на toggle on/off. Все биндинги — в `settings.json`.
- Трей: иконка меняет цвет/бейдж по состоянию (off / запущен / какой outbound активен). Меню: selector'ы, toggle, restart, open logs. Закрытие окна — сворачивание в трей.

## Этапы

**M0 — скелет (1 нед.)**
Tauri 2 + Svelte, `settings.json` (чтение/watch/schema), ClashApiClient (HTTP+WS), подключение к уже запущенному sing-box.

**M1 — ядро MVP (3–4 нед.)**
Дашборд: статус, selector'ы, traffic. Экран логов realtime с фильтром. Встроенный редактор конфига (Monaco + JSON Schema + `sing-box check`). ServiceController для Windows (сервис + SDDL, один UAC при установке). Мягкий перезапуск с сохранением выбора. Генерация secret на лету (рантайм-копия конфига). Менеджер бинарника sing-box: свой путь / скачивание, обновление, матрица совместимости. NSIS-инсталлер.

**M2 — трей и хоткеи (1–2 нед.)**
Трей с динамической иконкой и меню, глобальные хоткеи, попап выбора прокси, автозапуск, single instance.

**M3 — кроссплатформа (2 нед.)**
Linux (systemd/setcap, AppImage/deb), macOS (launchd, dmg). CI: GitHub Actions матрица сборки, автообновления.

**M4 — потом**
Таблица соединений с kill (`DELETE /connections/{id}`), свой fallback поверх selector (пинг активного outbound → автопереключение на резервный), подписки.

## Риски и заметки

- Clash API: слушать строго `127.0.0.1`. Secret не храним в настройках пользователя — генерируем на лету при каждом запуске сервиса: GUI создаёт рантайм-копию конфига с подставленным `experimental.clash_api.secret` (пользовательский `config.json` не трогаем), sing-box запускается с ней. Если пользователь сам задал secret в конфиге — уважаем его.
- Права на управление сервисом Windows (SDDL) — самая хитрая часть; fallback: elevation-запрос только на start/stop через отдельный маленький helper.
- Endpoint'ы `/logs`, `/traffic`, `/connections` — WebSocket, не polling.
- WebView2 на Windows предустановлен с Win10+, но инсталлер должен уметь докачать (Tauri это делает сам).
- Не хранить состояние в GUI: источник правды — sing-box API + `settings.json`. GUI можно убить/перезапустить в любой момент.
