//! A proxy provider's "change IP" link: opening it gives the proxy a new exit
//! address. Mobile and some residential providers hand one out per port.
//!
//! The link is called from this machine, not through the proxy: providers
//! accept it from anywhere, and a rotating proxy may be unreachable through
//! itself while it rotates. Every profile behind the proxy moves with it.

use crate::ip_utils;
use crate::proxy_manager::{now_ms, ProxyManager, StoredProxy, PROXY_MANAGER};
use serde::Serialize;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const ROTATION_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_URL_CHARS: usize = 2048;
const BULK_CONCURRENCY: usize = 4;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, utoipa::ToSchema)]
pub struct RotationResult {
  pub proxy_id: String,
  /// The provider's HTTP status, when it answered.
  pub status: Option<u16>,
  pub elapsed_ms: u64,
  /// A coded backend error when the IP change was not confirmed.
  pub error: Option<String>,
}

/// What a confirmed rotation looked like. Unlike [`RotationResult`], the
/// provider's answer is followed by a re-check of the exit, so this carries
/// the addresses the proxy moved between — what a toast and the table's
/// "last rotated" tooltip read.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RotationReport {
  pub proxy_id: String,
  /// The exit as it stood before the change link was called, when it could
  /// be seen at all.
  pub old_ip: Option<String>,
  /// The exit seen after the rotation, when it could be seen.
  pub new_ip: Option<String>,
  /// Unix milliseconds the rotation was stamped at; drives the auto-rotation
  /// timer.
  pub rotated_at: u64,
  /// False when the provider answered but the exit was seen again at the old
  /// address: the request went through, the address did not move.
  pub success: bool,
}

/// Blank clears; anything else must be an http(s) URL.
pub fn checked_rotation_url(value: Option<String>) -> Result<Option<String>, String> {
  let Some(value) = value
    .map(|v| v.trim().to_string())
    .filter(|v| !v.is_empty())
  else {
    return Ok(None);
  };
  let valid = value.chars().count() <= MAX_URL_CHARS
    && !value.chars().any(char::is_whitespace)
    && url::Url::parse(&value)
      .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some());
  if valid {
    Ok(Some(value))
  } else {
    Err(crate::backend_error("PROXY_ROTATION_URL_INVALID"))
  }
}

fn client() -> Result<reqwest::Client, String> {
  crate::app_proxy::client_builder()
    .timeout(ROTATION_TIMEOUT)
    .build()
    .map_err(|e| crate::backend_error_with_detail("PROXY_ROTATION_FAILED", e))
}

async fn call(client: &reqwest::Client, proxy_id: &str) -> RotationResult {
  let refused = |code: &str| RotationResult {
    proxy_id: proxy_id.to_string(),
    status: None,
    elapsed_ms: 0,
    error: Some(crate::backend_error(code)),
  };
  match PROXY_MANAGER
    .get_stored_proxies()
    .into_iter()
    .find(|proxy| proxy.id == proxy_id)
  {
    None => refused("PROXY_NOT_FOUND"),
    Some(proxy) => match proxy.rotation_url {
      Some(url) => request(client, proxy_id, &url).await,
      None => refused("PROXY_ROTATION_URL_MISSING"),
    },
  }
}

