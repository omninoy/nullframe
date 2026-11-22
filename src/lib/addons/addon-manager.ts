import { auditLogger } from "./audit-logger";
import { customCssService } from "./custom-css-store";
import { PluginSandboxInstance } from "./plugin-sandbox";
import { themeEngine } from "./theme-engine";
import type {
  InstalledAddon,
  PluginManifest,
  ThemeManifest,
  ToastOptions,
} from "./types";

const ADDONS_STORAGE_KEY = "nullframe_installed_addons";
const SAFE_MODE_KEY = "nullframe_safe_mode_active";

export interface AddonStateEvent {
  addons: InstalledAddon[];
  safeMode: boolean;
}

class AddonManagerService {
  private installedAddons = new Map<string, InstalledAddon>();
  private activePluginSandboxes = new Map<string, PluginSandboxInstance>();
  private listeners = new Set<(event: AddonStateEvent) => void>();
  private isSafeMode = false;
  private uiToastHandler: ((opts: ToastOptions) => void) | null = null;

  constructor() {
    this.checkSafeMode();
    this.loadInstalled();
    if (!this.isSafeMode && typeof window !== "undefined") {
      this.bootstrapAddons();
    }
  }

  public registerToastHandler(
    handler: (opts: ToastOptions) => void,
  ): () => void {
    this.uiToastHandler = handler;
    return () => {
      if (this.uiToastHandler === handler) this.uiToastHandler = null;
    };
  }

  private checkSafeMode(): void {
    if (typeof window === "undefined") return;
    const urlParams = new URLSearchParams(window.location.search);
    const hasSafeParam = urlParams.get("safe-mode") === "1";
    const hasStoredFlag =
      window.sessionStorage.getItem(SAFE_MODE_KEY) === "true";

    if (hasSafeParam || hasStoredFlag) {
      this.isSafeMode = true;
      auditLogger.record({
        addonId: "system",
        addonName: "System",
        level: "warn",
        category: "lifecycle",
        message:
          "Приложение запущено в Безопасном Режиме (Safe Mode). Все сторонние аддоны деактивированы.",
      });
    }
  }

  public enableSafeMode(): void {
    this.isSafeMode = true;
    if (typeof window !== "undefined") {
      window.sessionStorage.setItem(SAFE_MODE_KEY, "true");
    }
    // Deactivate everything
    this.activePluginSandboxes.forEach((sandbox) => sandbox.terminate());
    this.activePluginSandboxes.clear();
    themeEngine.removeTheme();
    customCssService.removeAllStyles();
    this.notify();
  }

  public disableSafeMode(): void {
    this.isSafeMode = false;
    if (typeof window !== "undefined") {
      window.sessionStorage.removeItem(SAFE_MODE_KEY);
    }
    this.bootstrapAddons();
    this.notify();
  }

  private loadInstalled(): void {
    if (typeof window === "undefined") return;
    try {
      const stored = window.localStorage.getItem(ADDONS_STORAGE_KEY);
      if (stored) {
        const list: InstalledAddon[] = JSON.parse(stored);
        list.forEach((addon) =>
          this.installedAddons.set(addon.manifest.id, addon),
        );
      } else {
        this.seedBuiltInAddons();
      }
    } catch {
      this.seedBuiltInAddons();
    }
  }

  private seedBuiltInAddons(): void {
    // Built-in starter themes and demo plugins
    const demoTheme: InstalledAddon<ThemeManifest> = {
      manifest: {
        id: "theme-cyberpunk-neon",
        name: "Cyberpunk Glow",
        version: "1.0.0",
        author: "NullFrame Community",
        description:
          "Высококонтрастная неоновая тема со светящимися границами карточек",
        type: "theme",
        entry: `
          [data-active-addon-theme="theme-cyberpunk-neon"] {
            --primary: #ff2dbe !important;
            --accent: #03d8f3 !important;
          }
          [data-active-addon-theme="theme-cyberpunk-neon"] .data-table > tbody > tr[data-table-row]:hover > td {
            box-shadow: inset 0 0 12px rgba(255, 45, 190, 0.15);
          }
        `,
        settings: [
          {
            key: "neonIntensity",
            label: "Интенсивность неона",
            type: "range",
            min: 1,
            max: 10,
            default: 5,
          },
        ],
      },
      enabled: false,
      installedAt: Date.now(),
      userSettings: { neonIntensity: 5 },
    };

    const demoPlugin: InstalledAddon<PluginManifest> = {
      manifest: {
        id: "plugin-quick-notifier",
        name: "Profile Action Notifier",
        version: "1.0.0",
        author: "NullFrame Team",
        description:
          "Пример плагина: выводит уведомления о статусах и действиях в приложении",
        type: "plugin",
        entry: `
          return {
            onLoad: function(ctx) {
              ctx.logger.info("Notifier plugin loaded");
            },
            onEnable: function(ctx) {
              ctx.ui.showToast({
                title: "Notifier активирован",
                description: "Плагин готов к работе в песочнице",
                variant: "success"
              });
            },
            onDisable: function(ctx) {
              ctx.logger.info("Notifier disabled");
            }
          };
        `,
        permissions: [
          { id: "ui:toast", description: "Показ системных тостов" },
          { id: "storage:local", description: "Сохранение настроек плагина" },
        ],
      },
      enabled: false,
      installedAt: Date.now(),
      userSettings: {},
    };

    this.installedAddons.set(demoTheme.manifest.id, demoTheme);
    this.installedAddons.set(demoPlugin.manifest.id, demoPlugin);
    this.persist();
  }

