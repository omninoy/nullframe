import { invoke } from "@tauri-apps/api/core";
import type { TFunction } from "i18next";
import { translateBackendError } from "@/lib/backend-errors";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import type { StoredProxy } from "@/types";

/** `RotationResult` in `src-tauri/src/proxy_rotation.rs`. */
interface RotationResult {
  proxy_id: string;
  status: number | null;
  elapsed_ms: number;
  error: string | null;
}

/** `RotationReport` in `src-tauri/src/proxy_rotation.rs`. */
interface RotationReport {
  proxy_id: string;
  /** The exit as it stood before the change link was called, when seen. */
  old_ip: string | null;
  /** The exit seen after the rotation, when seen. */
  new_ip: string | null;
  rotated_at: number;
  /** False when the provider answered but the exit came back at the old address. */
  success: boolean;
}

/** A link worth sending to the backend, which checks it again. */
export function looksLikeRotationUrl(value: string): boolean {
  return /^https?:\/\/\S+$/i.test(value.trim());
}

/**
 * Ask the providers of these proxies for new IPs and say what happened. Proxies
 * without a change-IP link are left out.
 */
export async function rotateProxyIps(
  proxies: StoredProxy[],
  t: TFunction,
): Promise<void> {
  const rotatable = proxies.filter((proxy) => proxy.rotation_url);
  if (rotatable.length === 0) {
    showErrorToast(t("proxies.rotation.noneSelected"));
    return;
  }
  if (rotatable.length === 1) {
    const [proxy] = rotatable;
    try {
      const report = await invoke<RotationReport>("rotate_proxy_ip", {
        proxyId: proxy.id,
      });
      if (report.success) {
        showSuccessToast(t("proxies.rotation.requested", { name: proxy.name }));
      } else {
        showErrorToast(t("proxies.rotation.unchanged", { name: proxy.name }));
      }
    } catch (error) {
      showErrorToast(
        t("proxies.rotation.failedFor", {
          name: proxy.name,
          error: translateBackendError(t, error),
        }),
      );
    }
    return;
  }
  const results = await invoke<RotationResult[]>("rotate_proxy_ips", {
    proxyIds: rotatable.map((proxy) => proxy.id),
  });
  const succeeded = results.filter((result) => !result.error).length;
  if (succeeded > 0) {
    showSuccessToast(t("proxies.rotation.requestedMany", { count: succeeded }));
  }
  for (const result of results) {
    if (!result.error) continue;
    const name =
      rotatable.find((proxy) => proxy.id === result.proxy_id)?.name ??
      result.proxy_id;
    showErrorToast(
      t("proxies.rotation.failedFor", {
        name,
        error: translateBackendError(t, result.error),
      }),
    );
  }
}