async fn request(client: &reqwest::Client, proxy_id: &str, url: &str) -> RotationResult {
  let started = Instant::now();
  let mut result = RotationResult {
    proxy_id: proxy_id.to_string(),
    status: None,
    elapsed_ms: 0,
    error: None,
  };
  // The URL is never logged: providers put the account key in it.
  match client.get(url).send().await {
    Ok(response) => {
      let status = response.status();
      result.status = Some(status.as_u16());
      if !status.is_success() {
        result.error = Some(
          serde_json::json!({
            "code": "PROXY_ROTATION_FAILED",
            "params": { "status": status.as_u16().to_string() }
          })
          .to_string(),
        );
      }
    }
    Err(e) => {
      result.error = Some(crate::system_proxy::explain(&e).unwrap_or_else(|| {
        if e.is_timeout() {
          crate::backend_error("PROXY_ROTATION_TIMEOUT")
        } else {
          crate::backend_error("PROXY_ROTATION_UNREACHABLE")
        }
      }));
    }
  }
  result.elapsed_ms = started.elapsed().as_millis() as u64;
  match (&result.error, result.status) {
    (None, Some(status)) => log::info!(
      "Proxy IP change requested proxy={proxy_id} status={status} elapsed_ms={}",
      result.elapsed_ms
    ),
    (Some(_), status) => log::warn!(
      "Proxy IP change failed proxy={proxy_id} status={} elapsed_ms={}",
      status.map_or_else(|| "none".to_string(), |s| s.to_string()),
      result.elapsed_ms
    ),
    (None, None) => {}
  }
  result
}

pub async fn rotate(proxy_id: &str) -> Result<RotationResult, String> {
  let result = call(&client()?, proxy_id).await;
  match result.error {
    Some(error) => Err(error),
    None => Ok(result),
  }
}

/// How often the scheduler wakes to look for rotations that came due.
const SCHEDULER_TICK: Duration = Duration::from_secs(60);
/// How long a provider is given to actually move the exit after it answered.
/// Two seconds sits inside the 1-3s window providers document.
const PROPAGATION_WAIT: Duration = Duration::from_secs(2);
/// A slow provider is asked again before "the IP did not change" is believed.
const IP_CONFIRM_ATTEMPTS: usize = 3;
/// The cap `ip_utils` puts on one public-IP ask; kept the same so a stalled
/// exit fails the probe rather than the rotation.
const IP_PROBE_TIMEOUT: Duration = Duration::from_secs(12);
/// After a failed rotation the proxy sits out this long before the scheduler
/// asks the same provider again, so a dead link is retried at a human pace.
const FAILURE_BACKOFF: Duration = Duration::from_secs(10 * 60);

/// The proxy's current exit address, asked through the proxy itself.
/// `None` when the type cannot be probed with reqwest (an Xray link) or the
/// ask failed — neither blocks the rotation, which works from anywhere.
async fn exit_ip(proxy: &StoredProxy) -> Option<String> {
  let probe_url = ProxyManager::build_probe_proxy_url(&proxy.proxy_settings);
  if !crate::proxy_storage::reqwest_can_proxy(&probe_url) {
    return None;
  }
  match tokio::time::timeout(
    IP_PROBE_TIMEOUT,
    ip_utils::fetch_public_ip(Some(&probe_url)),
  )
  .await
  {
    Ok(Ok(ip)) => Some(ip),
    Ok(Err(e)) => {
      log::debug!("Exit probe failed proxy={} err=\"{e}\"", proxy.id);
      None
    }
    Err(_) => None,
  }
}

