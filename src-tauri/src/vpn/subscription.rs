//! VPN Subscription support for NekoBox, Happ, Incy, v2rayN and Clash providers.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;
use yaml_rust2::YamlLoader;

/// Prior ping/latency/validity/name of one subscription node, keyed by
/// server:port — what a subscription refresh must carry over.
type NodeSnapshotMap =
  HashMap<(String, String, u16), (String, Option<u64>, Option<bool>, Option<String>)>;

/// Represents a subscribed VPN/proxy provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VpnSubscription {
  pub id: String,
  pub name: String,
  pub url: String,
  #[serde(default = "default_true")]
  pub auto_update: bool,
  #[serde(default = "default_interval")]
  pub update_interval_mins: u32,
  #[serde(default)]
  pub last_updated_at: Option<u64>,
  #[serde(default)]
  pub node_count: usize,
  #[serde(default)]
  pub error: Option<String>,
}

fn default_true() -> bool {
  true
}

fn default_interval() -> u32 {
  1440 // 24 hours
}

/// A single proxy/VPN node parsed from a subscription.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VpnSubscriptionNode {
  pub id: String,
  pub subscription_id: String,
  pub name: String,
  pub protocol: String, // "vless", "vmess", "trojan", "hysteria2", "shadowsocks", "wireguard"
  pub raw_uri: String,
  pub server: String,
  pub port: u16,
  #[serde(default)]
  pub country_code: Option<String>,
  #[serde(default)]
  pub latency_ms: Option<u64>,
  #[serde(default)]
  pub is_valid: Option<bool>,
  #[serde(default)]
  pub last_checked_at: Option<u64>,
}

/// Helper to extract country code from flag emoji or common country prefixes in server name.
pub fn extract_country_code(name: &str) -> Option<String> {
  // Check for country flag emojis (regional indicator symbols 0x1F1E6 - 0x1F1FF)
  let chars: Vec<char> = name.chars().collect();
  for i in 0..chars.len().saturating_sub(1) {
    let c1 = chars[i] as u32;
    let c2 = chars[i + 1] as u32;
    if (0x1F1E6..=0x1F1FF).contains(&c1) && (0x1F1E6..=0x1F1FF).contains(&c2) {
      let b1 = ((c1 - 0x1F1E6) as u8) + b'A';
      let b2 = ((c2 - 0x1F1E6) as u8) + b'A';
      return Some(format!("{}{}", b1 as char, b2 as char));
    }
  }

  // Common keywords in VPN node names
  let lower = name.to_lowercase();
  let patterns = [
    ("united states", "US"),
    ("usa", "US"),
    ("сша", "US"),
    ("america", "US"),
    ("russia", "RU"),
    ("россия", "RU"),
    ("moscow", "RU"),
    ("germany", "DE"),
    ("германия", "DE"),
    ("frankfurt", "DE"),
    ("netherlands", "NL"),
    ("нидерланды", "NL"),
    ("amsterdam", "NL"),
    ("united kingdom", "GB"),
    ("great britain", "GB"),
    ("london", "GB"),
    ("france", "FR"),
    ("paris", "FR"),
    ("finland", "FI"),
    ("helsinki", "FI"),
    ("turkey", "TR"),
    ("турция", "TR"),
    ("istanbul", "TR"),
    ("japan", "JP"),
    ("tokyo", "JP"),
    ("singapore", "SG"),
    ("hong kong", "HK"),
    ("poland", "PL"),
    ("warsaw", "PL"),
    ("sweden", "SE"),
    ("stockholm", "SE"),
    ("kazakhstan", "KZ"),
    ("казахстан", "KZ"),
    ("almaty", "KZ"),
    ("georgia", "GE"),
    ("грузия", "GE"),
    ("tbilisi", "GE"),
    ("uae", "AE"),
    ("dubai", "AE"),
    ("canada", "CA"),
    ("switzerland", "CH"),
    ("austria", "AT"),
  ];

  for (pattern, code) in patterns {
    if lower.contains(pattern) {
      return Some(code.to_string());
    }
  }

  // Word token matching for 2-letter codes (e.g. "US", "DE", "NL", "RU")
  let token_codes = [
    ("us", "US"),
    ("ru", "RU"),
    ("de", "DE"),
    ("nl", "NL"),
    ("fi", "FI"),
    ("fr", "FR"),
    ("gb", "GB"),
    ("uk", "GB"),
    ("jp", "JP"),
    ("sg", "SG"),
    ("hk", "HK"),
    ("pl", "PL"),
    ("se", "SE"),
    ("kz", "KZ"),
    ("ge", "GE"),
    ("tr", "TR"),
    ("ae", "AE"),
    ("ca", "CA"),
    ("ch", "CH"),
    ("at", "AT"),
    ("it", "IT"),
    ("es", "ES"),
  ];

  for token in lower.split(|c: char| !c.is_alphabetic()) {
    for (t, code) in token_codes {
      if token == t {
        return Some(code.to_string());
      }
    }
  }

  None
}

