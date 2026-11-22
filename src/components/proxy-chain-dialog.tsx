"use client";

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { LuArrowRight, LuGlobe, LuLink, LuLoaderCircle } from "react-icons/lu";
import { FlagIcon } from "@/components/flag-icon";
import { ProxyVpnSelector } from "@/components/proxy-vpn-selector";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { getCachedVpnResult } from "@/components/vpn-check-button";
import { useProxyChains } from "@/hooks/use-proxy-chains";
import { useProxyEvents } from "@/hooks/use-proxy-events";
import { useVpnEvents } from "@/hooks/use-vpn-events";
import { useVpnSubscriptions } from "@/hooks/use-vpn-subscriptions";
import { extractCountryCode } from "@/lib/flag-utils";
import { getCachedProxyResult } from "@/lib/proxy-check-store";
import type { ProxyChain } from "@/types/proxy-chain";

interface ProxyChainDialogProps {
  isOpen: boolean;
  onClose: () => void;
  chain?: ProxyChain | null;
  onSuccess?: (chain: ProxyChain) => void;
}

export function ProxyChainDialog({
  isOpen,
  onClose,
  chain,
  onSuccess,
}: ProxyChainDialogProps) {
  const { t } = useTranslation();
  const { addChain, updateChain } = useProxyChains();
  const { storedProxies } = useProxyEvents();
  const { vpnConfigs } = useVpnEvents();
  const { nodes: subscriptionNodes } = useVpnSubscriptions();

  const [name, setName] = useState("");
  const [hop1Id, setHop1Id] = useState("");
  const [hop2Id, setHop2Id] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isEdit = Boolean(chain);

  useEffect(() => {
    if (!isOpen) return;
    if (chain) {
      setName(chain.name);
      setHop1Id(chain.hop1_proxy_id);
      setHop2Id(chain.hop2_proxy_id);
    } else {
      setName("");
      setHop1Id("");
      setHop2Id("");
    }
    setError(null);
  }, [isOpen, chain]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      setError(
        t("proxyChains.dialog.errors.nameRequired", "Укажите название цепочки"),
      );
      return;
    }
    if (!hop1Id) {
      setError(
        t(
          "proxyChains.dialog.errors.hop1Required",
          "Выберите входной прокси (Хоп 1 / Relay)",
        ),
      );
      return;
    }
    if (!hop2Id) {
      setError(
        t(
          "proxyChains.dialog.errors.hop2Required",
          "Выберите выходной прокси (Хоп 2 / Exit)",
        ),
      );
      return;
    }
    if (hop1Id === hop2Id) {
      setError(
        t(
          "proxyChains.dialog.errors.sameHop",
          "Входной и выходной прокси должны различаться",
        ),
      );
      return;
    }
    try {
      setIsSubmitting(true);
      setError(null);
      let result: ProxyChain;
      if (isEdit && chain) {
        result = await updateChain(chain.id, {
          name,
          hop1_proxy_id: hop1Id,
          hop2_proxy_id: hop2Id,
        });
      } else {
        result = await addChain(name, hop1Id, hop2Id);
      }
      onSuccess?.(result);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  const getHopDetails = (id: string) => {
    if (!id) return null;
    if (id.startsWith("vpn-")) {
      const vpnId = id.slice(4);
      const vpn = vpnConfigs.find((v) => v.id === vpnId);
      if (vpn) {
        return {
          name: `WG — ${vpn.name}`,
          countryCode:
            getCachedVpnResult(vpn.id)?.country_code ??
            extractCountryCode(undefined, vpn.name) ??
            undefined,
          protocol: "wireguard",
        };
      }
    }
    if (id.startsWith("subnode-")) {
      const nodeId = id.slice(8);
      for (const nodes of Object.values(subscriptionNodes)) {
        const found = nodes.find((n) => n.id === nodeId);
        if (found) {
          return {
            name: found.name,
            countryCode: found.country_code,
            protocol: found.protocol,
          };
        }
      }
    }
    const proxy = storedProxies.find((p) => p.id === id);
    if (proxy) {
      const res = getCachedProxyResult(proxy.id);
      return {
        name: proxy.name,
        countryCode:
          res?.country_code ??
          extractCountryCode(proxy.geo_country, proxy.name) ??
          undefined,
        protocol: proxy.proxy_settings.proxy_type,
      };
    }
    return {
      name: id,
    };
  };

  const selectedHop1 = getHopDetails(hop1Id);
  const selectedHop2 = getHopDetails(hop2Id);

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2">
            <LuLink className="size-5 text-primary" />
            {isEdit
              ? t("proxyChains.dialog.titleEdit", "Редактировать цепочку")
              : t("proxyChains.dialog.titleAdd", "Создать цепочку прокси")}
          </DialogTitle>
          <DialogDescription>
            {t(
              "proxyChains.dialog.description",
              "Трафик сначала отправляется на промежуточный прокси (Relay), а затем выходит в интернет через конечный узел (Exit).",
            )}
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleSubmit} className="space-y-4 pt-1">
          {error && (
            <div className="p-3 text-sm text-destructive bg-destructive/10 rounded-lg border border-destructive/20">
              {error}
            </div>
          )}

          <div className="space-y-1.5">
            <Label htmlFor="chain-name">
              {t("proxyChains.dialog.nameLabel", "Название цепочки")}
            </Label>
            <Input
              id="chain-name"
              placeholder="Chain: UK -> NL / Private Tunnel"
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={isSubmitting}
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="chain-hop1">
              {t(
                "proxyChains.dialog.hop1Label",
                "Хоп 1: Входной узел (Entry / Relay)",
              )}
            </Label>
            <ProxyVpnSelector
              value={hop1Id}
              onSelect={(id) => setHop1Id(id ?? "")}
              allowNone={false}
              allowChains={false}
              excludeId={hop2Id}
              disabled={isSubmitting}
              placeholder={t(
                "proxyChains.dialog.selectHop1Placeholder",
                "Выберите промежуточный прокси / узел...",
              )}
            />
          </div>

          <div className="flex justify-center items-center py-0.5">
            <div className="flex items-center gap-2 px-3 py-1 rounded-full bg-muted/60 text-xs font-mono text-muted-foreground border">
              <span>Хоп 1</span>
              <LuArrowRight className="size-3.5 text-primary" />
              <span>Хоп 2 (Выход)</span>
            </div>
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="chain-hop2">
              {t(
                "proxyChains.dialog.hop2Label",
                "Хоп 2: Конечный узел (Exit Node)",
              )}
            </Label>
            <ProxyVpnSelector
              value={hop2Id}
              onSelect={(id) => setHop2Id(id ?? "")}
              allowNone={false}
              allowChains={false}
              excludeId={hop1Id}
              disabled={isSubmitting}
              placeholder={t(
                "proxyChains.dialog.selectHop2Placeholder",
                "Выберите конечный прокси / узел...",
              )}
            />
          </div>

          {selectedHop1 && selectedHop2 && hop1Id && hop2Id && (
            <div className="p-3 rounded-lg border bg-muted/20 flex items-center justify-between text-xs">
              <div className="flex items-center gap-1.5 min-w-0">
                {selectedHop1.countryCode ? (
                  <FlagIcon
                    countryCode={selectedHop1.countryCode}
                    squared
                    className="size-3.5 rounded-[2px]"
                  />
                ) : (
                  <LuGlobe className="size-3.5 text-muted-foreground" />
                )}
                <span className="truncate font-medium">
                  {selectedHop1.name}
                </span>
              </div>

              <LuArrowRight className="size-4 shrink-0 mx-2 text-primary" />

              <div className="flex items-center gap-1.5 min-w-0">
                {selectedHop2.countryCode ? (
                  <FlagIcon
                    countryCode={selectedHop2.countryCode}
                    squared
                    className="size-3.5 rounded-[2px]"
                  />
                ) : (
                  <LuGlobe className="size-3.5 text-muted-foreground" />
                )}
                <span className="truncate font-medium">
                  {selectedHop2.name}
                </span>
              </div>
            </div>
          )}

          <DialogFooter className="pt-2">
            <Button
              type="button"
              variant="outline"
              onClick={onClose}
              disabled={isSubmitting}
            >
              {t("common.buttons.cancel", "Отмена")}
            </Button>
            <Button type="submit" disabled={isSubmitting}>
              {isSubmitting ? (
                <>
                  <LuLoaderCircle className="size-4 mr-2 animate-spin" />
                  {t("common.buttons.saving", "Сохранение...")}
                </>
              ) : isEdit ? (
                t("common.buttons.save", "Сохранить")
              ) : (
                t("common.buttons.create", "Создать")
              )}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