/// Ask the provider for a new exit and confirm the move. The addresses are
/// read through the proxy before and after the change link, `last_rotated_at`
/// is stamped, and the report says whether the exit actually moved. The stamp
/// is written whenever the provider accepted the request — a provider that
/// keeps a sticky session must not be re-asked every tick — while a provider
/// that refused is an `Err` and leaves the stamp alone.
pub async fn trigger_ip_rotation(proxy_id: &str) -> Result<RotationReport, String> {
  let proxy = PROXY_MANAGER
    .get_stored_proxies()
    .into_iter()
    .find(|proxy| proxy.id == proxy_id)
    .ok_or_else(|| crate::backend_error("PROXY_NOT_FOUND"))?;
  let url = proxy
    .rotation_url
    .as_deref()
    .map(str::trim)
    .filter(|url| !url.is_empty())
    .ok_or_else(|| crate::backend_error("PROXY_ROTATION_URL_MISSING"))?;

  let client = client()?;
  let old_ip = exit_ip(&proxy).await;

  let result = request(&client, proxy_id, url).await;
  if let Some(error) = result.error {
    return Err(error);
  }

  // Providers move the exit a moment after they answer, so the new address
  // is read until it differs from the old one or the attempts run out.
  let mut new_ip = None;
  let mut success = true;
  for _ in 0..IP_CONFIRM_ATTEMPTS {
    tokio::time::sleep(PROPAGATION_WAIT).await;
    new_ip = exit_ip(&proxy).await;
    match (&old_ip, &new_ip) {
      (Some(old), Some(new)) if old == new => success = false,
      _ => {
        success = true;
        break;
      }
    }
  }

  let rotated_at = now_ms();
  if let Err(e) = PROXY_MANAGER.set_last_rotated_at(proxy_id, rotated_at) {
    log::warn!("Rotation stamp save failed proxy={proxy_id} err=\"{e}\"");
  }
  log::info!(
    "Proxy IP rotated proxy={proxy_id} moved={success} old_seen={} new_seen={} elapsed_ms={}",
    old_ip.is_some(),
    new_ip.is_some(),
    result.elapsed_ms
  );
  Ok(RotationReport {
    proxy_id: proxy_id.to_string(),
    old_ip,
    new_ip,
    rotated_at,
    success,
  })
}

/// The stored proxies whose auto-rotation came due: a change link, an
/// interval above zero, and a last rotation old enough — or none at all, so a
/// schedule set while the app was closed catches up on the first tick.
fn due_rotations(failed_at: &HashMap<String, Instant>) -> Vec<StoredProxy> {
  PROXY_MANAGER
    .get_stored_proxies()
    .into_iter()
    .filter(|proxy| proxy.rotation_url.is_some())
    .filter(|proxy| proxy.rotation_interval_mins.is_some_and(|mins| mins > 0))
    .filter(|proxy| {
      let interval_ms = u64::from(proxy.rotation_interval_mins.unwrap_or(0)) * 60_000;
      let last = proxy.last_rotated_at.unwrap_or(0);
      now_ms().saturating_sub(last) >= interval_ms
    })
    .filter(|proxy| {
      failed_at
        .get(&proxy.id)
        .is_none_or(|at| at.elapsed() >= FAILURE_BACKOFF)
    })
    .collect()
}

/// The background timer that rotates every proxy whose interval came due.
/// Rotations run one after another: each waits out the provider's swap, and
/// asking every provider at once would give the same machine a burst of
/// simultaneous exit changes to reconcile.
pub async fn run_auto_rotation_scheduler() {
  let mut tick = tokio::time::interval(SCHEDULER_TICK);
  tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
  let mut failed_at: HashMap<String, Instant> = HashMap::new();
  loop {
    tick.tick().await;
    let due = due_rotations(&failed_at);
    // Proxies that no longer exist stop holding a backoff slot.
    failed_at.retain(|id, _| due.iter().any(|proxy| proxy.id == *id));
    for proxy in due {
      log::info!("Auto-rotation due proxy={}", proxy.id);
      match trigger_ip_rotation(&proxy.id).await {
        Ok(report) => {
          log::info!(
            "Auto-rotation done proxy={} moved={}",
            report.proxy_id,
            report.success
          );
        }
        Err(e) => {
          failed_at.insert(proxy.id.clone(), Instant::now());
          log::warn!(
            "Auto-rotation failed proxy={} err=\"{}\"",
            proxy.id,
            crate::log_redaction::text(&e)
          );
        }
      }
    }
  }
}

/// Several proxies at once, a few in flight at a time. Each gets its own
/// result, so one failing provider does not hide the others.
pub async fn rotate_many(proxy_ids: Vec<String>) -> Result<Vec<RotationResult>, String> {
  use futures_util::StreamExt;
  let client = client()?;
  let mut seen = std::collections::HashSet::new();
  let unique: Vec<String> = proxy_ids
    .into_iter()
    .filter(|id| seen.insert(id.clone()))
    .collect();
  Ok(
    futures_util::stream::iter(unique)
      .map(|id| {
        let client = client.clone();
        async move { call(&client, &id).await }
      })
      .buffered(BULK_CONCURRENCY)
      .collect()
      .await,
  )
}

