//! Proxy Chain Storage and Management.
//!
//! Allows creating and persisting multi-hop proxy chains (Entry Relay -> Exit Node)
//! that can be assigned to browser profiles.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex as TokioMutex;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProxyChain {
  pub id: String,
  pub name: String,
  pub hop1_proxy_id: String,
  pub hop2_proxy_id: String,
  #[serde(default)]
  pub created_at: u64,
  /// Last end-to-end route probe. Latency is measured through both hops with
  /// a real request, not per-hop, so a dead exit behind a live relay still
  /// reports invalid.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub latency_ms: Option<u64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub is_valid: Option<bool>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub last_checked_at: Option<u64>,
}

pub struct ProxyChainManager {
  storage_dir: PathBuf,
}

impl Default for ProxyChainManager {
  fn default() -> Self {
    Self::new()
  }
}

impl ProxyChainManager {
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

  fn chains_file(&self) -> PathBuf {
    self.storage_dir.join("proxy_chains.json")
  }

  pub fn list_chains(&self) -> Vec<ProxyChain> {
    if let Ok(bytes) = fs::read(self.chains_file()) {
      if let Ok(chains) = serde_json::from_slice::<Vec<ProxyChain>>(&bytes) {
        return chains;
      }
    }
    Vec::new()
  }

  pub fn get_chain(&self, id: &str) -> Option<ProxyChain> {
    self.list_chains().into_iter().find(|c| c.id == id)
  }

  pub fn save_chains(&self, chains: &[ProxyChain]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(chains).map_err(|e| e.to_string())?;
    fs::write(self.chains_file(), json).map_err(|e| e.to_string())
  }

  pub fn add_chain(
    &self,
    name: String,
    hop1_proxy_id: String,
    hop2_proxy_id: String,
  ) -> Result<ProxyChain, String> {
    let trimmed = name.trim().to_string();
    if trimmed.is_empty() {
      return Err("Chain name cannot be empty".to_string());
    }
    if hop1_proxy_id.is_empty() || hop2_proxy_id.is_empty() {
      return Err("Both hop1 and hop2 proxies must be selected".to_string());
    }
    if hop1_proxy_id == hop2_proxy_id {
      return Err("Hop 1 (Relay) and Hop 2 (Exit) must be different proxies".to_string());
    }

    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_secs())
      .unwrap_or(0);

    let chain = ProxyChain {
      id: format!("chain-{}", Uuid::new_v4()),
      name: trimmed,
      hop1_proxy_id,
      hop2_proxy_id,
      created_at: now,
      latency_ms: None,
      is_valid: None,
      last_checked_at: None,
    };

    let mut chains = self.list_chains();
    chains.push(chain.clone());
    self.save_chains(&chains)?;

    Ok(chain)
  }

  pub fn update_chain(
    &self,
    chain_id: &str,
    name: Option<String>,
    hop1_proxy_id: Option<String>,
    hop2_proxy_id: Option<String>,
  ) -> Result<ProxyChain, String> {
    let mut chains = self.list_chains();
    let idx = chains
      .iter()
      .position(|c| c.id == chain_id)
      .ok_or_else(|| format!("Chain not found: {chain_id}"))?;

    let mut current = chains[idx].clone();
    let hops_changed = hop1_proxy_id.is_some() || hop2_proxy_id.is_some();

    if let Some(n) = name {
      let t = n.trim().to_string();
      if !t.is_empty() {
        current.name = t;
      }
    }
    if let Some(h1) = hop1_proxy_id {
      if !h1.is_empty() {
        current.hop1_proxy_id = h1;
      }
    }
    if let Some(h2) = hop2_proxy_id {
      if !h2.is_empty() {
        current.hop2_proxy_id = h2;
      }
    }

    if current.hop1_proxy_id == current.hop2_proxy_id {
      return Err("Hop 1 (Relay) and Hop 2 (Exit) must be different proxies".to_string());
    }

    // New hops invalidate any stored route probe.
    if hops_changed {
      current.latency_ms = None;
      current.is_valid = None;
      current.last_checked_at = None;
    }

    chains[idx] = current.clone();
    self.save_chains(&chains)?;

    Ok(current)
  }

  /// Store the result of an end-to-end route probe.
  pub fn record_ping(
    &self,
    chain_id: &str,
    latency_ms: Option<u64>,
    is_valid: bool,
  ) -> Result<ProxyChain, String> {
    let mut chains = self.list_chains();
    let idx = chains
      .iter()
      .position(|c| c.id == chain_id)
      .ok_or_else(|| format!("Chain not found: {chain_id}"))?;
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_secs())
      .unwrap_or(0);
    let chain = &mut chains[idx];
    chain.latency_ms = latency_ms;
    chain.is_valid = Some(is_valid);
    chain.last_checked_at = Some(now);
    let updated = chain.clone();
    self.save_chains(&chains)?;
    Ok(updated)
  }

  pub fn delete_chain(&self, chain_id: &str) -> Result<(), String> {
    let mut chains = self.list_chains();
    let initial_len = chains.len();
    chains.retain(|c| c.id != chain_id);

    if chains.len() != initial_len {
      self.save_chains(&chains)?;
    }
    Ok(())
  }
}

pub static PROXY_CHAIN_MANAGER: LazyLock<TokioMutex<ProxyChainManager>> =
  LazyLock::new(|| TokioMutex::new(ProxyChainManager::new()));

#[cfg(test)]
#[path = "chain_tests.rs"]
mod tests;
