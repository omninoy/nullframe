export interface ProxyChain {
  id: string;
  name: string;
  hop1_proxy_id: string;
  hop2_proxy_id: string;
  created_at?: number;
  /** End-to-end route probe results (both hops, one real request). */
  latency_ms?: number | null;
  is_valid?: boolean | null;
  last_checked_at?: number | null;
}

export interface ProxyChainPingResult {
  chain: ProxyChain;
  latency_ms: number | null;
  is_valid: boolean;
  error: string | null;
}
