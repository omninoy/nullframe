# Руководство по системе тем и расширений (THEME.md)

Документация по архитектуре семантических дизайн-токенов, системе тем, пользовательским CSS-сниппетам и изолированной песочнице плагинов в `nullframe`.

---

## 1. Архитектура дизайн-токенов

Система стилизации построена на **Tailwind CSS v4** с использованием современных директив `@theme inline` и CSS Custom Properties (`src/styles/globals.css`).

Каждый токен имеет семантическое назначение и доступен как в виде CSS-переменной (`var(--...)`), так и в виде Tailwind utility-класса:

| Семантический токен  | CSS Custom Property                      | Tailwind v4 Utility Class                   | Описание роли                                 |
| :------------------- | :--------------------------------------- | :------------------------------------------ | :-------------------------------------------- |
| **background**       | `--background`                           | `bg-background`                             | Базовый фон окна и приложения                 |
| **surface**          | `--surface`                              | `bg-surface`                                | Контейнеры и панели первого уровня            |
| **surface-elevated** | `--surface-elevated`                     | `bg-surface-elevated`                       | Приподнятые элементы (модальные окна, попапы) |
| **card**             | `--card`, `--card-foreground`            | `bg-card`, `text-card-foreground`           | Карточки и списки                             |
| **sidebar**          | `--sidebar`, `--sidebar-foreground`      | `bg-sidebar`, `text-sidebar-foreground`     | Боковая панель (Rail Nav)                     |
| **border**           | `--border`                               | `border-border`                             | Границы карточек и инпутов                    |
| **divider**          | `--divider`                              | `border-divider`, `divide-divider`          | Тонкие разделительные линии между строками    |
| **text-primary**     | `--text-primary`                         | `text-text-primary`                         | Высококонтрастный основной текст              |
| **text-secondary**   | `--text-secondary`                       | `text-text-secondary`                       | Второстепенный поясняющий текст               |
| **text-muted**       | `--text-muted`                           | `text-text-muted`                           | Подписи, подсказки, метаданные                |
| **text-disabled**    | `--text-disabled`                        | `text-text-disabled`                        | Текст неактивных элементов                    |
| **placeholder**      | `--placeholder`                          | `placeholder:text-placeholder`              | Плейсхолдеры полей ввода                      |
| **primary**          | `--primary`, `--primary-foreground`      | `bg-primary`, `text-primary-foreground`     | Главный акцентный цвет                        |
| **primary-hover**    | `--primary-hover`                        | `hover:bg-primary-hover`                    | Состояние наведения основной кнопки           |
| **primary-active**   | `--primary-active`                       | `active:bg-primary-active`                  | Состояние нажатия основной кнопки             |
| **secondary**        | `--secondary`, `--secondary-foreground`  | `bg-secondary`, `text-secondary-foreground` | Вторичные кнопки и фильтры                    |
| **secondary-hover**  | `--secondary-hover`                      | `hover:bg-secondary-hover`                  | Состояние наведения вторичной кнопки          |
| **accent**           | `--accent`, `--accent-foreground`        | `bg-accent`, `text-accent-foreground`       | Выделенные и выбранные элементы               |
| **success**          | `--success`, `--success-foreground`      | `bg-success`, `text-success`                | Статусы успеха и онлайн                       |
| **warning**          | `--warning`, `--warning-foreground`      | `bg-warning`, `text-warning`                | Предупреждения и статус паузы                 |
| **danger / error**   | `--destructive`, `--danger`, `--error`   | `bg-destructive`, `bg-danger`, `bg-error`   | Ошибки, удаление, бан                         |
| **info**             | `--info`, `--info-foreground`            | `bg-info`, `text-info`                      | Информационные плашки                         |
| **overlay / scrim**  | `--overlay`, `--scrim`                   | `bg-overlay`                                | Затемнение под модальными окнами              |
| **shadow-color**     | `--shadow-color`                         | `shadow-[var(--shadow-color)]`              | Тени карточек и всплывающих окон              |
| **focus-ring**       | `--ring`, `--focus-ring`                 | `ring-ring`, `ring-focus-ring`              | Фокусные кольца доступности                   |
| **scrollbar**        | `--scrollbar-thumb`, `--scrollbar-track` | (нативные правила скролла)                  | Стилизация скроллбаров                        |

---

## 2. Каталог встроенных тем

В приложении доступно 25 встроенных тем с автоматической проверкой контрастности WCAG AA (≥ 4.5:1 для текста):

