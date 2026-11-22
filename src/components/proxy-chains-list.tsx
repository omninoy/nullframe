"use client";

import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  LuActivity,
  LuArrowRight,
  LuGlobe,
  LuLink,
  LuLoaderCircle,
  LuPencil,
  LuPlus,
  LuTrash2,
} from "react-icons/lu";
import { FlagIcon } from "@/components/flag-icon";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { getCachedVpnResult } from "@/components/vpn-check-button";
import { useProxyChains } from "@/hooks/use-proxy-chains";
import { useProxyEvents } from "@/hooks/use-proxy-events";
import { useVpnEvents } from "@/hooks/use-vpn-events";
import { useVpnSubscriptions } from "@/hooks/use-vpn-subscriptions";
import { extractCountryCode } from "@/lib/flag-utils";
import { getCachedProxyResult } from "@/lib/proxy-check-store";
import { cn } from "@/lib/utils";
import type { ProxyChain } from "@/types/proxy-chain";
import { ProxyChainDialog } from "./proxy-chain-dialog";

export function ProxyChainsList() {
  const { t } = useTranslation();
  const { chains, deleteChain, pingChain, pingingChainId } = useProxyChains();
  const { storedProxies } = useProxyEvents();
  const { vpnConfigs } = useVpnEvents();
  const { nodes: subscriptionNodes } = useVpnSubscriptions();

  const [editingChain, setEditingChain] = useState<ProxyChain | null>(null);
  const [showCreateDialog, setShowCreateDialog] = useState(false);

  // Map of proxy/node ID to display details
  const proxyDetailsMap = useMemo(() => {
    const map = new Map<
      string,
      { name: string; countryCode?: string; subtitle: string }
    >();

    // Chains store WireGuard configs as `vpn-<config id>`.
    for (const vpn of vpnConfigs) {
      map.set(`vpn-${vpn.id}`, {
        name: `WG — ${vpn.name}`,
        countryCode:
          getCachedVpnResult(vpn.id)?.country_code ??
          extractCountryCode(undefined, vpn.name) ??
          undefined,
        subtitle: "WireGuard",
      });
    }

    for (const p of storedProxies) {
      const res = getCachedProxyResult(p.id);
      const cc = res?.country_code ?? extractCountryCode(p.geo_country, p.name);
      map.set(p.id, {
        name: p.name,
        countryCode: cc ?? undefined,
        subtitle: `${p.proxy_settings.proxy_type.toUpperCase()} · ${p.proxy_settings.host}:${p.proxy_settings.port}`,
      });
    }

    for (const subNodes of Object.values(subscriptionNodes)) {
      for (const n of subNodes) {
        const details = {
          name: n.name,
          countryCode: n.country_code,
          subtitle: `${n.protocol.toUpperCase()} · ${n.server}:${n.port}`,
        };
        // Chains store subscription nodes as `subnode-<node id>` (the same
        // form profiles use), so key both spellings or the card lookup misses
        // and renders the raw ID instead of the node's name.
        map.set(n.id, details);
        map.set(`subnode-${n.id}`, details);
      }
    }

    return map;
  }, [storedProxies, subscriptionNodes, vpnConfigs]);

  return (
    <div className="flex flex-col h-full space-y-4">
      <div>
        <h3 className="text-sm font-semibold">
          {t("proxyChains.title", "Цепочки прокси (Proxy Chains)")}
        </h3>
        <p className="text-xs text-muted-foreground">
          {t(
            "proxyChains.subtitle",
            "Маршрутизация трафика профиля через промежуточный узел (Relay) к конечному выходу (Exit).",
          )}
        </p>
      </div>

      {chains.length === 0 ? (
        <div className="flex flex-col items-center justify-center p-8 border border-dashed rounded-xl bg-muted/10 text-center my-auto">
          <div className="size-12 rounded-full bg-primary/10 flex items-center justify-center mb-3">
            <LuLink className="size-6 text-primary" />
          </div>
          <h4 className="text-sm font-medium mb-1">
            {t("proxyChains.emptyTitle", "Цепочек прокси пока нет")}
          </h4>
          <p className="text-xs text-muted-foreground max-w-sm mb-4">
            {t(
              "proxyChains.emptyDescription",
              "Создайте цепочку, объединив два любых прокси (Hop 1 Relay → Hop 2 Exit), чтобы скрыть реальный IP как от сайтов, так и от промежуточных узлов.",
            )}
          </p>
          <Button
            size="sm"
            variant="outline"
            className="gap-1.5"
            onClick={() => setShowCreateDialog(true)}
          >
            <LuPlus className="size-4" />
            {t("proxyChains.createButton", "Создать цепочку")}
          </Button>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-2.5 overflow-y-auto pr-1">
          {chains.map((chain) => {
            const hop1 = proxyDetailsMap.get(chain.hop1_proxy_id);
            const hop2 = proxyDetailsMap.get(chain.hop2_proxy_id);

            return (
              <div
                key={chain.id}
                className="flex items-center justify-between p-3.5 rounded-xl border bg-card/60 hover:bg-card hover:border-primary/30 transition-all shadow-xs gap-3"
              >
                <div className="flex flex-col gap-1.5 min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-sm text-foreground truncate">
                      {chain.name}
                    </span>
                    <Badge
                      variant="soft"
                      className="text-[10px] uppercase font-mono px-1.5 py-0"
                    >
                      Chain 2-Hop
                    </Badge>
                  </div>

                  {/* Route visualization */}
                  <div className="flex items-center gap-2 text-xs flex-wrap">
                    {/* Hop 1 */}
                    <div className="flex items-center gap-1.5 px-2 py-1 rounded-md bg-muted/50 border max-w-[200px] truncate">
                      <span className="text-[10px] font-mono text-muted-foreground uppercase">
                        Relay:
                      </span>
                      {hop1?.countryCode ? (
                        <FlagIcon
                          countryCode={hop1.countryCode}
                          squared
                          className="size-3.5 rounded-[2px] shrink-0"
                        />
                      ) : (
                        <LuGlobe className="size-3.5 text-muted-foreground shrink-0" />
                      )}
                      <span className="truncate font-medium">
                        {hop1?.name ?? chain.hop1_proxy_id}
                      </span>
                    </div>

                    <LuArrowRight className="size-3.5 text-primary shrink-0" />

                    {/* Hop 2 */}
                    <div className="flex items-center gap-1.5 px-2 py-1 rounded-md bg-muted/50 border max-w-[200px] truncate">
                      <span className="text-[10px] font-mono text-muted-foreground uppercase">
                        Exit:
                      </span>
                      {hop2?.countryCode ? (
                        <FlagIcon
                          countryCode={hop2.countryCode}
                          squared
                          className="size-3.5 rounded-[2px] shrink-0"
                        />
                      ) : (
                        <LuGlobe className="size-3.5 text-muted-foreground shrink-0" />
                      )}
                      <span className="truncate font-medium">
                        {hop2?.name ?? chain.hop2_proxy_id}
                      </span>
                    </div>
                  </div>
                </div>

                {/* Actions */}
                <div className="flex items-center gap-1 shrink-0">
                  {typeof chain.latency_ms === "number" ? (
                    <Badge
                      variant="soft"
                      className={cn(
                        "text-[10px] px-1.5 py-0 font-mono shrink-0",
                        chain.is_valid === false
                          ? "bg-destructive/10 text-destructive"
                          : "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
                      )}
                    >
                      {chain.latency_ms} ms
                    </Badge>
                  ) : null}
                  <Button
                    size="icon"
                    variant="ghost"
                    className="size-8"
                    disabled={pingingChainId === chain.id}
                    onClick={() => void pingChain(chain.id)}
                    title={t(
                      "proxyChains.pingBtn",
                      "Проверить цепочку целиком",
                    )}
                  >
                    {pingingChainId === chain.id ? (
                      <LuLoaderCircle className="size-3.5 animate-spin text-primary" />
                    ) : (
                      <LuActivity className="size-3.5" />
                    )}
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="size-8"
                    onClick={() => setEditingChain(chain)}
                    title={t("common.buttons.edit", "Редактировать")}
                  >
                    <LuPencil className="size-3.5" />
                  </Button>
                  <Button
                    size="icon"
                    variant="ghost"
                    className="size-8 text-destructive hover:text-destructive"
                    onClick={() => void deleteChain(chain.id)}
                    title={t("common.buttons.delete", "Удалить")}
                  >
                    <LuTrash2 className="size-3.5" />
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <ProxyChainDialog
        isOpen={showCreateDialog || editingChain !== null}
        onClose={() => {
          setShowCreateDialog(false);
          setEditingChain(null);
        }}
        chain={editingChain}
      />
    </div>
  );
}
