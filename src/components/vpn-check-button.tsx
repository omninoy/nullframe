"use client";

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import * as React from "react";
import { useTranslation } from "react-i18next";
import { LuCheck, LuCircleDashed, LuShieldCheck, LuX } from "react-icons/lu";
import {
  CheckPill,
  CheckSpinner,
  type CheckState,
} from "@/components/entity-page";
import { FlagIcon } from "@/components/flag-icon";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { translateBackendError } from "@/lib/backend-errors";
import { extractCountryCode, formatRelativeTime } from "@/lib/flag-utils";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import type { ProxyCheckResult, VpnConfig } from "@/types";

interface VpnCheckEntry {
  /** The config the result belongs to; any other config shows no result. */
  routeKey: string;
  checking: boolean;
  result: ProxyCheckResult | null;
}

const IDLE: VpnCheckEntry = { routeKey: "", checking: false, result: null };

/** The last check of each VPN this session, so a row that remounts keeps it. */
const entries = new Map<string, VpnCheckEntry>();
const listeners = new Set<() => void>();
const anyVpnListeners = new Set<() => void>();
let vpnVersion = 0;
let watchingVpnSaves = false;

let vpnInitialBatchLoaded = false;
let vpnInitialBatchPromise: Promise<void> | null = null;

export async function loadAllCachedVpnChecks(): Promise<void> {
  if (vpnInitialBatchLoaded) return;
  if (vpnInitialBatchPromise) return vpnInitialBatchPromise;
  vpnInitialBatchPromise = invoke<Record<string, ProxyCheckResult>>(
    "get_all_cached_vpn_checks",
  )
    .then((map) => {
      vpnInitialBatchLoaded = true;
      let changed = false;
      for (const [vpnId, result] of Object.entries(map)) {
        const current = entries.get(vpnId);
        if (!current?.result) {
          entries.set(vpnId, {
            routeKey: "",
            checking: false,
            result,
          });
          changed = true;
        }
      }
      if (changed) {
        vpnVersion += 1;
        for (const listener of anyVpnListeners) listener();
        for (const listener of listeners) listener();
      }
    })
    .catch(() => {
      vpnInitialBatchLoaded = true;
    });
  return vpnInitialBatchPromise;
}

function watchSavedVpnChecks() {
  if (watchingVpnSaves) return;
  watchingVpnSaves = true;
  void loadAllCachedVpnChecks();
  listen<string>("vpn-check-saved", (event) => {
    const vpnId = event.payload;
    invoke<ProxyCheckResult | null>("get_cached_vpn_check", { vpnId })
      .then((result) => {
        if (!result) return;
        const current = entries.get(vpnId);
        if (current?.checking) return;
        publish(vpnId, {
          routeKey: current?.routeKey ?? "",
          checking: false,
          result,
        });
      })
      .catch(() => {});
  }).catch(() => {
    watchingVpnSaves = false;
  });
}

/** An edit, a synced download or an API update bumps `updated_at`, so a
 * result from before it no longer describes the tunnel. */
export function vpnRouteKey(vpn: VpnConfig): string {
  return JSON.stringify([
    vpn.id,
    vpn.vpn_type,
    vpn.updated_at ?? null,
    vpn.config_data,
  ]);
}

function publish(vpnId: string, entry: VpnCheckEntry) {
  entries.set(vpnId, entry);
  vpnVersion += 1;
  for (const listener of listeners) listener();
  for (const listener of anyVpnListeners) listener();
}

