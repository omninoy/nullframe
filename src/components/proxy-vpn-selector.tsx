"use client";

import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  LuActivity,
  LuCheck,
  LuChevronDown,
  LuChevronRight,
  LuGlobe,
  LuLink,
  LuPlus,
} from "react-icons/lu";
import { FlagIcon } from "@/components/flag-icon";
import { Badge } from "@/components/ui/badge";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { getCachedVpnResult } from "@/components/vpn-check-button";
import { useProxyChains } from "@/hooks/use-proxy-chains";
import { useProxyEvents } from "@/hooks/use-proxy-events";
import { useVpnEvents } from "@/hooks/use-vpn-events";
import { useVpnSubscriptions } from "@/hooks/use-vpn-subscriptions";
import { extractCountryCode } from "@/lib/flag-utils";
import { getCachedProxyResult } from "@/lib/proxy-check-store";
import { cn } from "@/lib/utils";

export interface ProxyVpnSelectorContentProps {
  value?: string;
  onSelect: (
    id: string | undefined,
    option?: { name: string; countryCode?: string; protocol?: string },
  ) => void;
  allowNone?: boolean;
  allowChains?: boolean;
  excludeId?: string;
  onClose?: () => void;
  onOpenAddProxy?: () => void;
}

export function ProxyVpnSelectorContent({
  value,
  onSelect,
  allowNone = true,
  allowChains = false,
  excludeId,
  onClose,
  onOpenAddProxy,
}: ProxyVpnSelectorContentProps) {
  const { t } = useTranslation();
  const [tab, setTab] = useState<"proxies" | "vpn">(() =>
    value?.startsWith("vpn-") || value?.startsWith("subnode-")
      ? "vpn"
      : "proxies",
  );
  const [pingingNodeId, setPingingNodeId] = useState<string | null>(null);

  const { storedProxies } = useProxyEvents();
  const { vpnConfigs } = useVpnEvents();
  const { chains } = useProxyChains();
  const {
    subscriptions,
    nodes: subNodesMap,
    pingSingleNode,
  } = useVpnSubscriptions();

  const [expandedSubs, setExpandedSubs] = useState<Record<string, boolean>>(
    () => {
      if (!value?.startsWith("subnode-")) return {};
      const targetNodeId = value.slice(8);
      for (const [subId, nodes] of Object.entries(subNodesMap)) {
        if (nodes.some((n) => n.id === targetNodeId)) {
          return { [subId]: true };
        }
      }
      return {};
    },
  );

  const toggleExpand = (subId: string) => {
    setExpandedSubs((prev) => ({
      ...prev,
      [subId]: !prev[subId],
    }));
  };

  const sortedProxies = useMemo(() => {
    return [...storedProxies]
      .filter((p) => p.id !== excludeId)
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [storedProxies, excludeId]);

  const sortedChains = useMemo(() => {
    return chains
      .filter((c) => c.id !== excludeId)
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [chains, excludeId]);

  const sortedVpns = useMemo(() => {
    return [...vpnConfigs]
      .filter((v) => `vpn-${v.id}` !== excludeId)
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [vpnConfigs, excludeId]);

  return (
    <div className="flex flex-col">
      <div className="flex border-b p-1 bg-muted/40 gap-1">
        <button
          type="button"
          className={cn(
            "flex-1 py-1 px-2.5 text-xs font-medium rounded-md transition-all text-center cursor-pointer",
            tab === "proxies"
              ? "bg-background text-foreground shadow-xs font-semibold"
              : "text-muted-foreground hover:text-foreground",
          )}
          onClick={() => setTab("proxies")}
        >
          {t("proxies.management.tabProxies", "Прокси")}
        </button>
        <button
          type="button"
          className={cn(
            "flex-1 py-1 px-2.5 text-xs font-medium rounded-md transition-all text-center cursor-pointer",
            tab === "vpn"
              ? "bg-background text-foreground shadow-xs font-semibold"
              : "text-muted-foreground hover:text-foreground",
          )}
          onClick={() => setTab("vpn")}
        >
          {t("proxies.management.tabVpns", "VPN")}
        </button>
      </div>

      <Command>
        <CommandInput
          placeholder={
            tab === "proxies"
              ? t("createProfile.proxy.search", "Поиск прокси...")
              : t("vpnSubscriptions.searchPlaceholder", "Поиск VPN / узлов...")
          }
        />
        <CommandList className="max-h-72 overflow-y-auto">
          {/* Only the VPN tab needs an explicit empty state: collapsed
              subscription blocks render no CommandItems, so cmdk would show
              "nothing found" right above the subscription blocks themselves. */}
          {tab === "vpn" &&
            sortedVpns.length === 0 &&
            subscriptions.length === 0 && (
              <CommandEmpty>
                {t("createProfile.proxy.notFound", "Ничего не найдено")}
              </CommandEmpty>
            )}

          {tab === "proxies" ? (
            <>
              {allowNone && (
                <CommandGroup>
                  <CommandItem
                    value="__none__"
                    keywords={[
                      t("common.labels.none", "Нет"),
                      t("createProfile.proxy.noProxy", "Без прокси"),
                      "none",
                    ]}
                    onSelect={() => {
                      onSelect(undefined);
                      onClose?.();
                    }}
                  >
                    <LuCheck
                      className={cn(
                        "size-4 shrink-0",
                        !value || value === "__none__"
                          ? "opacity-100"
                          : "opacity-0",
                      )}
                      aria-hidden="true"
                    />
                    {t("common.labels.none", "Без прокси")}
                  </CommandItem>
                </CommandGroup>
              )}

              {/* Proxy Chains */}
              {allowChains && sortedChains.length > 0 && (
                <CommandGroup
                  heading={t("proxyChains.title", "Цепочки прокси (Chains)")}
                >
                  {sortedChains.map((chain) => (
                    <CommandItem
                      key={chain.id}
                      value={chain.id}
                      keywords={[
                        chain.name,
                        "цепь",
                        "цепочка",
                        "chain",
                        "relay",
                      ]}
                      onSelect={() => {
                        onSelect(chain.id, { name: chain.name });
                        onClose?.();
                      }}
                      className="flex items-center gap-2"
                    >
                      <LuCheck
                        className={cn(
                          "size-4 shrink-0",
                          value === chain.id ? "opacity-100" : "opacity-0",
                        )}
                        aria-hidden="true"
                      />
                      <LuLink className="size-3.5 shrink-0 text-primary" />
                      <span className="truncate font-medium flex-1">
                        {chain.name}
                      </span>
                      <Badge
                        variant="secondary"
                        className="ml-auto text-[10px] px-1.5 py-0 font-mono"
                      >
                        Chain
                      </Badge>
                    </CommandItem>
                  ))}
                </CommandGroup>
              )}

              {/* Stored Proxies */}
              <CommandGroup
                heading={t("proxies.management.tabProxies", "Прокси")}
              >
                {sortedProxies.map((proxy) => {
                  const res = getCachedProxyResult(proxy.id);
                  const countryCode =
                    res?.country_code ??
                    extractCountryCode(proxy.geo_country, proxy.name);
                  const latency =
                    res?.is_valid && typeof res.latency_ms === "number"
                      ? res.latency_ms
                      : null;
                  return (
                    <CommandItem
                      key={proxy.id}
                      value={proxy.id}
                      keywords={[
                        proxy.name,
                        proxy.proxy_settings.host,
                        proxy.proxy_settings.proxy_type,
                        countryCode ?? "",
                      ]}
                      onSelect={() => {
                        onSelect(proxy.id, {
                          name: proxy.name,
                          countryCode: countryCode ?? undefined,
                          protocol: proxy.proxy_settings.proxy_type,
                        });
                        onClose?.();
                      }}
                      className="flex items-center gap-2"
                    >
                      <LuCheck
                        className={cn(
                          "size-4 shrink-0",
                          value === proxy.id ? "opacity-100" : "opacity-0",
                        )}
                        aria-hidden="true"
                      />
                      {countryCode ? (
                        <FlagIcon
                          countryCode={countryCode}
                          className="h-3 shrink-0 rounded-[2px]"
                        />
                      ) : (
                        <LuGlobe className="size-3.5 shrink-0 text-muted-foreground/60" />
                      )}
                      <span className="truncate flex-1">{proxy.name}</span>
                      {latency !== null && (
                        <span className="ml-auto shrink-0 font-mono text-[11px] text-muted-foreground">
                          {latency} ms
                        </span>
                      )}
                    </CommandItem>
                  );
                })}
                {onOpenAddProxy && (
                  <CommandItem
                    value="__add_proxy__"
                    onSelect={() => {
                      onClose?.();
                      onOpenAddProxy();
                    }}
                    className="text-primary font-medium flex items-center gap-2"
                  >
                    <LuPlus className="size-4 shrink-0" aria-hidden="true" />
                    {t("createProfile.proxy.addProxy", "Добавить прокси")}
                  </CommandItem>
                )}
              </CommandGroup>
            </>
          ) : (
            <>
              {/* WireGuard VPNs */}
              {sortedVpns.length > 0 && (
                <CommandGroup
                  heading={t("vpnSubscriptions.tabs.wireguard", "WireGuard")}
                >
                  {sortedVpns.map((vpn) => {
                    const res = getCachedVpnResult(vpn.id);
                    const countryCode =
                      res?.country_code ??
                      extractCountryCode(undefined, vpn.name);
                    const latency =
                      res?.is_valid && typeof res.latency_ms === "number"
                        ? res.latency_ms
                        : null;
                    return (
                      <CommandItem
                        key={vpn.id}
                        value={`vpn-${vpn.id}`}
                        keywords={[
                          vpn.name,
                          "VPN",
                          "WireGuard",
                          countryCode ?? "",
                        ]}
                        onSelect={() => {
                          onSelect(`vpn-${vpn.id}`, {
                            name: vpn.name,
                            countryCode: countryCode ?? undefined,
                            protocol: "wireguard",
                          });
                          onClose?.();
                        }}
                        className="flex items-center gap-2"
                      >
                        <LuCheck
                          className={cn(
                            "size-4 shrink-0",
                            value === `vpn-${vpn.id}`
                              ? "opacity-100"
                              : "opacity-0",
                          )}
                          aria-hidden="true"
                        />
                        {countryCode ? (
                          <FlagIcon
                            countryCode={countryCode}
                            className="h-3 shrink-0 rounded-[2px]"
                          />
                        ) : (
                          <Badge
                            variant="soft"
                            className="px-1 py-0 text-[11px] leading-tight"
                          >
                            WG
                          </Badge>
                        )}
                        <span className="truncate flex-1">{vpn.name}</span>
                        {latency !== null && (
                          <span className="ml-auto shrink-0 font-mono text-[11px] text-muted-foreground">
                            {latency} ms
                          </span>
                        )}
                      </CommandItem>
                    );
                  })}
                </CommandGroup>
              )}

              {/* Subscriptions as collapsible accordion blocks */}
              {subscriptions.map((sub) => {
                // The other hop of a chain excludes its node here, mirroring
                // the proxies/chains/WireGuard lists above: the same node must
                // not be selectable as both Relay and Exit.
                const subNodes = (subNodesMap[sub.id] || []).filter(
                  (n) => `subnode-${n.id}` !== excludeId,
                );
                const isExpanded = Boolean(expandedSubs[sub.id]);

                // Extract unique country flags preview
                const uniqueCountries = Array.from(
                  new Set(
                    subNodes
                      .map(
                        (n) =>
                          n.country_code ||
                          extractCountryCode(undefined, n.name),
                      )
                      .filter(Boolean) as string[],
                  ),
                );

                // Always statically sort by ping from lowest to highest
                const sortedNodes = [...subNodes].sort((a, b) => {
                  const latA =
                    a.is_valid && typeof a.latency_ms === "number"
                      ? a.latency_ms
                      : null;
                  const latB =
                    b.is_valid && typeof b.latency_ms === "number"
                      ? b.latency_ms
                      : null;
                  if (latA !== null && latB !== null) {
                    return latA - latB;
                  }
                  if (latA !== null && latB === null) return -1;
                  if (latA === null && latB !== null) return 1;
                  return a.name.localeCompare(b.name);
                });

                return (
                  <div
                    key={sub.id}
                    className="my-1 border-b border-border/40 pb-1 last:border-b-0"
                  >
                    {/* Subscription block header (collapsed by default) */}
                    <button
                      type="button"
                      className="w-full flex items-center justify-between px-2.5 py-1.5 hover:bg-muted/40 cursor-pointer rounded-md select-none text-xs font-medium border border-border/40 bg-muted/20 text-left"
                      onClick={(e) => {
                        e.stopPropagation();
                        toggleExpand(sub.id);
                      }}
                    >
                      <div className="flex items-center gap-1.5 min-w-0 flex-1">
                        {isExpanded ? (
                          <LuChevronDown className="size-3.5 text-primary shrink-0" />
                        ) : (
                          <LuChevronRight className="size-3.5 text-muted-foreground shrink-0" />
                        )}
                        <span className="truncate font-medium">{sub.name}</span>
                        <div className="flex items-center gap-0.5 ml-1 overflow-hidden shrink-0">
                          {uniqueCountries.slice(0, 4).map((code) => (
                            <FlagIcon
                              key={code}
                              countryCode={code}
                              className="h-2.5 rounded-[1px]"
                            />
                          ))}
                        </div>
                      </div>
                      <Badge
                        variant="outline"
                        className="text-[10px] px-1 py-0 text-muted-foreground shrink-0 ml-1"
                      >
                        {subNodes.length}
                      </Badge>
                    </button>

                    {/* Expanded Nodes List */}
                    {isExpanded && (
                      <div className="mt-1 space-y-0.5 pl-2 pr-1">
                        {sortedNodes.map((node) => {
                          const countryCode =
                            node.country_code ??
                            extractCountryCode(undefined, node.name);
                          const isPinging = pingingNodeId === node.id;
                          const isSelected = value === `subnode-${node.id}`;

                          return (
                            <CommandItem
                              key={node.id}
                              value={`subnode-${node.id}`}
                              keywords={[
                                node.name,
                                node.protocol,
                                sub.name,
                                countryCode ?? "",
                              ]}
                              onSelect={() => {
                                onSelect(`subnode-${node.id}`, {
                                  name: node.name,
                                  countryCode: countryCode ?? undefined,
                                  protocol: node.protocol,
                                });
                                onClose?.();
                              }}
                              className="flex items-center gap-2 pl-3 pr-2 py-1 text-xs"
                            >
                              <LuCheck
                                className={cn(
                                  "size-3.5 shrink-0",
                                  isSelected
                                    ? "opacity-100 text-primary"
                                    : "opacity-0",
                                )}
                                aria-hidden="true"
                              />
                              {countryCode ? (
                                <FlagIcon
                                  countryCode={countryCode}
                                  className="h-3 shrink-0 rounded-[2px]"
                                />
                              ) : (
                                <LuGlobe className="size-3.5 shrink-0 text-muted-foreground/60" />
                              )}
                              <span className="truncate flex-1">
                                {node.name}
                              </span>
                              <Badge
                                variant="secondary"
                                className="text-[9px] uppercase font-mono px-1 py-0 shrink-0"
                              >
                                {node.protocol}
                              </Badge>
                              {node.latency_ms !== null &&
                              node.latency_ms !== undefined ? (
                                <span
                                  className={cn(
                                    "shrink-0 font-mono text-[11px]",
                                    node.is_valid
                                      ? "text-emerald-500 font-medium"
                                      : "text-destructive",
                                  )}
                                >
                                  {node.latency_ms} ms
                                </span>
                              ) : (
                                <span className="shrink-0 font-mono text-[11px] text-muted-foreground/50">
                                  —
                                </span>
                              )}
                              <button
                                type="button"
                                title={t(
                                  "vpnSubscriptions.ping",
                                  "Проверить пинг",
                                )}
                                className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground shrink-0 cursor-pointer transition-colors"
                                onClick={async (e) => {
                                  e.stopPropagation();
                                  e.preventDefault();
                                  try {
                                    setPingingNodeId(node.id);
                                    await pingSingleNode(
                                      node.id,
                                      node.subscription_id,
                                    );
                                  } catch (err) {
                                    console.error("Ping error:", err);
                                  } finally {
                                    setPingingNodeId(null);
                                  }
                                }}
                              >
                                <LuActivity
                                  className={cn(
                                    "size-3.5",
                                    isPinging && "animate-spin text-primary",
                                  )}
                                />
                              </button>
                            </CommandItem>
                          );
                        })}
                      </div>
                    )}
                  </div>
                );
              })}
            </>
          )}
        </CommandList>
      </Command>
    </div>
  );
}

export interface ProxyVpnSelectorProps {
  value?: string;
  onSelect: (
    id: string | undefined,
    option?: { name: string; countryCode?: string; protocol?: string },
  ) => void;
  allowNone?: boolean;
  allowChains?: boolean;
  excludeId?: string;
  placeholder?: string;
  disabled?: boolean;
  className?: string;
  onOpenAddProxy?: () => void;
}

export function ProxyVpnSelector({
  value,
  onSelect,
  allowNone = true,
  allowChains = false,
  excludeId,
  placeholder,
  disabled = false,
  className,
  onOpenAddProxy,
}: ProxyVpnSelectorProps) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);

  const { storedProxies } = useProxyEvents();
  const { vpnConfigs } = useVpnEvents();
  const { chains } = useProxyChains();
  const { nodes: subNodesMap } = useVpnSubscriptions();

  const allSubNodes = useMemo(() => {
    return Object.values(subNodesMap).flat();
  }, [subNodesMap]);

  // Resolve currently selected details for button trigger
  const selectedDetails = useMemo(() => {
    if (!value || value === "__none__") return null;

    if (value.startsWith("chain-")) {
      const chain = chains.find((c) => c.id === value);
      if (chain) {
        return {
          name: `Цепочка — ${chain.name}`,
          isChain: true,
        };
      }
    }

    if (value.startsWith("vpn-")) {
      const vpnId = value.slice(4);
      const vpn = vpnConfigs.find((v) => v.id === vpnId);
      if (vpn) {
        const cached = getCachedVpnResult(vpn.id);
        const code =
          cached?.country_code ?? extractCountryCode(undefined, vpn.name);
        return {
          name: `WG — ${vpn.name}`,
          countryCode: code,
          isWg: true,
        };
      }
    }

    if (value.startsWith("subnode-")) {
      const nodeId = value.slice(8);
      const node = allSubNodes.find((n) => n.id === nodeId);
      if (node) {
        const code =
          node.country_code ?? extractCountryCode(undefined, node.name);
        return {
          name: `VPN — ${node.name}`,
          countryCode: code,
          protocol: node.protocol,
        };
      }
    }

    const proxy = storedProxies.find((p) => p.id === value);
    if (proxy) {
      const cached = getCachedProxyResult(proxy.id);
      const code =
        cached?.country_code ??
        extractCountryCode(proxy.geo_country, proxy.name) ??
        undefined;
      return {
        name: proxy.name,
        countryCode: code,
        protocol: proxy.proxy_settings.proxy_type,
      };
    }

    return null;
  }, [value, chains, vpnConfigs, allSubNodes, storedProxies]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          disabled={disabled}
          className={cn(
            "flex h-9 w-full items-center justify-between rounded-md border border-input bg-transparent px-3 py-1 text-xs shadow-xs transition-colors hover:bg-accent/50 focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
            className,
          )}
        >
          <div className="flex items-center gap-2 truncate">
            {selectedDetails ? (
              <>
                {selectedDetails.isChain ? (
                  <LuLink className="size-3.5 text-primary shrink-0" />
                ) : selectedDetails.countryCode ? (
                  <FlagIcon
                    countryCode={selectedDetails.countryCode}
                    className="size-3.5 shrink-0 rounded-[2px]"
                  />
                ) : selectedDetails.isWg ? (
                  <Badge
                    variant="soft"
                    className="px-1 py-0 text-[10px] leading-tight"
                  >
                    WG
                  </Badge>
                ) : (
                  <LuGlobe className="size-3.5 shrink-0 text-muted-foreground" />
                )}
                <span className="truncate font-medium">
                  {selectedDetails.name}
                </span>
              </>
            ) : (
              <span className="text-muted-foreground">
                {placeholder ??
                  (allowNone
                    ? t("createProfile.proxy.noProxy", "Без прокси / VPN")
                    : t(
                        "proxyChains.dialog.selectHop1Placeholder",
                        "Выберите прокси / узел...",
                      ))}
              </span>
            )}
          </div>
          <LuChevronDown className="size-3.5 opacity-60 shrink-0 ml-2" />
        </button>
      </PopoverTrigger>

      <PopoverContent className="w-88 p-0" align="start" sideOffset={6}>
        <ProxyVpnSelectorContent
          value={value}
          onSelect={onSelect}
          allowNone={allowNone}
          allowChains={allowChains}
          excludeId={excludeId}
          onClose={() => setOpen(false)}
          onOpenAddProxy={onOpenAddProxy}
        />
      </PopoverContent>
    </Popover>
  );
}
