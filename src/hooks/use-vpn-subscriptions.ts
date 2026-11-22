"use client";

import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import {
  vpnSubscriptionList,
  vpnSubscriptionNodesList,
} from "@/lib/vpn-subscription-store";
import type {
  VpnSubscription,
  VpnSubscriptionNode,
} from "@/types/vpn-subscription";

const EMPTY_SUBS: VpnSubscription[] = [];
const EMPTY_NODES: Record<string, VpnSubscriptionNode[]> = {};

function groupNodes(
  allNodes: VpnSubscriptionNode[],
): Record<string, VpnSubscriptionNode[]> {
  const grouped: Record<string, VpnSubscriptionNode[]> = {};
  for (const node of allNodes) {
    if (!grouped[node.subscription_id]) {
      grouped[node.subscription_id] = [];
    }
    grouped[node.subscription_id].push(node);
  }
  return grouped;
}

export function useVpnSubscriptions() {
  const [subscriptions, setSubscriptions] = useState<VpnSubscription[]>(
    () => vpnSubscriptionList.peek() ?? EMPTY_SUBS,
  );
  const [nodes, setNodes] = useState<Record<string, VpnSubscriptionNode[]>>(
    () => {
      const peeked = vpnSubscriptionNodesList.peek();
      return peeked ? groupNodes(peeked) : EMPTY_NODES;
    },
  );
  const [loading, setLoading] = useState(false);
  const [refreshingSubId, setRefreshingSubId] = useState<string | null>(null);
  const [pingingSubId, setPingingSubId] = useState<string | null>(null);

  const fetchSubscriptions = useCallback(async () => {
    try {
      setLoading(true);
      const [subs, allNodes] = await Promise.all([
        vpnSubscriptionList.refresh(),
        vpnSubscriptionNodesList.refresh(),
      ]);
      setSubscriptions(subs);
      setNodes(groupNodes(allNodes));
    } catch (error) {
      console.error("Failed to load VPN subscriptions:", error);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    let disposed = false;

    const stopSubs = vpnSubscriptionList.subscribe({
      value: (list) => {
        if (!disposed) setSubscriptions(list);
      },
      error: (err) => console.error("Failed to load VPN subscriptions:", err),
    });

    const stopNodes = vpnSubscriptionNodesList.subscribe({
      value: (allNodes) => {
        if (!disposed) setNodes(groupNodes(allNodes));
      },
      error: (err) => console.error("Failed to load VPN nodes:", err),
    });

    // Initial load if needed
    void (async () => {
      try {
        const [subs, allNodes] = await Promise.all([
          vpnSubscriptionList.load(),
          vpnSubscriptionNodesList.load(),
        ]);
        if (!disposed) {
          setSubscriptions(subs);
          setNodes(groupNodes(allNodes));
        }
      } catch (err) {
        console.error("Failed initial load of VPN subscriptions:", err);
      }
    })();

    return () => {
      disposed = true;
      stopSubs();
      stopNodes();
    };
  }, []);

  const addSubscription = useCallback(
    async (name: string, url: string, autoUpdate = true) => {
      try {
        const newSub = await invoke<VpnSubscription>("add_vpn_subscription", {
          name,
          url,
          autoUpdate,
        });
        showSuccessToast(
          `Подписка "${newSub.name}" успешно добавлена (${newSub.node_count} серверов)`,
        );
        try {
          await emit("vpn-subscriptions-changed", {});
        } catch {}
        await Promise.all([
          vpnSubscriptionList.refresh(),
          vpnSubscriptionNodesList.refresh(),
        ]);
        return newSub;
      } catch (error) {
        showErrorToast(String(error));
        throw error;
      }
    },
    [],
  );

  const updateSubscription = useCallback(
    async (
      id: string,
      options?: {
        name?: string;
        url?: string;
        autoUpdate?: boolean;
        fetchNow?: boolean;
      },
    ) => {
      try {
        setRefreshingSubId(id);
        const updated = await invoke<VpnSubscription>(
          "update_vpn_subscription",
          {
            subscriptionId: id,
            name: options?.name ?? null,
            url: options?.url ?? null,
            autoUpdate: options?.autoUpdate ?? null,
            fetchNow: options?.fetchNow ?? true,
          },
        );
        showSuccessToast(
          `Подписка "${updated.name}" обновлена (${updated.node_count} серверов)`,
        );
        try {
          await emit("vpn-subscriptions-changed", {});
        } catch {}
        await Promise.all([
          vpnSubscriptionList.refresh(),
          vpnSubscriptionNodesList.refresh(),
        ]);
        return updated;
      } catch (error) {
        showErrorToast(String(error));
        throw error;
      } finally {
        setRefreshingSubId(null);
      }
    },
    [],
  );

  const deleteSubscription = useCallback(async (id: string) => {
    try {
      await invoke("delete_vpn_subscription", { subscriptionId: id });
      showSuccessToast("Подписка удалена");
      try {
        await emit("vpn-subscriptions-changed", {});
      } catch {}
      await Promise.all([
        vpnSubscriptionList.refresh(),
        vpnSubscriptionNodesList.refresh(),
      ]);
    } catch (error) {
      showErrorToast(String(error));
      throw error;
    }
  }, []);

  const pingSubscription = useCallback(async (id: string) => {
    try {
      setPingingSubId(id);
      const pingedNodes = await invoke<VpnSubscriptionNode[]>(
        "ping_vpn_subscription",
        {
          subscriptionId: id,
        },
      );
      try {
        await emit("vpn-subscriptions-changed", {});
      } catch {}
      await vpnSubscriptionNodesList.refresh();
      const available = pingedNodes.filter((n) => n.is_valid).length;
      toast.info(
        `Пинг завершён: ${available} из ${pingedNodes.length} узлов доступны`,
      );
      return pingedNodes;
    } catch (error) {
      showErrorToast(String(error));
      throw error;
    } finally {
      setPingingSubId(null);
    }
  }, []);

  const pingSingleNode = useCallback(async (nodeId: string, _subId: string) => {
    try {
      const updatedNode = await invoke<VpnSubscriptionNode>(
        "ping_vpn_subscription_node",
        {
          nodeId,
        },
      );
      try {
        await emit("vpn-subscriptions-changed", {});
      } catch {}
      await vpnSubscriptionNodesList.refresh();
      return updatedNode;
    } catch (error) {
      console.error("Failed to ping node:", error);
      throw error;
    }
  }, []);

  return {
    subscriptions,
    nodes,
    loading,
    refreshingSubId,
    pingingSubId,
    fetchSubscriptions,
    addSubscription,
    updateSubscription,
    deleteSubscription,
    pingSubscription,
    pingSingleNode,
  };
}
