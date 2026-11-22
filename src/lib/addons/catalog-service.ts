import type { PluginManifest, ThemeManifest } from "./types";

export interface CatalogItem {
  id: string;
  name: string;
  author: string;
  version: string;
  description: string;
  type: "theme" | "plugin";
  downloads: number;
  rating: number;
  verified: boolean;
  previewUrl?: string;
  manifest: ThemeManifest | PluginManifest;
  content: string; // CSS or JS code bundle
}

export const COMMUNITY_CATALOG: CatalogItem[] = [
  {
    id: "catalog-tokyo-night-glass",
    name: "Tokyo Night Acrylic Glass",
    author: "Enkai",
    version: "1.4.0",
    description:
      "Атмосферная тема в стиле неонового ночного Токио с полупрозрачным акриловым размытием панелей.",
    type: "theme",
    downloads: 14200,
    rating: 4.9,
    verified: true,
    manifest: {
      id: "theme-tokyo-night-glass",
      name: "Tokyo Night Acrylic Glass",
      version: "1.4.0",
      author: "Enkai",
      description: "Атмосферная тема в стиле неонового ночного Токио",
      type: "theme",
      entry: `
        [data-active-addon-theme="theme-tokyo-night-glass"] {
          --background: #1a1b26 !important;
          --card: rgba(36, 40, 59, 0.7) !important;
          --popover: rgba(36, 40, 59, 0.85) !important;
          --primary: #7aa2f7 !important;
          --accent: #bb9af7 !important;
        }
        [data-active-addon-theme="theme-tokyo-night-glass"] .surface-material {
          backdrop-filter: blur(var(--theme-blur, 16px)) saturate(180%) !important;
        }
      `,
      settings: [
        {
          key: "blur",
          label: "Размытие фона (px)",
          type: "range",
          min: 0,
          max: 30,
          default: 16,
        },
      ],
    },
    content: `
      [data-active-addon-theme="theme-tokyo-night-glass"] {
        --background: #1a1b26 !important;
        --card: rgba(36, 40, 59, 0.7) !important;
        --popover: rgba(36, 40, 59, 0.85) !important;
        --primary: #7aa2f7 !important;
        --accent: #bb9af7 !important;
      }
      [data-active-addon-theme="theme-tokyo-night-glass"] .surface-material {
        backdrop-filter: blur(var(--theme-blur, 16px)) saturate(180%) !important;
      }
    `,
  },
  {
    id: "catalog-nordic-frost",
    name: "Nordic Frost Ultra",
    author: "ArcticCode",
    version: "2.1.0",
    description:
      "Холодная северная цветовая гамма с кристально четкими линиями и ледяными акцентами.",
    type: "theme",
    downloads: 8900,
    rating: 4.8,
    verified: true,
    manifest: {
      id: "theme-nordic-frost",
      name: "Nordic Frost Ultra",
      version: "2.1.0",
      author: "ArcticCode",
      description: "Ледяная палитра для максимальной концентрации",
      type: "theme",
      entry: `
        [data-active-addon-theme="theme-nordic-frost"] {
          --background: #2e3440 !important;
          --card: #3b4252 !important;
          --primary: #88c0d0 !important;
          --secondary: #81a1c1 !important;
        }
      `,
    },
    content: `
      [data-active-addon-theme="theme-nordic-frost"] {
        --background: #2e3440 !important;
        --card: #3b4252 !important;
        --primary: #88c0d0 !important;
        --secondary: #81a1c1 !important;
      }
    `,
  },
  {
    id: "catalog-activity-tracker",
    name: "Profile Activity Watcher",
    author: "NullFrameLabs",
    version: "1.1.0",
    description:
      "Плагин для отслеживания длительности активных профилей и вывода всплывающих уведомлений о сессиях.",
    type: "plugin",
    downloads: 6300,
    rating: 4.7,
    verified: true,
    manifest: {
      id: "plugin-activity-watcher",
      name: "Profile Activity Watcher",
      version: "1.1.0",
      author: "NullFrameLabs",
      description: "Мониторинг длительности работы профилей",
      type: "plugin",
      entry: `
        return {
          onLoad: function(ctx) {
            ctx.logger.info("Activity Watcher ready");
          },
          onEnable: function(ctx) {
            ctx.ui.showToast({
              title: "Activity Watcher запущен",
              description: "Мониторинг профилей активирован",
              variant: "default"
            });
          },
          onDisable: function(ctx) {
            ctx.logger.info("Activity Watcher stopped");
          }
        };
      `,
      permissions: [
        {
          id: "ui:toast",
          description: "Показ уведомлений о статусе активности",
        },
        {
          id: "storage:local",
          description: "Сохранение статистики длительности сессий",
        },
      ],
    },
    content: `
      return {
        onLoad: function(ctx) {
          ctx.logger.info("Activity Watcher ready");
        },
        onEnable: function(ctx) {
          ctx.ui.showToast({
            title: "Activity Watcher запущен",
            description: "Мониторинг профилей активирован",
            variant: "default"
          });
        },
        onDisable: function(ctx) {
          ctx.logger.info("Activity Watcher stopped");
        }
      };
    `,
  },
];