/// Decode Base64 string if possible
fn try_decode_base64(input: &str) -> Option<String> {
  use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
  use base64::Engine;

  let trimmed = input.trim();
  if trimmed.contains("://") || trimmed.starts_with("proxies:") || trimmed.starts_with("port:") {
    return None;
  }

  // Remove potential whitespace or newlines inside base64 payload
  let clean: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();

  for engine in [&STANDARD, &STANDARD_NO_PAD, &URL_SAFE, &URL_SAFE_NO_PAD] {
    if let Ok(bytes) = engine.decode(&clean) {
      if let Ok(text) = String::from_utf8(bytes) {
        if text.contains("://") || text.contains("proxies:") {
          return Some(text);
        }
      }
    }
  }
  None
}

/// Parse subscription content which can be Base64-encoded, plaintext URIs, or Clash YAML.
pub fn parse_subscription_content(
  subscription_id: &str,
  raw_content: &str,
) -> Result<Vec<VpnSubscriptionNode>, String> {
  let content = try_decode_base64(raw_content).unwrap_or_else(|| raw_content.to_string());

  // Check if it is Clash YAML
  if content.contains("proxies:") {
    if let Ok(nodes) = parse_clash_yaml(subscription_id, &content) {
      if !nodes.is_empty() {
        return Ok(nodes);
      }
    }
  }

  // Otherwise parse as line-separated URIs
  parse_uri_list(subscription_id, &content)
}

/// Parse line-delimited URI list (vless, vmess, trojan, hysteria2, ss, wireguard)
pub fn parse_uri_list(
  subscription_id: &str,
  content: &str,
) -> Result<Vec<VpnSubscriptionNode>, String> {
  let mut nodes = Vec::new();

  for raw_line in content.lines() {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
      continue;
    }

    if let Some(node) = parse_single_uri(subscription_id, line) {
      nodes.push(node);
    }
  }

  if nodes.is_empty() {
    return Err("No supported proxy or VPN nodes found in subscription content".to_string());
  }

  Ok(nodes)
}

/// Parse a single URI into VpnSubscriptionNode
fn parse_single_uri(subscription_id: &str, uri: &str) -> Option<VpnSubscriptionNode> {
  let (scheme, rest) = uri.split_once("://")?;
  let scheme_lower = scheme.to_lowercase();

  let protocol = match scheme_lower.as_str() {
    "vless" => "vless",
    "vmess" => "vmess",
    "trojan" => "trojan",
    "hysteria2" | "hy2" => "hysteria2",
    "ss" => "shadowsocks",
    "wireguard" | "wg" => "wireguard",
    _ => return None,
  };

  let (server, port, name) = if protocol == "vmess" && !rest.contains('@') {
    // v2rayN VMess JSON base64 format
    parse_vmess_legacy_json(rest)?
  } else {
    parse_standard_uri_fields(protocol, rest)?
  };

  let country_code = extract_country_code(&name);

  Some(VpnSubscriptionNode {
    id: Uuid::new_v4().to_string(),
    subscription_id: subscription_id.to_string(),
    name,
    protocol: protocol.to_string(),
    raw_uri: uri.to_string(),
    server,
    port,
    country_code,
    latency_ms: None,
    is_valid: None,
    last_checked_at: None,
  })
}