function subscribe(listener: () => void) {
  watchSavedVpnChecks();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getCachedVpnStatus(
  vpnId: string,
): "valid" | "invalid" | "untested" {
  const entry = entries.get(vpnId);
  if (!entry?.result) return "untested";
  return entry.result.is_valid ? "valid" : "invalid";
}

export function getCachedVpnResult(vpnId: string): ProxyCheckResult | null {
  return entries.get(vpnId)?.result ?? null;
}

export function useVpnCheckVersion(): number {
  return React.useSyncExternalStore(
    (cb) => {
      watchSavedVpnChecks();
      anyVpnListeners.add(cb);
      return () => {
        anyVpnListeners.delete(cb);
      };
    },
    () => vpnVersion,
    () => vpnVersion,
  );
}

export async function runVpnCheck(
  vpn: VpnConfig,
  _translateFailure?: (error: unknown) => string,
): Promise<ProxyCheckResult | null> {
  const vpnId = vpn.id;
  const routeKey = vpnRouteKey(vpn);
  const current = entries.get(vpnId);
  const before =
    current?.routeKey === routeKey ? current : { ...IDLE, routeKey };
  if (before.checking) return null;
  publish(vpnId, { ...before, checking: true });
  try {
    const checked = await invoke<ProxyCheckResult>("check_vpn_validity", {
      vpnId,
    });
    publish(vpnId, { routeKey, checking: false, result: checked });
    return checked;
  } catch (error) {
    publish(vpnId, {
      routeKey,
      checking: false,
      result: {
        ip: "",
        timestamp: Math.floor(Date.now() / 1000),
        is_valid: false,
      },
    });
    throw error;
  }
}

interface VpnCheckButtonProps {
  vpn: VpnConfig;
  disabled?: boolean;
}

export function VpnCheckButton({ vpn, disabled = false }: VpnCheckButtonProps) {
  const { t } = useTranslation();
  const vpnId = vpn.id;
  const vpnName = vpn.name;
  const routeKey = vpnRouteKey(vpn);
  const getSnapshot = React.useCallback(
    () => entries.get(vpnId) ?? IDLE,
    [vpnId],
  );
  const raw = React.useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
  const { checking, result } =
    raw.routeKey === routeKey || (!raw.routeKey && raw.result) ? raw : IDLE;

  const handleCheck = React.useCallback(async () => {
    try {
      const checked = await runVpnCheck(vpn);
      if (checked?.is_valid) {
        showSuccessToast(t("vpnCheck.valid", { name: vpnName }));
      } else {
        showErrorToast(t("vpnCheck.invalid", { name: vpnName }));
      }
    } catch (error) {
      showErrorToast(
        t("vpnCheck.failed", { error: translateBackendError(t, error) }),
      );
    }
  }, [vpn, vpnName, t]);

  const state: CheckState = checking
    ? "checking"
    : !result
      ? "idle"
      : result.is_valid
        ? "ok"
        : "failed";

  const countryCode =
    result?.country_code ?? extractCountryCode(undefined, vpn.name);

  const leftIcon = countryCode ? (
    <FlagIcon
      countryCode={countryCode}
      className="h-2.5 shrink-0 rounded-[2px]"
    />
  ) : (
    <LuShieldCheck className="size-3 text-primary shrink-0" />
  );

  const rightIcon =
    state === "checking" ? (
      <CheckSpinner />
    ) : state === "ok" ? (
      <LuCheck className="text-success-text size-3 shrink-0" />
    ) : state === "failed" ? (
      <LuX className="text-destructive-text size-3 shrink-0" />
    ) : (
      <LuCircleDashed className="size-3 text-muted-foreground/60 shrink-0" />
    );

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <CheckPill
          state={state}
          resultKey={result?.timestamp}
          onClick={() => void handleCheck()}
          disabled={disabled}
          aria-busy={checking}
          aria-label={t(
            checking ? "vpnCheck.tooltipChecking" : "vpnCheck.tooltipDefault",
          )}
          icon={leftIcon}
          suffixIcon={rightIcon}
        >
          {state === "checking"
            ? t("appFeedback.checking")
            : state === "ok"
              ? typeof result?.latency_ms === "number"
                ? t("proxyCheck.latencyValue", { ms: result.latency_ms })
                : t("entities.check.valid")
              : state === "failed"
                ? t("entities.check.invalid")
                : t("entities.check.notChecked")}
        </CheckPill>
      </TooltipTrigger>
      <TooltipContent>
        {checking ? (
          <p>{t("vpnCheck.tooltipChecking")}</p>
        ) : result ? (
          <div className="space-y-1">
            <p>
              {t(
                result.is_valid
                  ? "vpnCheck.tooltipValid"
                  : "vpnCheck.tooltipInvalid",
              )}
            </p>
            {result.ip && (
              <p className="font-mono text-xs text-primary-foreground/90">
                {result.ip}
                {result.country ? ` (${result.country})` : ""}
              </p>
            )}
            {typeof result.latency_ms === "number" && (
              <p className="text-xs text-primary-foreground/90">
                {t("proxyCheck.latencyLabel")}:{" "}
                <span className="font-mono font-medium">
                  {t("proxyCheck.latencyValue", { ms: result.latency_ms })}
                </span>
              </p>
            )}
            <p className="text-primary-foreground/75">
              {t("vpnCheck.tooltipChecked", {
                time: formatRelativeTime(result.timestamp),
              })}
            </p>
          </div>
        ) : (
          <p>{t("vpnCheck.tooltipDefault")}</p>
        )}
      </TooltipContent>
    </Tooltip>
  );
}
