"use client";

import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import { proxyChainList } from "@/lib/vpn-subscription-store";
import type { ProxyChain } from "@/types/proxy-chain";

const EMPTY_CHAINS: ProxyChain[] = [];

export function useProxyChains() {
  const [chains, setChains] = useState<ProxyChain[]>(
    () => proxyChainList.peek() ?? EMPTY_CHAINS,
  );
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    let disposed = false;
    const stop = proxyChainList.subscribe({
      value: (list) => {
        if (!disposed) setChains(list);
      },
      error: (err) => console.error("Failed to load proxy chains:", err),
    });

    void (async () => {
      try {
        setLoading(true);
        const list = await proxyChainList.load();
        if (!disposed) setChains(list);
      } catch (err) {
        console.error("Failed initial load of proxy chains:", err);
      } finally {
        if (!disposed) setLoading(false);
      }
    })();

    return () => {
      disposed = true;
      stop();
    };
  }, []);

  const addChain = useCallback(
    async (name: string, hop1_proxy_id: string, hop2_proxy_id: string) => {
      try {
        const newChain = await invoke<ProxyChain>("add_proxy_chain", {
          name,
          hop1ProxyId: hop1_proxy_id,
          hop2ProxyId: hop2_proxy_id,
        });
        showSuccessToast(`Цепочка "${newChain.name}" успешно создана`);
        try {
          await emit("proxy-chains-changed", {});
        } catch {}
        await proxyChainList.refresh();
        return newChain;
      } catch (error) {
        showErrorToast(String(error));
        throw error;
      }
    },
    [],
  );

  const updateChain = useCallback(
    async (
      id: string,
      options?: {
        name?: string;
        hop1_proxy_id?: string;
        hop2_proxy_id?: string;
      },
    ) => {
      try {
        const updated = await invoke<ProxyChain>("update_proxy_chain", {
          chainId: id,
          name: options?.name ?? null,
          hop1ProxyId: options?.hop1_proxy_id ?? null,
          hop2ProxyId: options?.hop2_proxy_id ?? null,
        });
        showSuccessToast(`Цепочка "${updated.name}" обновлена`);
        try {
          await emit("proxy-chains-changed", {});
        } catch {}
        await proxyChainList.refresh();
        return updated;
      } catch (error) {
        showErrorToast(String(error));
        throw error;
      }
    },
    [],
  );

  const deleteChain = useCallback(async (id: string) => {
    try {
      await invoke("delete_proxy_chain", { chainId: id });
      showSuccessToast("Цепочка удалена");
      try {
        await emit("proxy-chains-changed", {});
      } catch {}
      await proxyChainList.refresh();
    } catch (error) {
      showErrorToast(String(error));
      throw error;
    }
  }, []);

  const [pingingChainId, setPingingChainId] = useState<string | null>(null);

  const pingChain = useCallback(async (id: string) => {
    setPingingChainId(id);
    try {
      const result = await invoke<{
        chain: ProxyChain;
        latency_ms: number | null;
        is_valid: boolean;
        error: string | null;
      }>("ping_proxy_chain", { chainId: id });
      try {
        await emit("proxy-chains-changed", {});
      } catch {}
      await proxyChainList.refresh();
      if (result.is_valid) {
        showSuccessToast(`Цепочка доступна: ${result.latency_ms} ms`);
      } else {
        showErrorToast(result.error || "Цепочка недоступна");
      }
      return result;
    } catch (error) {
      showErrorToast(String(error));
      throw error;
    } finally {
      setPingingChainId(null);
    }
  }, []);

  return {
    chains,
    loading,
    addChain,
    updateChain,
    deleteChain,
    pingChain,
    pingingChainId,
  };
}
