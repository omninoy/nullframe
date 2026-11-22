import { auditLogger } from "./audit-logger";
import type { InstalledAddon, ThemeManifest } from "./types";

const THEME_STYLE_ID = "nullframe-addon-active-theme";
const THEME_VARS_ID = "nullframe-addon-theme-variables";

class ThemeEngineService {
  private activeTheme: InstalledAddon<ThemeManifest> | null = null;
  private listeners = new Set<
    (theme: InstalledAddon<ThemeManifest> | null) => void
  >();

  public getActiveTheme(): InstalledAddon<ThemeManifest> | null {
    return this.activeTheme;
  }

  public applyTheme(
    addon: InstalledAddon<ThemeManifest>,
    cssContent: string,
  ): void {
    if (typeof document === "undefined") return;

    this.activeTheme = addon;

    // 1. Inject or update CSS variables derived from theme settings
    this.applyThemeVariables(addon);

    // 2. Inject or update theme stylesheet
    let styleEl = document.getElementById(
      THEME_STYLE_ID,
    ) as HTMLStyleElement | null;
    if (!styleEl) {
      styleEl = document.createElement("style");
      styleEl.id = THEME_STYLE_ID;
      styleEl.setAttribute("data-addon-id", addon.manifest.id);
      document.head.appendChild(styleEl);
    }
    styleEl.textContent = this.sanitizeThemeCss(cssContent);

    // 3. Document attribute for CSS theme targeting
    document.documentElement.setAttribute(
      "data-active-addon-theme",
      addon.manifest.id,
    );

    auditLogger.record({
      addonId: addon.manifest.id,
      addonName: addon.manifest.name,
      level: "info",
      category: "ui",
      message: `Тема «${addon.manifest.name}» применена`,
    });

    this.notify();
  }

  public removeTheme(): void {
    if (typeof document === "undefined") return;

    const oldTheme = this.activeTheme;
    this.activeTheme = null;

    const styleEl = document.getElementById(THEME_STYLE_ID);
    if (styleEl) styleEl.remove();

    const varsEl = document.getElementById(THEME_VARS_ID);
    if (varsEl) varsEl.remove();

    document.documentElement.removeAttribute("data-active-addon-theme");

    if (oldTheme) {
      auditLogger.record({
        addonId: oldTheme.manifest.id,
        addonName: oldTheme.manifest.name,
        level: "info",
        category: "ui",
        message: `Тема «${oldTheme.manifest.name}» отключена`,
      });
    }

    this.notify();
  }

  public updateSetting(key: string, value: unknown): void {
    if (!this.activeTheme) return;

    this.activeTheme.userSettings[key] = value;
    this.applyThemeVariables(this.activeTheme);
    this.notify();

    auditLogger.record({
      addonId: this.activeTheme.manifest.id,
      addonName: this.activeTheme.manifest.name,
      level: "info",
      category: "ui",
      message: `Параметр темы «${key}» изменен на «${String(value)}»`,
    });
  }

  private applyThemeVariables(addon: InstalledAddon<ThemeManifest>): void {
    if (typeof document === "undefined") return;

    let varsEl = document.getElementById(
      THEME_VARS_ID,
    ) as HTMLStyleElement | null;
    if (!varsEl) {
      varsEl = document.createElement("style");
      varsEl.id = THEME_VARS_ID;
      document.head.appendChild(varsEl);
    }

    const definitions = addon.manifest.settings ?? [];
    const lines: string[] = [":root {"];

    for (const def of definitions) {
      const val = addon.userSettings[def.key] ?? def.default;
      const cssVarName = `--theme-${def.key.replace(/([A-Z])/g, "-$1").toLowerCase()}`;

      if (def.type === "range" && typeof val === "number") {
        lines.push(`  ${cssVarName}: ${val}px;`);
      } else if (def.type === "color" && typeof val === "string") {
        lines.push(`  ${cssVarName}: ${val};`);
      } else if (def.type === "boolean") {
        lines.push(`  ${cssVarName}: ${val ? "1" : "0"};`);
      } else if (
        def.type === "image" &&
        typeof val === "string" &&
        val.trim().length > 0
      ) {
        lines.push(`  ${cssVarName}: url("${val}");`);
      } else if (val !== undefined && val !== null) {
        lines.push(`  ${cssVarName}: ${String(val)};`);
      }
    }

    lines.push("}");
    varsEl.textContent = lines.join("\n");
  }

  private sanitizeThemeCss(css: string): string {
    return css
      .replace(/javascript:/gi, "blocked-javascript:")
      .replace(/vbscript:/gi, "blocked-vbscript:")
      .replace(/-moz-binding/gi, "blocked-binding")
      .replace(/expression\s*\(/gi, "blocked-expression(");
  }

  public subscribe(
    listener: (theme: InstalledAddon<ThemeManifest> | null) => void,
  ): () => void {
    this.listeners.add(listener);
    listener(this.activeTheme);
    return () => this.listeners.delete(listener);
  }

  private notify(): void {
    this.listeners.forEach((listener) => listener(this.activeTheme));
  }
}

export const themeEngine = new ThemeEngineService();
