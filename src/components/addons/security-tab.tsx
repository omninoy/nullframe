"use client";

import { useEffect, useState } from "react";
import {
  LuCircleAlert,
  LuInfo,
  LuShieldAlert,
  LuShieldCheck,
  LuTrash2,
} from "react-icons/lu";
import { AnimatedSwitch } from "@/components/ui/animated-switch";
import { Button } from "@/components/ui/button";
import { addonManager } from "@/lib/addons/addon-manager";
import { auditLogger } from "@/lib/addons/audit-logger";
import type { AuditLogEntry } from "@/lib/addons/types";

export function SecurityTab() {
  const [logs, setLogs] = useState<AuditLogEntry[]>([]);
  const [isSafeMode, setIsSafeMode] = useState(false);

  useEffect(() => {
    const unsubLog = auditLogger.subscribe(setLogs);
    const unsubManager = addonManager.subscribe((state) => {
      setIsSafeMode(state.safeMode);
    });
    return () => {
      unsubLog();
      unsubManager();
    };
  }, []);

  const handleToggleSafeMode = () => {
    if (isSafeMode) {
      addonManager.disableSafeMode();
    } else {
      addonManager.enableSafeMode();
    }
  };

  return (
    <div className="flex flex-col gap-6 text-sm">
      {/* 1. Safe Mode Section */}
      <section className="flex flex-col gap-2 rounded-lg border border-border bg-card p-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2.5">
            <div
              className={`grid size-9 place-items-center rounded-md ${
                isSafeMode
                  ? "bg-warning/15 text-warning"
                  : "bg-success/15 text-success"
              }`}
            >
              {isSafeMode ? (
                <LuShieldAlert className="size-5" />
              ) : (
                <LuShieldCheck className="size-5" />
              )}
            </div>
            <div>
              <div className="flex items-center gap-2">
                <span className="font-semibold text-foreground text-sm">
                  Безопасный режим (Safe Mode)
                </span>
                {isSafeMode && (
                  <span className="rounded bg-warning/20 px-1.5 py-0.5 text-[10px] font-medium text-warning">
                    АКТИВЕН
                  </span>
                )}
              </div>
              <p className="text-xs text-muted-foreground">
                Принудительно отключает все сторонние плагины, темы и CSS для
                восстановления работы приложения.
              </p>
            </div>
          </div>

          <AnimatedSwitch
            checked={isSafeMode}
            onCheckedChange={handleToggleSafeMode}
            aria-label="Переключить безопасный режим"
          />
        </div>
      </section>

      {/* 2. Audit Log Section */}
      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <div>
            <h3 className="font-semibold text-foreground text-base">
              Журнал безопасности и действий (Audit Log)
            </h3>
            <p className="text-muted-foreground text-xs">
              Фиксация вызовов API, сетевых запросов, изменений настроек и
              нарушений песочницы.
            </p>
          </div>
          <Button
            variant="outline"
            size="sm"
            className="h-7 text-xs gap-1"
            onClick={() => auditLogger.clear()}
          >
            <LuTrash2 className="size-3" />
            Очистить журнал
          </Button>
        </div>

        <div className="rounded-lg border border-border bg-black/30 overflow-hidden font-mono text-xs">
          {logs.length === 0 ? (
            <div className="p-8 text-center text-muted-foreground">
              Журнал аудита пуст. События безопасности не зафиксированы.
            </div>
          ) : (
            <div className="flex flex-col divide-y divide-border/40 max-h-[460px] overflow-y-auto">
              {logs.map((entry) => {
                let badgeClass = "bg-foreground/10 text-muted-foreground";
                let _Icon = LuInfo;
                if (entry.level === "security") {
                  badgeClass = "bg-destructive/20 text-destructive";
                  _Icon = LuCircleAlert;
                } else if (entry.level === "error") {
                  badgeClass = "bg-destructive/15 text-destructive";
                  _Icon = LuCircleAlert;
                } else if (entry.level === "warn") {
                  badgeClass = "bg-warning/20 text-warning";
                  _Icon = LuShieldAlert;
                }

                return (
                  <div
                    key={entry.id}
                    className="flex items-start gap-3 p-3 hover:bg-foreground/5 transition-colors"
                  >
                    <span className="text-[11px] text-muted-foreground shrink-0 mt-0.5">
                      {new Date(entry.timestamp).toLocaleTimeString()}
                    </span>
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] shrink-0 font-medium ${badgeClass}`}
                    >
                      {entry.category.toUpperCase()}
                    </span>
                    <div className="flex flex-col flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-semibold text-foreground truncate">
                          {entry.addonName}
                        </span>
                        <span className="text-muted-foreground text-[10px]">
                          ({entry.addonId})
                        </span>
                      </div>
                      <span className="text-foreground/90 mt-0.5 break-words">
                        {entry.message}
                      </span>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