#[tauri::command]
pub fn set_proxy_rotation_url(
  proxy_id: String,
  rotation_url: Option<String>,
) -> Result<crate::proxy_manager::StoredProxy, String> {
  let rotation_url = checked_rotation_url(rotation_url)?;
  PROXY_MANAGER.set_rotation_url(&proxy_id, rotation_url)
}

/// Set a proxy's auto-rotation schedule: the minutes between automatic
/// rotations (`None` or 0 turns the timer off) and whether a launch on this
/// proxy rotates first.
#[tauri::command]
pub fn set_proxy_rotation_config(
  proxy_id: String,
  interval_mins: Option<u32>,
  rotate_on_launch: bool,
) -> Result<(), String> {
  let interval_mins = interval_mins.filter(|mins| *mins > 0);
  PROXY_MANAGER
    .set_rotation_config(&proxy_id, interval_mins, rotate_on_launch)
    .map(|_| ())
}

/// A confirmed rotation: the provider answered, the exit was re-read, and
/// `last_rotated_at` moved. The report is what the toast and the table's
/// tooltip show.
#[tauri::command]
pub async fn rotate_proxy_ip(proxy_id: String) -> Result<RotationReport, String> {
  trigger_ip_rotation(&proxy_id).await
}

#[tauri::command]
pub async fn rotate_proxy_ips(proxy_ids: Vec<String>) -> Result<Vec<RotationResult>, String> {
  rotate_many(proxy_ids).await
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rotation_urls_must_be_http_links() {
    assert_eq!(checked_rotation_url(None), Ok(None));
    assert_eq!(checked_rotation_url(Some("  ".to_string())), Ok(None));
    assert_eq!(
      checked_rotation_url(Some(
        " https://api.provider.example/rotate?key=abc ".to_string()
      )),
      Ok(Some(
        "https://api.provider.example/rotate?key=abc".to_string()
      ))
    );
    for bad in [
      "ftp://provider.example/rotate",
      "provider.example/rotate",
      "https://",
      "https://provider.example/rotate key",
      "javascript:alert(1)",
    ] {
      assert!(
        checked_rotation_url(Some(bad.to_string()))
          .unwrap_err()
          .contains("PROXY_ROTATION_URL_INVALID"),
        "{bad}"
      );
    }
    assert!(checked_rotation_url(Some(format!("https://p.example/{}", "a".repeat(2100)))).is_err());
  }

  /// Answers every request with `status` and counts the requests.
  async fn provider(status: u16) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
      "http://{}/rotate?key=secret",
      listener.local_addr().unwrap()
    );
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
      while let Ok((mut stream, _)) = listener.accept().await {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf).await;
        let _ = stream
          .write_all(
            format!("HTTP/1.1 {status} X\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
              .as_bytes(),
          )
          .await;
      }
    });
    (url, hits)
  }

  #[tokio::test]
  async fn a_provider_answer_decides_the_result() {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();

    let (url, hits) = provider(200).await;
    let ok = request(&client, "p1", &url).await;
    assert_eq!((ok.status, ok.error.as_deref()), (Some(200), None));
    assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);

    let (url, _) = provider(429).await;
    let refused = request(&client, "p1", &url).await;
    assert_eq!(refused.status, Some(429));
    let error = refused.error.unwrap();
    assert!(error.contains("PROXY_ROTATION_FAILED") && error.contains("429"));

    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let unreachable = request(&client, "p1", &format!("http://127.0.0.1:{port}/rotate")).await;
    assert_eq!(unreachable.status, None);
    assert!(unreachable
      .error
      .unwrap()
      .contains("PROXY_ROTATION_UNREACHABLE"));
  }
}
