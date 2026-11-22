"use client";

import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  LuActivity,
  LuArrowUpDown,
  LuCheck,
  LuChevronDown,
  LuChevronRight,
  LuCopy,
  LuEllipsis,
  LuGlobe,
  LuLoaderCircle,
  LuPencil,
  LuRefreshCw,
  LuSearch,
  LuShieldAlert,
  LuTrash2,
} from "react-icons/lu";
import { toast } from "sonner";
import { FlagIcon } from "@/components/flag-icon";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { useVpnSubscriptions } from "@/hooks/use-vpn-subscriptions";
import { extractCountryCode } from "@/lib/flag-utils";
import { cn } from "@/lib/utils";
import type {
  VpnSubscription,
  VpnSubscriptionNode,
} from "@/types/vpn-subscription";
import { VpnSubscriptionDialog } from "./vpn-subscription-dialog";

interface VpnSubscriptionAccordionProps {
  onSelectNode?: (node: VpnSubscriptionNode) => void;
  subscriptionManager?: ReturnType<typeof useVpnSubscriptions>;
}

export function VpnSubscriptionAccordion({
  onSelectNode,
  subscriptionManager,
}: VpnSubscriptionAccordionProps) {
  const { t } = useTranslation();
  const defaultManager = useVpnSubscriptions();
  const {
    subscriptions,
    nodes,
    loading,
    refreshingSubId,
    pingingSubId,
    updateSubscription,
    deleteSubscription,
    pingSubscription,
    pingSingleNode,
  } = subscriptionManager ?? defaultManager;

  const [expandedSubId, setExpandedSubId] = useState<string | null>(null);
  const [editingSub, setEditingSub] = useState<VpnSubscription | null>(null);
  const [searchQueries, setSearchQueries] = useState<Record<string, string>>(
    {},
  );
  const [sortByPing, setSortByPing] = useState<Record<string, boolean>>({});
  const [pingingNodeId, setPingingNodeId] = useState<string | null>(null);
  const [copiedNodeId, setCopiedNodeId] = useState<string | null>(null);

  const toggleExpand = (subId: string) => {
    setExpandedSubId((prev) => (prev === subId ? null : subId));
  };

  const handleCopyUri = (node: VpnSubscriptionNode) => {
    navigator.clipboard.writeText(node.raw_uri);
    setCopiedNodeId(node.id);
    toast.success(
      t("vpnSubscriptions.copied", "Ссылка на подключение скопирована"),
    );
    setTimeout(() => {
      setCopiedNodeId(null);
    }, 2000);
  };

  const handlePingNode = async (node: VpnSubscriptionNode) => {
    try {
      setPingingNodeId(node.id);
      const res = await pingSingleNode(node.id, node.subscription_id);
      if (res.is_valid && res.latency_ms !== undefined) {
        toast.success(`${node.name}: ${res.latency_ms} ms`);
      } else {
        toast.error(`${node.name}: недоступен (таймаут)`);
      }
    } catch {
      toast.error(`${node.name}: ошибка проверки`);
    } finally {
      setPingingNodeId(null);
    }
  };

  if (loading && subscriptions.length === 0) {
    return (
      <div className="flex items-center justify-center py-12 text-muted-foreground text-sm">
        <LuLoaderCircle className="w-5 h-5 mr-2 animate-spin text-primary" />
        {t("common.buttons.loading", "Загрузка подписок...")}
      </div>
    );
  }

  if (subscriptions.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center p-8 border border-dashed rounded-xl bg-muted/10 text-center">
        <div className="p-3 bg-muted rounded-full text-muted-foreground mb-3">
          <LuGlobe className="w-6 h-6" />
        </div>
        <h4 className="text-sm font-semibold mb-1">
          {t("vpnSubscriptions.emptyTitle", "Нет добавленных подписок")}
        </h4>
        <p className="text-xs text-muted-foreground max-w-sm mb-4">
          {t(
            "vpnSubscriptions.emptyDesc",
            "Добавьте ссылку на подписку от вашего провайдера (VLESS, Trojan, VMess, Hysteria2, SS или Clash YAML), чтобы автоматически загрузить серверы.",
          )}
        </p>
      </div>
    );
  }

  return (
    <div className="space-y-3">
      {subscriptions.map((sub) => {
        const isExpanded = expandedSubId === sub.id;
        const subNodes = nodes[sub.id] || [];
        const isRefreshing = refreshingSubId === sub.id;
        const isPinging = pingingSubId === sub.id;
        const searchQuery = (searchQueries[sub.id] || "").toLowerCase().trim();
        const isSortPing = Boolean(sortByPing[sub.id]);

        // Extract unique country codes for subscription card header
        const uniqueCountries = Array.from(
          new Set(
            subNodes
              .map(
                (n) => n.country_code || extractCountryCode(undefined, n.name),
              )
              .filter(Boolean) as string[],
          ),
        );

        let displayNodes = subNodes.filter((node) => {
          if (!searchQuery) return true;
          return (
            node.name.toLowerCase().includes(searchQuery) ||
            node.protocol.toLowerCase().includes(searchQuery) ||
            Boolean(node.country_code?.toLowerCase().includes(searchQuery)) ||
            node.server.toLowerCase().includes(searchQuery)
          );
        });

        if (isSortPing) {
          displayNodes = [...displayNodes].sort((a, b) => {
            if (
              a.is_valid &&
              b.is_valid &&
              typeof a.latency_ms === "number" &&
              typeof b.latency_ms === "number"
            ) {
              return a.latency_ms - b.latency_ms;
            }
            if (a.is_valid && !b.is_valid) return -1;
            if (!a.is_valid && b.is_valid) return 1;
            if (a.latency_ms !== null && b.latency_ms === null) return -1;
            if (a.latency_ms === null && b.latency_ms !== null) return 1;
            return 0;
          });
        }

        const availableCount = subNodes.filter((n) => n.is_valid).length;

        return (
          <div
            key={sub.id}
            className="border rounded-xl bg-card overflow-hidden transition-all shadow-xs"
          >
            {/* Subscription Accordion Header */}
            <div className="flex items-center justify-between p-3.5 hover:bg-muted/30 transition-colors">
              <button
                type="button"
                className="flex items-center gap-3 cursor-pointer select-none flex-1 min-w-0 text-left bg-transparent border-0 p-0 text-inherit font-inherit"
                onClick={() => toggleExpand(sub.id)}
              >
                <span className="p-1 hover:bg-muted rounded text-muted-foreground shrink-0 inline-flex items-center justify-center">
                  {isExpanded ? (
                    <LuChevronDown className="w-4 h-4 text-primary" />
                  ) : (
                    <LuChevronRight className="w-4 h-4" />
                  )}
                </span>

                {/* Country Flags left of subscription name */}
                <div className="flex items-center gap-1 shrink-0">
                  {uniqueCountries.slice(0, 3).map((code) => (
                    <FlagIcon
                      key={code}
                      countryCode={code}
                      squared
                      className="size-4 shrink-0 rounded-[2px]"
                    />
                  ))}
                  {uniqueCountries.length === 0 && (
                    <LuGlobe className="size-4 shrink-0 text-muted-foreground/60" />
                  )}
                </div>

                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2 flex-wrap">
                    <span className="font-semibold text-sm truncate">
                      {sub.name}
                    </span>
                    <Badge
                      variant="secondary"
                      className="text-[11px] font-normal px-2 py-0"
                    >
                      {sub.node_count}{" "}
                      {t("vpnSubscriptions.nodesUnit", "серверов")}
                    </Badge>
                    {sub.auto_update && (
                      <Badge
                        variant="outline"
                        className="text-[10px] text-emerald-600 dark:text-emerald-400 border-emerald-500/30"
                      >
                        Auto
                      </Badge>
                    )}
                    {sub.error && (
                      <Badge
                        variant="destructive"
                        className="text-[10px] flex items-center gap-1"
                      >
                        <LuShieldAlert className="w-3 h-3" />
                        {sub.error}
                      </Badge>
                    )}
                  </div>
                  <div className="text-[11px] text-muted-foreground flex items-center gap-3 mt-0.5 truncate">
                    <span>
                      {sub.last_updated_at
                        ? `${t("vpnSubscriptions.updated", "Обновлено")}: ${new Date(
                            sub.last_updated_at * 1000,
                          ).toLocaleDateString()} ${new Date(
                            sub.last_updated_at * 1000,
                          ).toLocaleTimeString([], {
                            hour: "2-digit",
                            minute: "2-digit",
                          })}`
                        : t("vpnSubscriptions.neverUpdated", "Не обновлялось")}
                    </span>
                    {availableCount > 0 && (
                      <span className="text-emerald-600 dark:text-emerald-400">
                        • {availableCount}{" "}
                        {t("vpnSubscriptions.available", "доступно")}
                      </span>
                    )}
                  </div>
                </div>
              </button>

              {/* Action Buttons for this specific subscription */}
              <div className="flex items-center gap-1.5 shrink-0 ml-2">
                <Button
                  size="sm"
                  variant="outline"
                  className="h-8 px-2.5 text-xs gap-1.5"
                  onClick={() => pingSubscription(sub.id)}
                  disabled={isPinging || subNodes.length === 0}
                  title={t(
                    "vpnSubscriptions.pingAllHint",
                    "Пинговать только узлы этой подписки",
                  )}
                >
                  {isPinging ? (
                    <LuLoaderCircle className="w-3.5 h-3.5 animate-spin text-primary" />
                  ) : (
                    <LuActivity className="w-3.5 h-3.5 text-blue-500" />
                  )}
                  <span className="hidden sm:inline">
                    {t("vpnSubscriptions.pingBtn", "Пинг")}
                  </span>
                </Button>

                <Button
                  size="sm"
                  variant="outline"
                  className="h-8 px-2.5 text-xs gap-1.5 text-emerald-600 dark:text-emerald-400 border-emerald-500/20 hover:bg-emerald-500/10"
                  onClick={() => updateSubscription(sub.id)}
                  disabled={isRefreshing}
                  title={t(
                    "vpnSubscriptions.updateHint",
                    "Обновить подписку (загрузить новые, удалить старые)",
                  )}
                >
                  <LuRefreshCw
                    className={cn(
                      "w-3.5 h-3.5",
                      isRefreshing && "animate-spin text-primary",
                    )}
                  />
                  <span className="hidden sm:inline">
                    {t("vpnSubscriptions.updateBtn", "Обновить")}
                  </span>
                </Button>

                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      size="icon"
                      variant="ghost"
                      className="size-8 text-muted-foreground hover:text-foreground"
                    >
                      <LuEllipsis className="w-4 h-4" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <DropdownMenuItem onClick={() => setEditingSub(sub)}>
                      <LuPencil className="w-3.5 h-3.5 mr-2" />
                      {t("common.buttons.edit", "Редактировать")}
                    </DropdownMenuItem>
                    <DropdownMenuItem
                      className="text-destructive focus:text-destructive"
                      onClick={() => deleteSubscription(sub.id)}
                    >
                      <LuTrash2 className="w-3.5 h-3.5 mr-2" />
                      {t("common.buttons.delete", "Удалить")}
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              </div>
            </div>

            {/* Expanded Accordion Body (Nodes List) */}
            {isExpanded && (
              <div className="border-t bg-muted/15 p-3 space-y-3">
                {/* Search & Sort by Ping Header */}
                <div className="flex items-center gap-2">
                  <div className="relative flex-1">
                    <LuSearch className="absolute left-2.5 top-2.5 h-3.5 w-3.5 text-muted-foreground" />
                    <Input
                      placeholder={t(
                        "vpnSubscriptions.searchPlaceholder",
                        "Поиск по названию, стране, протоколу...",
                      )}
                      value={searchQueries[sub.id] || ""}
                      onChange={(e) =>
                        setSearchQueries((prev) => ({
                          ...prev,
                          [sub.id]: e.target.value,
                        }))
                      }
                      className="h-8 pl-8 text-xs bg-background"
                    />
                  </div>
                  <Button
                    size="sm"
                    variant={isSortPing ? "secondary" : "outline"}
                    className={cn(
                      "h-8 px-2.5 text-xs gap-1.5 shrink-0 transition-colors",
                      isSortPing &&
                        "bg-primary/10 text-primary border-primary/30",
                    )}
                    onClick={() =>
                      setSortByPing((prev) => ({
                        ...prev,
                        [sub.id]: !prev[sub.id],
                      }))
                    }
                    title={t(
                      "vpnSubscriptions.sortByPing",
                      "Сортировать по пингу",
                    )}
                  >
                    <LuArrowUpDown className="size-3.5" />
                    <span>{t("vpnSubscriptions.sortByPing", "По пингу")}</span>
                  </Button>
                  <div className="text-[11px] text-muted-foreground shrink-0 font-mono">
                    {displayNodes.length} / {subNodes.length}
                  </div>
                </div>

                {displayNodes.length === 0 ? (
                  <div className="text-center py-6 text-xs text-muted-foreground">
                    {searchQuery
                      ? t(
                          "vpnSubscriptions.noSearchMatches",
                          "Ничего не найдено по запросу",
                        )
                      : t(
                          "vpnSubscriptions.noNodesInSub",
                          "Нет доступных серверов в подписке",
                        )}
                  </div>
                ) : (
                  <div className="border rounded-lg bg-background overflow-hidden divide-y text-xs">
                    {displayNodes.map((node) => {
                      const isNodePinging = pingingNodeId === node.id;
                      const isCopied = copiedNodeId === node.id;
                      const countryCode =
                        node.country_code ||
                        extractCountryCode(undefined, node.name);

                      let latencyColor = "text-muted-foreground border-muted";
                      if (node.is_valid && node.latency_ms !== undefined) {
                        if (node.latency_ms < 100) {
                          latencyColor =
                            "text-emerald-600 bg-emerald-50 dark:bg-emerald-950/30 border-emerald-500/30";
                        } else if (node.latency_ms < 250) {
                          latencyColor =
                            "text-amber-600 bg-amber-50 dark:bg-amber-950/30 border-amber-500/30";
                        } else {
                          latencyColor =
                            "text-red-600 bg-red-50 dark:bg-red-950/30 border-red-500/30";
                        }
                      } else if (node.is_valid === false) {
                        latencyColor =
                          "text-destructive bg-destructive/10 border-destructive/20";
                      }

                      return (
                        <div
                          key={node.id}
                          className="flex items-center justify-between p-2.5 hover:bg-muted/40 transition-colors gap-2"
                        >
                          <div className="flex items-center gap-2.5 min-w-0 flex-1">
                            {/* Graphic SVG Flag to the left of node name */}
                            {countryCode ? (
                              <FlagIcon
                                countryCode={countryCode}
                                squared
                                className="size-4 shrink-0 rounded-[2px]"
                              />
                            ) : (
                              <LuGlobe className="size-4 shrink-0 text-muted-foreground/60" />
                            )}
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center gap-1.5 flex-wrap">
                                <span className="font-medium truncate">
                                  {node.name}
                                </span>
                                <Badge
                                  variant="secondary"
                                  className="text-[10px] px-1.5 py-0 uppercase font-mono tracking-tight"
                                >
                                  {node.protocol}
                                </Badge>
                              </div>
                              <div className="text-[11px] text-muted-foreground font-mono truncate">
                                {node.server}:{node.port}
                              </div>
                            </div>
                          </div>

                          {/* Ping latency & actions */}
                          <div className="flex items-center gap-2 shrink-0">
                            <span
                              className={cn(
                                "text-[11px] font-mono px-2 py-0.5 rounded border select-none",
                                latencyColor,
                              )}
                            >
                              {node.latency_ms !== undefined &&
                              node.latency_ms !== null
                                ? `${node.latency_ms} ms`
                                : node.is_valid === false
                                  ? t("vpnSubscriptions.timeout", "Timeout")
                                  : "—"}
                            </span>

                            <Button
                              size="icon"
                              variant="ghost"
                              className="size-7 text-muted-foreground hover:text-foreground"
                              onClick={() => handlePingNode(node)}
                              disabled={isNodePinging}
                              title={t(
                                "vpnSubscriptions.pingNode",
                                "Проверить пинг узла",
                              )}
                            >
                              {isNodePinging ? (
                                <LuLoaderCircle className="size-3.5 animate-spin text-primary" />
                              ) : (
                                <LuActivity className="size-3.5" />
                              )}
                            </Button>

                            <Button
                              size="icon"
                              variant="ghost"
                              className="size-7 text-muted-foreground hover:text-foreground"
                              onClick={() => handleCopyUri(node)}
                              title={t(
                                "vpnSubscriptions.copyUri",
                                "Копировать URI",
                              )}
                            >
                              {isCopied ? (
                                <LuCheck className="size-3.5 text-emerald-500" />
                              ) : (
                                <LuCopy className="size-3.5" />
                              )}
                            </Button>

                            {onSelectNode && (
                              <Button
                                size="sm"
                                variant="outline"
                                className="h-7 text-xs ml-1"
                                onClick={() => onSelectNode(node)}
                              >
                                {t("common.buttons.select", "Выбрать")}
                              </Button>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>
            )}
          </div>
        );
      })}

      <VpnSubscriptionDialog
        isOpen={editingSub !== null}
        onClose={() => setEditingSub(null)}
        subscription={editingSub}
      />
    </div>
  );
}