/// Parse standard URI fields like `user@host:port?query#name`
fn parse_standard_uri_fields(protocol: &str, rest: &str) -> Option<(String, u16, String)> {
  let (body, fragment) = match rest.split_once('#') {
    Some((b, f)) => (
      b,
      Some(urlencoding::decode(f).unwrap_or(f.into()).to_string()),
    ),
    None => (rest, None),
  };

  let without_query = body.split('?').next().unwrap_or(body);
  let host_port_part = without_query
    .rsplit_once('@')
    .map(|(_, h)| h)
    .unwrap_or(without_query);

  let (host, port) = if let Some(bracket_end) = host_port_part.find(']') {
    let host = &host_port_part[1..bracket_end];
    let port_str = host_port_part[bracket_end + 1..].trim_start_matches(':');
    let port = port_str.parse::<u16>().unwrap_or(443);
    (host.to_string(), port)
  } else if let Some((h, p)) = host_port_part.split_once(':') {
    let port = p.parse::<u16>().unwrap_or(443);
    (h.to_string(), port)
  } else {
    (host_port_part.to_string(), 443)
  };

  let name = fragment.unwrap_or_else(|| format!("{}-{}", protocol.to_uppercase(), host));

  Some((host, port, name))
}

/// Parse legacy VMess base64 JSON
fn parse_vmess_legacy_json(body: &str) -> Option<(String, u16, String)> {
  use base64::engine::general_purpose::STANDARD;
  use base64::Engine;

  let (json_part, fragment_override) = match body.split_once('#') {
    Some((j, f)) => (
      j,
      Some(urlencoding::decode(f).unwrap_or(f.into()).to_string()),
    ),
    None => (body, None),
  };

  let decoded = STANDARD.decode(json_part.trim()).ok()?;
  let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;

  let server = json.get("add")?.as_str()?.to_string();
  let port = match json.get("port")? {
    serde_json::Value::Number(n) => n.as_u64()? as u16,
    serde_json::Value::String(s) => s.parse::<u16>().ok()?,
    _ => 443,
  };
  let name = fragment_override
    .or_else(|| {
      json
        .get("ps")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
    })
    .unwrap_or_else(|| format!("VMESS-{}", server));

  Some((server, port, name))
}

/// Parse Clash YAML format
fn parse_clash_yaml(
  subscription_id: &str,
  content: &str,
) -> Result<Vec<VpnSubscriptionNode>, String> {
  let docs = YamlLoader::load_from_str(content).map_err(|e| e.to_string())?;
  let doc = docs.first().ok_or("Empty YAML document")?;

  let proxies = match &doc["proxies"] {
    yaml_rust2::Yaml::Array(arr) => arr,
    _ => return Err("No 'proxies' array in YAML".to_string()),
  };

  let mut nodes = Vec::new();

  for item in proxies {
    let name = item["name"].as_str().unwrap_or("Proxy").to_string();
    let server = match item["server"].as_str() {
      Some(s) => s.to_string(),
      None => continue,
    };
    let port = match item["port"].as_i64() {
      Some(p) => p as u16,
      None => 443,
    };
    let ptype = item["type"].as_str().unwrap_or("").to_lowercase();

    let protocol = match ptype.as_str() {
      "ss" | "shadowsocks" => "shadowsocks",
      "vmess" => "vmess",
      "vless" => "vless",
      "trojan" => "trojan",
      "hysteria2" | "hy2" => "hysteria2",
      "wireguard" | "wg" => "wireguard",
      _ => continue,
    };

    // Synthesize raw URI for storage & dialer
    let raw_uri = match protocol {
      "vless" => {
        let uuid = item["uuid"].as_str().unwrap_or("");
        let sni = item["servername"]
          .as_str()
          .or_else(|| item["sni"].as_str())
          .unwrap_or("");
        format!(
          "vless://{}@{}:{}?sni={}#{}",
          uuid,
          server,
          port,
          sni,
          urlencoding::encode(&name)
        )
      }
      "trojan" => {
        let pass = item["password"].as_str().unwrap_or("");
        let sni = item["sni"].as_str().unwrap_or("");
        format!(
          "trojan://{}@{}:{}?sni={}#{}",
          pass,
          server,
          port,
          sni,
          urlencoding::encode(&name)
        )
      }
      "hysteria2" => {
        let pass = item["password"]
          .as_str()
          .or_else(|| item["auth"].as_str())
          .unwrap_or("");
        let sni = item["sni"].as_str().unwrap_or("");
        format!(
          "hysteria2://{}@{}:{}?sni={}#{}",
          pass,
          server,
          port,
          sni,
          urlencoding::encode(&name)
        )
      }
      "shadowsocks" => {
        let cipher = item["cipher"].as_str().unwrap_or("aes-128-gcm");
        let pass = item["password"].as_str().unwrap_or("");
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        let auth = STANDARD.encode(format!("{}:{}", cipher, pass));
        format!(
          "ss://{}@{}:{}#{}",
          auth,
          server,
          port,
          urlencoding::encode(&name)
        )
      }
      _ => {
        format!(
          "{}://{}:{}#{}",
          protocol,
          server,
          port,
          urlencoding::encode(&name)
        )
      }
    };

    let country_code = extract_country_code(&name);

    nodes.push(VpnSubscriptionNode {
      id: Uuid::new_v4().to_string(),
      subscription_id: subscription_id.to_string(),
      name,
      protocol: protocol.to_string(),
      raw_uri,
      server,
      port,
      country_code,
      latency_ms: None,
      is_valid: None,
      last_checked_at: None,
    });
  }

  Ok(nodes)
}

