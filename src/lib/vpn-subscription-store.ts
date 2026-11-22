import { invoke } from "@tauri-apps/api/core";
import { createSharedResource } from "@/lib/profile-list-store";
import type { ProxyChain } from "@/types/proxy-chain";
import type {
  VpnSubscription,
  VpnSubscriptionNode,
} from "@/types/vpn-subscription";

export const vpnSubscriptionList = createSharedResource<VpnSubscription[]>(
  () => invoke<VpnSubscription[]>("get_vpn_subscriptions"),
  {
    events: ["vpn-subscriptions-changed"],
  },
);

export const vpnSubscriptionNodesList = createSharedResource<
  VpnSubscriptionNode[]
>(
  () =>
    invoke<VpnSubscriptionNode[]>("get_vpn_subscription_nodes", {
      subscriptionId: null,
    }),
  {
    events: ["vpn-subscriptions-changed"],
  },
);

export const proxyChainList = createSharedResource<ProxyChain[]>(
  () => invoke<ProxyChain[]>("get_proxy_chains"),
  {
    events: ["proxy-chains-changed"],
  },
);