  private persist(): void {
    if (typeof window === "undefined") return;
    try {
      const list = Array.from(this.installedAddons.values());
      window.localStorage.setItem(ADDONS_STORAGE_KEY, JSON.stringify(list));
    } catch {
      // ignore quota
    }
  }

  private async bootstrapAddons(): Promise<void> {
    if (this.isSafeMode) return;

    for (const addon of this.installedAddons.values()) {
      if (addon.enabled) {
        try {
          await this.activateAddon(addon);
        } catch (err: unknown) {
          addon.error = (err as Error).message;
          auditLogger.record({
            addonId: addon.manifest.id,
            addonName: addon.manifest.name,
            level: "error",
            category: "lifecycle",
            message: `Сбой при автозапуске аддона: ${(err as Error).message}`,
          });
        }
      }
    }
    this.notify();
  }

  public async setAddonEnabled(id: string, enabled: boolean): Promise<void> {
    const addon = this.installedAddons.get(id);
    if (!addon) return;

    addon.enabled = enabled;
    addon.error = null;
    this.persist();

    if (enabled && !this.isSafeMode) {
      await this.activateAddon(addon);
    } else {
      this.deactivateAddon(addon);
    }
    this.notify();
  }

  private async activateAddon(addon: InstalledAddon): Promise<void> {
    if (addon.manifest.type === "theme") {
      const themeAddon = addon as InstalledAddon<ThemeManifest>;
      themeEngine.applyTheme(themeAddon, themeAddon.manifest.entry);
    } else if (addon.manifest.type === "plugin") {
      const pluginAddon = addon as InstalledAddon<PluginManifest>;
      let sandbox = this.activePluginSandboxes.get(pluginAddon.manifest.id);
      if (!sandbox) {
        sandbox = new PluginSandboxInstance(pluginAddon, (action, payload) => {
          if (action === "toast" && this.uiToastHandler) {
            this.uiToastHandler(payload as ToastOptions);
          }
        });
        this.activePluginSandboxes.set(pluginAddon.manifest.id, sandbox);
        await sandbox.start(pluginAddon.manifest.entry);
      }
      sandbox.enable();
    }
  }

  private deactivateAddon(addon: InstalledAddon): void {
    if (addon.manifest.type === "theme") {
      if (themeEngine.getActiveTheme()?.manifest.id === addon.manifest.id) {
        themeEngine.removeTheme();
      }
    } else if (addon.manifest.type === "plugin") {
      const sandbox = this.activePluginSandboxes.get(addon.manifest.id);
      if (sandbox) {
        sandbox.terminate();
        this.activePluginSandboxes.delete(addon.manifest.id);
      }
    }
  }

  public installAddon(
    manifest: ThemeManifest | PluginManifest,
    rawCodeOrCss: string,
  ): void {
    const installed: InstalledAddon = {
      manifest: {
        ...manifest,
        entry: rawCodeOrCss,
      },
      enabled: false,
      installedAt: Date.now(),
      userSettings: {},
    };

    this.installedAddons.set(installed.manifest.id, installed);
    this.persist();
    auditLogger.record({
      addonId: installed.manifest.id,
      addonName: installed.manifest.name,
      level: "info",
      category: "lifecycle",
      message: `Аддон «${installed.manifest.name}» успешно установлен`,
    });
    this.notify();
  }

  public uninstallAddon(id: string): void {
    const addon = this.installedAddons.get(id);
    if (!addon) return;

    this.deactivateAddon(addon);
    this.installedAddons.delete(id);
    this.persist();
    auditLogger.record({
      addonId: id,
      addonName: addon.manifest.name,
      level: "info",
      category: "lifecycle",
      message: `Аддон «${addon.manifest.name}» удален`,
    });
    this.notify();
  }

  public getInstalledAddons(): InstalledAddon[] {
    return Array.from(this.installedAddons.values());
  }

  public getAddonsState(): AddonStateEvent {
    return {
      addons: this.getInstalledAddons(),
      safeMode: this.isSafeMode,
    };
  }

  public subscribe(listener: (event: AddonStateEvent) => void): () => void {
    this.listeners.add(listener);
    listener(this.getAddonsState());
    return () => this.listeners.delete(listener);
  }

  private notify(): void {
    const state = this.getAddonsState();
    this.listeners.forEach((listener) => listener(state));
  }
}

export const addonManager = new AddonManagerService();
