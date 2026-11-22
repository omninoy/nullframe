"use client";

import { useEffect, useState } from "react";
import {
  LuCheck,
  LuDownload,
  LuPalette,
  LuSlidersHorizontal,
  LuTrash2,
} from "react-icons/lu";
import { AnimatedSwitch } from "@/components/ui/animated-switch";
import { Button } from "@/components/ui/button";
import { addonManager } from "@/lib/addons/addon-manager";
import {
  type CatalogItem,
  COMMUNITY_CATALOG,
} from "@/lib/addons/catalog-service";
import { themeEngine } from "@/lib/addons/theme-engine";
import type { InstalledAddon, ThemeManifest } from "@/lib/addons/types";

export function ThemesTab() {
  const [installedThemes, setInstalledThemes] = useState<
    InstalledAddon<ThemeManifest>[]
  >([]);
  const [activeThemeId, setActiveThemeId] = useState<string | null>(null);
  const [selectedThemeForSettings, setSelectedThemeForSettings] =
    useState<InstalledAddon<ThemeManifest> | null>(null);

  useEffect(() => {
    const sync = () => {
      const all = addonManager.getInstalledAddons();
      const themes = all.filter(
        (a): a is InstalledAddon<ThemeManifest> => a.manifest.type === "theme",
      );
      setInstalledThemes(themes);
      const active = themeEngine.getActiveTheme();
      setActiveThemeId(active ? active.manifest.id : null);
    };

    sync();
    const unsubManager = addonManager.subscribe(sync);
    const unsubEngine = themeEngine.subscribe(sync);
    return () => {
      unsubManager();
      unsubEngine();
    };
  }, []);

  const handleToggleTheme = async (theme: InstalledAddon<ThemeManifest>) => {
    if (activeThemeId === theme.manifest.id) {
      await addonManager.setAddonEnabled(theme.manifest.id, false);
    } else {
      // Turn off other themes first
      for (const t of installedThemes) {
        if (t.enabled && t.manifest.id !== theme.manifest.id) {
          await addonManager.setAddonEnabled(t.manifest.id, false);
        }
      }
      await addonManager.setAddonEnabled(theme.manifest.id, true);
    }
  };

  const handleInstallFromCatalog = (item: CatalogItem) => {
    if (item.type !== "theme") return;
    addonManager.installAddon(item.manifest as ThemeManifest, item.content);
  };

  const catalogThemes = COMMUNITY_CATALOG.filter(
    (item) => item.type === "theme",
  );

  return (
    <div className="flex flex-col gap-6 text-sm">
      {/* 1. Installed Themes Section */}
      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="font-semibold text-foreground text-base">
              Установленные темы
            </h3>
            <p className="text-muted-foreground text-xs">
              Темы глубоко кастомизируют дизайн-токены, поверхности и эффекты
              интерфейса.
            </p>
          </div>
        </div>

        {installedThemes.length === 0 ? (
          <div className="rounded-lg border border-dashed border-border p-6 text-center text-muted-foreground">
            Нет установленных пользовательских тем. Выберите тему из каталога
            ниже.
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            {installedThemes.map((theme) => {
              const isActive = activeThemeId === theme.manifest.id;
              return (
                <div
                  key={theme.manifest.id}
                  className={`flex flex-col justify-between rounded-lg border p-4 transition-all ${
                    isActive
                      ? "border-primary bg-primary/5 shadow-xs"
                      : "border-border bg-card/60 hover:border-foreground/20"
                  }`}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="flex items-center gap-2.5">
                      <div className="grid size-9 shrink-0 place-items-center rounded-md bg-foreground/5 text-foreground">
                        <LuPalette className="size-4" />
                      </div>
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-foreground">
                            {theme.manifest.name}
                          </span>
                          <span className="rounded bg-foreground/10 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
                            v{theme.manifest.version}
                          </span>
                        </div>
                        <p className="line-clamp-2 text-xs text-muted-foreground">
                          {theme.manifest.description}
                        </p>
                      </div>
                    </div>

                    <AnimatedSwitch
                      checked={isActive}
                      onCheckedChange={() => handleToggleTheme(theme)}
                      aria-label={`Включить тему ${theme.manifest.name}`}
                    />
                  </div>

                  <div className="mt-4 flex items-center justify-between border-t border-border/50 pt-3 text-xs text-muted-foreground">
                    <span>Автор: {theme.manifest.author}</span>
                    <div className="flex items-center gap-1.5">
                      {theme.manifest.settings &&
                        theme.manifest.settings.length > 0 && (
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-7 gap-1 px-2 text-xs"
                            onClick={() => setSelectedThemeForSettings(theme)}
                          >
                            <LuSlidersHorizontal className="size-3" />
                            Параметры
                          </Button>
                        )}
                      <Button
                        variant="ghost"
                        size="sm"
                        className="h-7 text-destructive hover:bg-destructive/10 hover:text-destructive px-2 text-xs"
                        onClick={() =>
                          addonManager.uninstallAddon(theme.manifest.id)
                        }
                      >
                        <LuTrash2 className="size-3" />
                      </Button>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </section>

      {/* 2. Theme Settings Drawer / Modal if selected */}
      {selectedThemeForSettings && (
        <section className="rounded-lg border border-primary/40 bg-card p-4">
          <div className="flex items-center justify-between border-b border-border pb-3">
            <div className="flex items-center gap-2">
              <LuSlidersHorizontal className="size-4 text-primary" />
              <span className="font-semibold text-foreground">
                Настройки темы: {selectedThemeForSettings.manifest.name}
              </span>
            </div>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setSelectedThemeForSettings(null)}
            >
              Закрыть
            </Button>
          </div>
          <div className="mt-4 grid grid-cols-1 gap-4 md:grid-cols-2">
            {selectedThemeForSettings.manifest.settings?.map((setting) => {
              const currentVal =
                selectedThemeForSettings.userSettings[setting.key] ??
                setting.default;
              return (
                <div key={setting.key} className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-xs font-medium text-foreground">
                    <span>{setting.label}</span>
                    <span className="font-mono text-muted-foreground">
                      {String(currentVal)}
                    </span>
                  </div>
                  {setting.type === "range" && (
                    <input
                      type="range"
                      min={setting.min ?? 0}
                      max={setting.max ?? 100}
                      step={setting.step ?? 1}
                      value={Number(currentVal)}
                      onChange={(e) =>
                        themeEngine.updateSetting(
                          setting.key,
                          Number(e.target.value),
                        )
                      }
                      className="h-1.5 w-full cursor-pointer accent-primary"
                    />
                  )}
                  {setting.type === "color" && (
                    <div className="flex items-center gap-2">
                      <input
                        type="color"
                        value={String(currentVal)}
                        onChange={(e) =>
                          themeEngine.updateSetting(setting.key, e.target.value)
                        }
                        className="size-7 cursor-pointer rounded border border-border"
                      />
                      <input
                        type="text"
                        value={String(currentVal)}
                        onChange={(e) =>
                          themeEngine.updateSetting(setting.key, e.target.value)
                        }
                        className="h-7 w-28 rounded border border-input bg-transparent px-2 font-mono text-xs"
                      />
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </section>
      )}

      {/* 3. Community Theme Catalog */}
      <section className="flex flex-col gap-3">
        <h3 className="font-semibold text-foreground text-base">
          Каталог проверенных тем
        </h3>
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          {catalogThemes.map((item) => {
            const isInstalled = installedThemes.some(
              (t) => t.manifest.id === item.manifest.id,
            );
            return (
              <div
                key={item.id}
                className="flex flex-col justify-between rounded-lg border border-border bg-card p-4 hover:border-foreground/20"
              >
                <div>
                  <div className="flex items-center justify-between">
                    <span className="font-medium text-foreground">
                      {item.name}
                    </span>
                    <span className="rounded-full bg-success/10 px-2 py-0.5 text-[10px] text-success font-medium">
                      ★ {item.rating} · {item.downloads.toLocaleString()}{" "}
                      скачиваний
                    </span>
                  </div>
                  <p className="mt-1 text-xs text-muted-foreground">
                    {item.description}
                  </p>
                </div>
                <div className="mt-4 flex items-center justify-between border-t border-border/50 pt-3">
                  <span className="text-xs text-muted-foreground">
                    Автор: {item.author}
                  </span>
                  {isInstalled ? (
                    <Button
                      variant="outline"
                      size="sm"
                      disabled
                      className="h-7 text-xs gap-1"
                    >
                      <LuCheck className="size-3 text-success" />
                      Установлено
                    </Button>
                  ) : (
                    <Button
                      variant="default"
                      size="sm"
                      className="h-7 text-xs gap-1"
                      onClick={() => handleInstallFromCatalog(item)}
                    >
                      <LuDownload className="size-3" />
                      Установить
                    </Button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