/// VPN Subscription Storage and Operation Manager
pub struct VpnSubscriptionManager {
  storage_dir: PathBuf,
}

impl Default for VpnSubscriptionManager {
  fn default() -> Self {
    Self::new()
  }
}

impl VpnSubscriptionManager {
  pub fn new() -> Self {
    let dir = crate::app_dirs::vpn_dir();
    let _ = fs::create_dir_all(&dir);
    Self { storage_dir: dir }
  }

  pub fn with_dir(dir: &Path) -> Self {
    let _ = fs::create_dir_all(dir);
    Self {
      storage_dir: dir.to_path_buf(),
    }
  }

  fn subs_file(&self) -> PathBuf {
    self.storage_dir.join("vpn_subscriptions.json")
  }

  fn nodes_file(&self) -> PathBuf {
    self.storage_dir.join("vpn_subscription_nodes.json")
  }

  pub fn list_subscriptions(&self) -> Vec<VpnSubscription> {
    if let Ok(bytes) = fs::read(self.subs_file()) {
      if let Ok(subs) = serde_json::from_slice::<Vec<VpnSubscription>>(&bytes) {
        return subs;
      }
    }
    Vec::new()
  }

  pub fn save_subscriptions(&self, subs: &[VpnSubscription]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(subs).map_err(|e| e.to_string())?;
    fs::write(self.subs_file(), json).map_err(|e| e.to_string())?;
    Ok(())
  }

  pub fn list_nodes(&self) -> Vec<VpnSubscriptionNode> {
    if let Ok(bytes) = fs::read(self.nodes_file()) {
      if let Ok(nodes) = serde_json::from_slice::<Vec<VpnSubscriptionNode>>(&bytes) {
        return nodes;
      }
    }
    Vec::new()
  }

  pub fn save_nodes(&self, nodes: &[VpnSubscriptionNode]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(nodes).map_err(|e| e.to_string())?;
    fs::write(self.nodes_file(), json).map_err(|e| e.to_string())?;
    Ok(())
  }

  pub fn list_nodes_by_subscription(&self, sub_id: &str) -> Vec<VpnSubscriptionNode> {
    self
      .list_nodes()
      .into_iter()
      .filter(|n| n.subscription_id == sub_id)
      .collect()
  }

  /// Add a new subscription and immediately fetch its nodes.
  pub async fn add_subscription(
    &self,
    name: String,
    url: String,
    auto_update: bool,
  ) -> Result<VpnSubscription, String> {
    let id = Uuid::new_v4().to_string();
    let sub = VpnSubscription {
      id: id.clone(),
      name,
      url: url.trim().to_string(),
      auto_update,
      update_interval_mins: 1440,
      last_updated_at: None,
      node_count: 0,
      error: None,
    };

    let mut subs = self.list_subscriptions();
    subs.push(sub.clone());
    self.save_subscriptions(&subs)?;

    // Fetch and populate nodes
    self.fetch_and_update_nodes(&id).await
  }

  /// Update an existing subscription: update details and optionally fetch URL.
  pub async fn update_subscription(
    &self,
    id: &str,
    name: Option<String>,
    url: Option<String>,
    auto_update: Option<bool>,
    fetch_now: bool,
  ) -> Result<VpnSubscription, String> {
    let mut subs = self.list_subscriptions();
    let idx = subs
      .iter()
      .position(|s| s.id == id)
      .ok_or_else(|| "Subscription not found".to_string())?;

    let mut should_fetch = fetch_now;

    if let Some(n) = name {
      subs[idx].name = n;
    }
    if let Some(u) = url {
      if u != subs[idx].url {
        subs[idx].url = u;
        should_fetch = true;
      }
    }
    if let Some(au) = auto_update {
      subs[idx].auto_update = au;
    }

    self.save_subscriptions(&subs)?;

    if !should_fetch {
      return Ok(subs[idx].clone());
    }

    self.fetch_and_update_nodes(id).await
  }