1. **NullFrame Mono** (Dark) — фирменная монохромная тема NullFrame
2. **Tokyo Night** (Dark) — неоновая палитра ночного Токио
3. **Dracula** (Dark) — классический готический контраст
4. **Matchalk** (Dark) — мягкие тона матчи и мела
5. **Houston** (Dark) — космический глубокий синий
6. **Ayu Dark** (Dark) — теплый темный минимализм
7. **Ayu Light** (Light) — чистая светлая палитра
8. **Catppuccin Latte** (Light) — пастельный светлый уют
9. **Catppuccin Mocha** (Dark) — бархатистый темный Catppuccin
10. **Nord** (Dark) — арктические ледяные оттенки
11. **Gruvbox Dark** (Dark) — ретро-палитра в теплых тонах
12. **Gruvbox Light** (Light) — светлый винтаж
13. **Solarized Dark** (Dark) — эталонная инженерная палитра
14. **Solarized Light** (Light) — мягкий желтоватый пергамент
15. **One Dark** (Dark) — знаменитая палитра редактора Atom
16. **Monokai Pro** (Dark) — сочные контрастные акценты
17. **Rosé Pine** (Dark) — эстетика сосны и увядающей розы
18. **Rosé Pine Dawn** (Light) — рассветный светлый Rosé Pine
19. **GitHub Dark** (Dark) — официальная тёмная тема GitHub
20. **GitHub Light** (Light) — классический чистый светлый GitHub
21. **Cyberpunk 2077** (Dark) _(Новая)_ — Inky void, Neon Pink (`#ff2dbe`), Electric Cyan (`#03d8f3`), Matrix Mint
22. **Night Owl** (Dark) _(Новая)_ — культовая тема Сары Драснер (`#011627`, Function Blue, Keywords Purple)
23. **Everforest Dark** (Dark) _(Новая)_ — природные хвойные тона от Sainnhe
24. **Everforest Light** (Light) _(Новая)_ — светлый пергамент от Sainnhe
25. **Kanagawa Wave** (Dark) _(Новая)_ — традиционные японские цвета (Sumi Ink, Crystal Blue, Wave Aqua, Fuji White)

---

## 3. Как добавить новую тему без изменения компонентов

Для добавления новой темы достаточно добавить объект темы в массив `THEMES` в `src/lib/themes.ts`:

```ts
{
  id: "my-theme",
  name: "My Theme",
  mode: "dark", // или "light"
  colors: {
    "--background": "#12131a",
    "--foreground": "#e1e3ec",
    "--card": "#1a1c26",
    "--card-foreground": "#e1e3ec",
    "--popover": "#1a1c26",
    "--popover-foreground": "#e1e3ec",
    "--primary": "#a78bfa",
    "--primary-foreground": "#12131a",
    "--secondary": "#38bdf8",
    "--secondary-foreground": "#12131a",
    "--muted": "#262938",
    "--muted-foreground": "#9499ad",
    "--accent": "#f472b6",
    "--accent-foreground": "#12131a",
    "--destructive": "#f87171",
    "--destructive-foreground": "#12131a",
    "--success": "#4ade80",
    "--success-foreground": "#12131a",
    "--warning": "#fbbf24",
    "--warning-foreground": "#12131a",
    "--border": "#2f3347",
    "--chart-1": "#a78bfa",
    "--chart-2": "#38bdf8",
    "--chart-3": "#f472b6",
    "--chart-4": "#4ade80",
    "--chart-5": "#fbbf24",
  }
}
```

Все производные переменные (`--surface`, `--divider`, `--primary-hover`, `--info`, `--overlay` и т. д.) вычисляются движком `getDerivedThemeColors` автоматически с соблюдением требований WCAG AA!

---

## 4. Архитектура аддонов (Themes, Custom CSS, Plugins)

В приложении реализован полноценный модуль кастомизации (`src/lib/addons/`):

### 1. Темы оформления (`ThemeEngine`)

- Применяются через scoped тег `<style id="nullframe-addon-active-theme">`.
- Динамические параметры манифеста (`accentColor`, `backgroundBlur` и т. д.) транслируются в CSS-переменные вида `--theme-accent-color`, `--theme-background-blur`.

### 2. Пользовательские CSS-сниппеты (`CustomCssService`)

- Поддержка нескольких именованных сниппетов.
- Мгновенный **Live Preview** при вводе без перезагрузки.
- Кнопки автоподстановки системных токенов (`var(--primary)`, `var(--surface)` и др.).
- Экспорт и импорт в JSON.
- Санитизация опасных конструкций (`javascript:`, `expression(...)`).

### 3. Плагины в песочнице (`PluginSandboxInstance`)

- Сторонний код выполняется в изолированном скрытом `iframe` с атрибутом `sandbox="allow-scripts"` (без `allow-same-origin`).
- Заблокирован прямой доступ к `window.__TAURI__`, `localStorage`, `indexedDB`, родительскому `document` и `cookies`.
- Взаимодействие происходит по строго типизированному протоколу через `MessageChannel` с проверкой прав доступа (`PermissionGuard`).
- Все созданные таймеры, слушатели и UI-элементы регистрируются и автоматически очищаются при отключении плагина.

### 4. Безопасный режим (Safe Mode)

- Активируется в интерфейсе безопасности или параметром `?safe-mode=1`.
- Принудительно выгружает все сторонние темы, стили и плагины, гарантируя запуск приложения даже при наличии некорректного аддона.
- Audit Log протоколирует все вызовы API, изменения настроек и заблокированные попытки несанкционированного доступа.
