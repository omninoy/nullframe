export interface VpnSubscription {
  id: string;
  name: string;
  url: string;
  auto_update: boolean;
  update_interval_mins: number;
  last_updated_at?: number;
  node_count: number;
  error?: string;
}

export interface VpnSubscriptionNode {
  id: string;
  subscription_id: string;
  name: string;
  protocol: string;
  raw_uri: string;
  server: string;
  port: number;
  country_code?: string;
  latency_ms?: number;
  is_valid?: boolean;
  last_checked_at?: number;
}