  /// Fetch URL for subscription, parse nodes, diff and save.
  pub async fn fetch_and_update_nodes(&self, id: &str) -> Result<VpnSubscription, String> {
    let mut subs = self.list_subscriptions();
    let idx = subs
      .iter()
      .position(|s| s.id == id)
      .ok_or_else(|| "Subscription not found".to_string())?;

    let url = subs[idx].url.clone();
    log::info!(
      "Updating VPN subscription '{}' from {}",
      subs[idx].name,
      url
    );

    let client = reqwest::Client::builder()
      .timeout(Duration::from_secs(25))
      .build()
      .map_err(|e| e.to_string())?;

    let response = client
      .get(&url)
      .header("User-Agent", "Clash/v1.18.0 (NekoBox; Windows)")
      .header("Accept", "*/*")
      .send()
      .await;

    let now = chrono::Utc::now().timestamp() as u64;

    let raw_text = match response {
      Ok(resp) => {
        if !resp.status().is_success() {
          let err = format!("HTTP error: {}", resp.status());
          subs[idx].error = Some(err.clone());
          self.save_subscriptions(&subs)?;
          return Err(err);
        }
        match resp.text().await {
          Ok(t) => t,
          Err(e) => {
            let err = format!("Failed to read response: {e}");
            subs[idx].error = Some(err.clone());
            self.save_subscriptions(&subs)?;
            return Err(err);
          }
        }
      }
      Err(e) => {
        let err = format!("Network request failed: {e}");
        subs[idx].error = Some(err.clone());
        self.save_subscriptions(&subs)?;
        return Err(err);
      }
    };

    let parsed_nodes = match parse_subscription_content(id, &raw_text) {
      Ok(nodes) => nodes,
      Err(e) => {
        subs[idx].error = Some(e.clone());
        self.save_subscriptions(&subs)?;
        return Err(e);
      }
    };

    // Load existing nodes to preserve previous node IDs and ping / latency results
    let mut existing_nodes = self.list_nodes();
    let old_node_map: NodeSnapshotMap = existing_nodes
      .iter()
      .filter(|n| n.subscription_id == id)
      .map(|n| {
        (
          (n.protocol.clone(), n.server.clone(), n.port),
          (
            n.id.clone(),
            n.latency_ms,
            n.is_valid,
            n.country_code.clone(),
          ),
        )
      })
      .collect();

    // Remove old nodes of this subscription
    existing_nodes.retain(|n| n.subscription_id != id);

    // Merge previous IDs and metrics into new nodes
    let mut new_nodes = Vec::new();
    for mut node in parsed_nodes {
      if let Some((old_id, latency, is_valid, country)) =
        old_node_map.get(&(node.protocol.clone(), node.server.clone(), node.port))
      {
        node.id = old_id.clone();
        node.latency_ms = *latency;
        node.is_valid = *is_valid;
        if node.country_code.is_none() {
          node.country_code = country.clone();
        }
      }
      new_nodes.push(node);
    }

    let node_count = new_nodes.len();
    existing_nodes.extend(new_nodes);
    self.save_nodes(&existing_nodes)?;

    // Update subscription record
    subs[idx].last_updated_at = Some(now);
    subs[idx].node_count = node_count;
    subs[idx].error = None;
    self.save_subscriptions(&subs)?;

    Ok(subs[idx].clone())
  }

  /// Delete a subscription and all of its nodes.
  pub fn delete_subscription(&self, id: &str) -> Result<(), String> {
    let mut subs = self.list_subscriptions();
    subs.retain(|s| s.id != id);
    self.save_subscriptions(&subs)?;

    let mut nodes = self.list_nodes();
    nodes.retain(|n| n.subscription_id != id);
    self.save_nodes(&nodes)?;

    Ok(())
  }

