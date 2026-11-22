"use client";

import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { LuGlobe, LuLink, LuLoaderCircle, LuShieldCheck } from "react-icons/lu";
import { AnimatedSwitch } from "@/components/ui/animated-switch";
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
import { useVpnSubscriptions } from "@/hooks/use-vpn-subscriptions";
import type { VpnSubscription } from "@/types/vpn-subscription";

interface VpnSubscriptionDialogProps {
  isOpen: boolean;
  onClose: () => void;
  subscription?: VpnSubscription | null;
  onSuccess?: (sub: VpnSubscription) => void;
}

export function VpnSubscriptionDialog({
  isOpen,
  onClose,
  subscription,
  onSuccess,
}: VpnSubscriptionDialogProps) {
  const { t } = useTranslation();
  const { addSubscription, updateSubscription } = useVpnSubscriptions();

  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [autoUpdate, setAutoUpdate] = useState(true);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const isEdit = Boolean(subscription);

  useEffect(() => {
    if (!isOpen) return;
    if (subscription) {
      setName(subscription.name);
      setUrl(subscription.url);
      setAutoUpdate(subscription.auto_update);
    } else {
      setName("");
      setUrl("");
      setAutoUpdate(true);
    }
    setError(null);
  }, [subscription, isOpen]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const trimmedName = name.trim();
    const trimmedUrl = url.trim();

    if (!trimmedName) {
      setError(
        t("vpnSubscriptions.errors.nameRequired", "Укажите название подписки"),
      );
      return;
    }
    if (!trimmedUrl) {
      setError(
        t("vpnSubscriptions.errors.urlRequired", "Укажите URL подписки"),
      );
      return;
    }
    if (
      !trimmedUrl.startsWith("http://") &&
      !trimmedUrl.startsWith("https://")
    ) {
      setError(
        t(
          "vpnSubscriptions.errors.invalidUrl",
          "URL должен начинаться с http:// или https://",
        ),
      );
      return;
    }

    try {
      setIsSubmitting(true);
      setError(null);

      let result: VpnSubscription;
      if (isEdit && subscription) {
        result = await updateSubscription(subscription.id, {
          name: trimmedName,
          url: trimmedUrl,
          autoUpdate,
          fetchNow: trimmedUrl !== subscription.url,
        });
      } else {
        result = await addSubscription(trimmedName, trimmedUrl, autoUpdate);
      }

      onSuccess?.(result);
      onClose();
    } catch (err) {
      setError(String(err));
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => !open && !isSubmitting && onClose()}
    >
      <DialogContent className="sm:max-w-[480px]">
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <div className="flex items-center gap-2">
              <div className="p-2 bg-primary/10 text-primary rounded-lg">
                <LuShieldCheck className="w-5 h-5" />
              </div>
              <div>
                <DialogTitle>
                  {isEdit
                    ? t(
                        "vpnSubscriptions.dialog.editTitle",
                        "Редактировать подписку",
                      )
                    : t(
                        "vpnSubscriptions.dialog.addTitle",
                        "Добавить подписку VPN",
                      )}
                </DialogTitle>
                <DialogDescription>
                  {t(
                    "vpnSubscriptions.dialog.desc",
                    "Поддерживаются ссылки Base64, список URI (vless, vmess, trojan, hysteria2, ss, wg) и Clash YAML",
                  )}
                </DialogDescription>
              </div>
            </div>
          </DialogHeader>

          <div className="space-y-4 py-4">
            {error && (
              <div className="p-3 text-xs text-destructive bg-destructive/10 border border-destructive/20 rounded-md">
                {error}
              </div>
            )}

            <div className="space-y-2">
              <Label htmlFor="sub-name" className="text-xs font-medium">
                {t("vpnSubscriptions.dialog.nameLabel", "Название подписки")}
              </Label>
              <div className="relative">
                <LuGlobe className="absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
                <Input
                  id="sub-name"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="FastVPN Global / NekoBox / Incy"
                  className="pl-9 text-sm"
                  disabled={isSubmitting}
                  autoFocus
                />
              </div>
            </div>

            <div className="space-y-2">
              <Label htmlFor="sub-url" className="text-xs font-medium">
                {t("vpnSubscriptions.dialog.urlLabel", "URL подписки")}
              </Label>
              <div className="relative">
                <LuLink className="absolute left-3 top-2.5 h-4 w-4 text-muted-foreground" />
                <Input
                  id="sub-url"
                  value={url}
                  onChange={(e) => setUrl(e.target.value)}
                  placeholder="https://provider.example.com/sub/token123"
                  className="pl-9 text-sm"
                  disabled={isSubmitting}
                />
              </div>
              <p className="text-[11px] text-muted-foreground">
                {t(
                  "vpnSubscriptions.dialog.urlHint",
                  "При сохранении узлы будут автоматически загружены и проверены.",
                )}
              </p>
            </div>

            <div className="flex items-center justify-between p-3 border rounded-lg bg-muted/20">
              <div className="space-y-0.5">
                <Label
                  htmlFor="sub-auto-update"
                  className="text-xs font-medium cursor-pointer"
                >
                  {t(
                    "vpnSubscriptions.dialog.autoUpdate",
                    "Автообновление подписки",
                  )}
                </Label>
                <p className="text-[11px] text-muted-foreground">
                  {t(
                    "vpnSubscriptions.dialog.autoUpdateHint",
                    "Периодически проверять наличие новых узлов и удалять устаревшие",
                  )}
                </p>
              </div>
              <AnimatedSwitch
                id="sub-auto-update"
                checked={autoUpdate}
                onCheckedChange={setAutoUpdate}
                disabled={isSubmitting}
              />
            </div>
          </div>

          <DialogFooter className="gap-2 sm:gap-0">
            <Button
              type="button"
              variant="outline"
              onClick={onClose}
              disabled={isSubmitting}
              className="text-xs"
            >
              {t("common.buttons.cancel", "Отмена")}
            </Button>
            <Button
              type="submit"
              disabled={isSubmitting}
              className="text-xs min-w-[100px]"
            >
              {isSubmitting ? (
                <>
                  <LuLoaderCircle className="w-3.5 h-3.5 mr-1.5 animate-spin" />
                  {t("common.buttons.loading", "Загрузка...")}
                </>
              ) : isEdit ? (
                t("common.buttons.save", "Сохранить")
              ) : (
                t("vpnSubscriptions.dialog.addButton", "Добавить")
              )}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
