"use client";

import { useEffect, useState } from "react";
import {
  LuCheck,
  LuCircleAlert,
  LuDownload,
  LuPuzzle,
  LuShieldAlert,
  LuTrash2,
} from "react-icons/lu";
import { AnimatedSwitch } from "@/components/ui/animated-switch";
import { Button } from "@/components/ui/button";
import { addonManager } from "@/lib/addons/addon-manager";
import {
  type CatalogItem,
  COMMUNITY_CATALOG,
} from "@/lib/addons/catalog-service";
import type { InstalledAddon, PluginManifest } from "@/lib/addons/types";

export function PluginsTab() {
  const [installedPlugins, setInstalledPlugins] = useState<
    InstalledAddon<PluginManifest>[]
  >([]);
  const [pendingInstallItem, setPendingInstallItem] =
    useState<CatalogItem | null>(null);

  useEffect(() => {
    const sync = () => {
      const all = addonManager.getInstalledAddons();
      const plugins = all.filter(
        (a): a is InstalledAddon<PluginManifest> =>
          a.manifest.type === "plugin",
      );
      setInstalledPlugins(plugins);
    };

    sync();
    return addonManager.subscribe(sync);
  }, []);

  const handleTogglePlugin = async (plugin: InstalledAddon<PluginManifest>) => {
    await addonManager.setAddonEnabled(plugin.manifest.id, !plugin.enabled);
  };

  const handleConfirmInstall = () => {
    if (!pendingInstallItem) return;
    addonManager.installAddon(
      pendingInstallItem.manifest as PluginManifest,
      pendingInstallItem.content,
    );
    setPendingInstallItem(null);
  };

  const catalogPlugins = COMMUNITY_CATALOG.filter(
    (item) => item.type === "plugin",
  );

  return (
    <div className="flex flex-col gap-6 text-sm">
      {/* 1. Installed Plugins */}
      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="font-semibold text-foreground text-base">
              Установленные плагины
            </h3>
            <p className="text-muted-foreground text-xs">
              Плагины работают в изолированной песочнице и расширяют возможности
              браузера.
            </p>
          </div>
        </div>

        {installedPlugins.length === 0 ? (
          <div className="rounded-lg border border-dashed border-border p-6 text-center text-muted-foreground">
            Нет установленных плагинов. Выберите расширение из магазина ниже.
          </div>
        ) : (
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
            {installedPlugins.map((plugin) => (
              <div
                key={plugin.manifest.id}
                className={`flex flex-col justify-between rounded-lg border p-4 transition-all ${
                  plugin.enabled
                    ? "border-primary/50 bg-primary/5 shadow-xs"
                    : "border-border bg-card/60"
                }`}
              >
                <div>
                  <div className="flex items-start justify-between gap-3">
                    <div className="flex items-center gap-2.5">
                      <div className="grid size-9 shrink-0 place-items-center rounded-md bg-foreground/5 text-foreground">
                        <LuPuzzle className="size-4 text-primary" />
                      </div>
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="font-medium text-foreground">
                            {plugin.manifest.name}
                          </span>
                          <span className="rounded bg-foreground/10 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
                            v{plugin.manifest.version}
                          </span>
                        </div>
                        <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
                          {plugin.manifest.description}
                        </p>
                      </div>
                    </div>
                    <AnimatedSwitch
                      checked={plugin.enabled}
                      onCheckedChange={() => handleTogglePlugin(plugin)}
                      aria-label={`Включить плагин ${plugin.manifest.name}`}
                    />
                  </div>

                  {/* Permissions tags */}
                  <div className="mt-3 flex flex-wrap gap-1">
                    {plugin.manifest.permissions?.map((perm) => (
                      <span
                        key={perm.id}
                        className="rounded bg-foreground/8 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground"
                      >
                        {perm.id}
                      </span>
                    ))}
                  </div>

                  {plugin.error && (
                    <div className="mt-2 flex items-center gap-1.5 rounded bg-destructive/10 p-2 text-xs text-destructive">
                      <LuCircleAlert className="size-3.5 shrink-0" />
                      <span>{plugin.error}</span>
                    </div>
                  )}
                </div>

                <div className="mt-4 flex items-center justify-between border-t border-border/50 pt-3 text-xs text-muted-foreground">
                  <span>Автор: {plugin.manifest.author}</span>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 text-destructive hover:bg-destructive/10 hover:text-destructive px-2 text-xs"
                    onClick={() =>
                      addonManager.uninstallAddon(plugin.manifest.id)
                    }
                  >
                    <LuTrash2 className="size-3 mr-1" />
                    Удалить
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </section>

      {/* 2. Community Plugins Store */}
      <section className="flex flex-col gap-3">
        <h3 className="font-semibold text-foreground text-base">
          Магазин расширений
        </h3>
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          {catalogPlugins.map((item) => {
            const isInstalled = installedPlugins.some(
              (p) => p.manifest.id === item.manifest.id,
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
                      Установлен
                    </Button>
                  ) : (
                    <Button
                      variant="default"
                      size="sm"
                      className="h-7 text-xs gap-1"
                      onClick={() => setPendingInstallItem(item)}
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

      {/* 3. Permission Confirmation Modal */}
      {pendingInstallItem && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-xs p-4">
          <div className="w-full max-w-md rounded-xl border border-border bg-card p-6 shadow-xl">
            <div className="flex items-center gap-2 text-warning">
              <LuShieldAlert className="size-5" />
              <h4 className="font-semibold text-foreground text-base">
                Запрос разрешений
              </h4>
            </div>
            <p className="mt-2 text-xs text-muted-foreground">
              Плагин{" "}
              <strong className="text-foreground">
                {pendingInstallItem.name}
              </strong>{" "}
              запрашивает доступ к следующим возможностям:
            </p>

            <div className="mt-4 flex flex-col gap-2 rounded-lg border border-border/80 bg-foreground/5 p-3">
              {(pendingInstallItem.manifest as PluginManifest).permissions?.map(
                (perm) => (
                  <div key={perm.id} className="flex flex-col text-xs">
                    <span className="font-mono font-medium text-primary">
                      {perm.id}
                    </span>
                    <span className="text-muted-foreground text-[11px]">
                      {perm.description}
                    </span>
                  </div>
                ),
              )}
            </div>

            <div className="mt-6 flex items-center justify-end gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => setPendingInstallItem(null)}
              >
                Отмена
              </Button>
              <Button
                variant="default"
                size="sm"
                onClick={handleConfirmInstall}
              >
                Доверять и установить
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