  /// Measure latency for all nodes of a specific subscription.
  ///
  /// Two phases: a TCP reachability sweep first (fast), then a protocol-level
  /// probe for the reachable Xray-carried nodes. TCP alone is misleading for
  /// VLESS/VMess/Trojan/Hysteria2 — a dead REALITY node still accepts the
  /// connection and then swallows every tunneled byte, which used to paint the
  /// node green while profiles launched on it kept timing out.
  pub async fn ping_subscription_nodes(
    &self,
    sub_id: &str,
  ) -> Result<Vec<VpnSubscriptionNode>, String> {
    let mut all_nodes = self.list_nodes();
    let mut sub_nodes: Vec<VpnSubscriptionNode> = all_nodes
      .iter()
      .filter(|n| n.subscription_id == sub_id)
      .cloned()
      .collect();

    if sub_nodes.is_empty() {
      return Ok(Vec::new());
    }

    log::info!(
      "Pinging {} nodes for subscription '{}'",
      sub_nodes.len(),
      sub_id
    );

    // Phase 1: bounded TCP sweep — drops closed ports before the expensive
    // per-node Xray workers spin up.
    let tcp_results = run_tcp_ping(&sub_nodes, 16).await;

    // Phase 2: protocol probe for reachable Xray-carried nodes, deduplicated
    // by raw URI — subscriptions ship many copies of the same server.
    let protocol_targets: Vec<VpnSubscriptionNode> = sub_nodes
      .iter()
      .filter(|n| {
        tcp_results
          .get(&n.id)
          .is_some_and(|(_, valid)| *valid == Some(true))
          && crate::xray::is_xray_proxy_type(&n.protocol)
      })
      .cloned()
      .collect();
    let protocol_results = run_protocol_ping(&protocol_targets, 8).await;

    let mut ping_results = tcp_results;
    for (node_id, (latency, is_valid)) in protocol_results {
      ping_results.insert(node_id, (latency, is_valid));
    }

    let now = chrono::Utc::now().timestamp() as u64;

    // Apply results to sub_nodes and all_nodes
    for node in &mut sub_nodes {
      if let Some((latency, is_valid)) = ping_results.get(&node.id) {
        node.latency_ms = *latency;
        node.is_valid = *is_valid;
        node.last_checked_at = Some(now);
      }
    }

    for node in &mut all_nodes {
      if let Some((latency, is_valid)) = ping_results.get(&node.id) {
        node.latency_ms = *latency;
        node.is_valid = *is_valid;
        node.last_checked_at = Some(now);
      }
    }

    self.save_nodes(&all_nodes)?;
    Ok(sub_nodes)
  }

  /// Ping an individual node by its ID.
  ///
  /// Runs the same protocol-level probe the bulk ping uses, so a single-node
  /// check answers the question the user is actually asking: will traffic
  /// through this node work?
  pub async fn ping_single_node(&self, node_id: &str) -> Result<VpnSubscriptionNode, String> {
    let mut all_nodes = self.list_nodes();
    let idx = all_nodes
      .iter()
      .position(|n| n.id == node_id)
      .ok_or_else(|| "Node not found".to_string())?;

    let node = all_nodes[idx].clone();
    let (latency, is_valid) = if crate::xray::is_xray_proxy_type(&node.protocol) {
      let mut results = run_protocol_ping(std::slice::from_ref(&node), 1).await;
      results.remove(&node.id).unwrap_or((None, Some(false)))
    } else {
      tcp_ping(&node).await
    };

    all_nodes[idx].latency_ms = latency;
    all_nodes[idx].is_valid = is_valid;
    all_nodes[idx].last_checked_at = Some(chrono::Utc::now().timestamp() as u64);

    let updated = all_nodes[idx].clone();
    self.save_nodes(&all_nodes)?;

    Ok(updated)
  }
}

/// Probe URL for protocol-level node checks: a bare 204 every exit can
/// answer without cookies, redirects or a large body.
const PROTOCOL_PROBE_URL: &str = "http://www.gstatic.com/generate_204";

/// TCP reachability probe: fast, but wrong about TLS/REALITY nodes whose
/// ports are open while the tunnel underneath is dead.
async fn tcp_ping(node: &VpnSubscriptionNode) -> (Option<u64>, Option<bool>) {
  let target = format!("{}:{}", node.server, node.port);
  let start = Instant::now();
  let result = tokio::time::timeout(
    Duration::from_millis(3000),
    tokio::net::TcpStream::connect(&target),
  )
  .await;
  let elapsed = start.elapsed().as_millis() as u64;
  match result {
    Ok(Ok(_)) => (Some(elapsed), Some(true)),
    _ => (None, Some(false)),
  }
}

/// Run `tcp_ping` over every node with bounded concurrency.
async fn run_tcp_ping(
  nodes: &[VpnSubscriptionNode],
  concurrency: usize,
) -> HashMap<String, (Option<u64>, Option<bool>)> {
  let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
  let mut tasks = Vec::new();

  for node in nodes {
    let sem = semaphore.clone();
    let node = node.clone();
    tasks.push(tokio::spawn(async move {
      let _permit = sem.acquire().await;
      let (latency, is_valid) = tcp_ping(&node).await;
      (node.id, latency, is_valid)
    }));
  }

  let mut results = HashMap::new();
  for task in tasks {
    if let Ok((node_id, latency, is_valid)) = task.await {
      results.insert(node_id, (latency, is_valid));
    }
  }
  results
}

/// Protocol-level probe: dial the node through a real Xray worker and fetch a
/// bare 204 through it. Measures the tunneled round-trip, so a node whose TCP
/// port is open but whose VLESS/REALITY handshake dies reports as unreachable
/// instead of advertising the green latency that sent profiles into
/// ERR_TIMED_OUT.
///
/// Results are deduplicated by raw URI: subscriptions ship many copies of the
/// same server, and one worker answers for all of them.
async fn run_protocol_ping(
  nodes: &[VpnSubscriptionNode],
  concurrency: usize,
) -> HashMap<String, (Option<u64>, Option<bool>)> {
  if nodes.is_empty() {
    return HashMap::new();
  }

  // uri -> shared result, node_id list per uri
  let mut by_uri: HashMap<String, Vec<String>> = HashMap::new();
  for node in nodes {
    by_uri
      .entry(node.raw_uri.clone())
      .or_default()
      .push(node.id.clone());
  }

  let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
  let mut tasks = Vec::new();

  for (uri, node_ids) in by_uri {
    let sem = semaphore.clone();
    tasks.push(tokio::spawn(async move {
      let _permit = sem.acquire().await;
      let result = protocol_ping(&uri).await;
      (node_ids, result)
    }));
  }

  let mut results = HashMap::new();
  for task in tasks {
    if let Ok((node_ids, result)) = task.await {
      for node_id in node_ids {
        results.insert(node_id, result);
      }
    }
  }
  results
}

/// Probe one share link through a disposable Xray worker.
async fn protocol_ping(uri: &str) -> (Option<u64>, Option<bool>) {
  let worker = match crate::xray_worker_runner::start_xray_worker(None, uri).await {
    Ok(worker) => worker,
    Err(error) => {
      log::debug!("Node protocol check worker start failed err=\"{error}\"");
      return (None, Some(false));
    }
  };

  let probe = async {
    let proxy_url =
      crate::proxy_manager::ProxyManager::build_probe_proxy_url(&worker.local_proxy_settings());
    let client = reqwest::Client::builder()
      .proxy(reqwest::Proxy::all(&proxy_url).map_err(|error| error.to_string())?)
      .timeout(Duration::from_secs(8))
      .build()
      .map_err(|error| error.to_string())?;
    let started = Instant::now();
    let response = client
      .head(PROTOCOL_PROBE_URL)
      .send()
      .await
      .map_err(|error| error.to_string())?;
    let elapsed = started.elapsed().as_millis() as u64;
    let status_ok = response.status().is_success();
    if status_ok {
      Ok::<u64, String>(elapsed)
    } else {
      Err(format!("probe status {}", response.status()))
    }
  };

  let result = match probe.await {
    Ok(elapsed) => (Some(elapsed), Some(true)),
    Err(error) => {
      log::debug!("Node protocol check probe failed err=\"{error}\"");
      (None, Some(false))
    }
  };

  if let Err(error) = crate::xray_worker_runner::stop_xray_worker_now(&worker.id) {
    log::debug!("Node protocol check worker stop failed err=\"{error}\"");
  }
  result
}

pub static VPN_SUBSCRIPTION_MANAGER: LazyLock<TokioMutex<VpnSubscriptionManager>> =
  LazyLock::new(|| TokioMutex::new(VpnSubscriptionManager::new()));

#[cfg(test)]
#[path = "subscription_tests.rs"]
mod tests;
