#![allow(dead_code, unused_variables, unused_imports)]

//! Local headless CDP warming runner for Cookie Bot.

use crate::cookie_bot::CookieBotRun;
use rand::RngExt;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static ACTIVE_RUNS: std::sync::LazyLock<Mutex<HashMap<String, Arc<AtomicBool>>>> =
  std::sync::LazyLock::new(Default::default);

pub fn cancel_local_run(run_id: &str) -> bool {
  if let Some(flag) = ACTIVE_RUNS.lock().unwrap().get(run_id) {
    flag.store(true, Ordering::SeqCst);
    true
  } else {
    false
  }
}

pub fn default_sites_for_template(template_id: &str) -> Vec<String> {
  match template_id {
    "geo-ru" => vec![
      "https://ya.ru".into(),
      "https://vk.com".into(),
      "https://dzen.ru".into(),
      "https://www.ozon.ru".into(),
      "https://www.wildberries.ru".into(),
      "https://www.avito.ru".into(),
      "https://habr.com".into(),
      "https://www.rbc.ru".into(),
      "https://www.kinopoisk.ru".into(),
    ],
    "geo-us" => vec![
      "https://www.google.com".into(),
      "https://www.yahoo.com".into(),
      "https://www.cnn.com".into(),
      "https://www.foxnews.com".into(),
      "https://www.nytimes.com".into(),
      "https://craigslist.org".into(),
      "https://www.amazon.com".into(),
      "https://www.reddit.com".into(),
    ],
    "geo-eu" => vec![
      "https://www.spiegel.de".into(),
      "https://www.zeit.de".into(),
      "https://www.lemonde.fr".into(),
      "https://www.bbc.co.uk".into(),
      "https://www.elmundo.es".into(),
      "https://www.amazon.de".into(),
      "https://www.booking.com".into(),
    ],
    "tech-dev" => vec![
      "https://github.com".into(),
      "https://stackoverflow.com".into(),
      "https://news.ycombinator.com".into(),
      "https://dev.to".into(),
      "https://arxiv.org".into(),
      "https://medium.com".into(),
    ],
    "crypto-web3" => vec![
      "https://coinmarketcap.com".into(),
      "https://www.coingecko.com".into(),
      "https://www.binance.com".into(),
      "https://etherscan.io".into(),
      "https://www.coindesk.com".into(),
      "https://decrypt.co".into(),
    ],
    "news-and-media" => vec![
      "https://www.bbc.com".into(),
      "https://www.cnn.com".into(),
      "https://www.reuters.com".into(),
      "https://www.theverge.com".into(),
      "https://techcrunch.com".into(),
      "https://www.nytimes.com".into(),
      "https://www.bloomberg.com".into(),
      "https://www.theguardian.com".into(),
    ],
    "ecommerce" => vec![
      "https://www.amazon.com".into(),
      "https://www.ebay.com".into(),
      "https://www.walmart.com".into(),
      "https://www.aliexpress.com".into(),
      "https://www.target.com".into(),
      "https://www.etsy.com".into(),
      "https://www.bestbuy.com".into(),
    ],
    "social-entertainment" => vec![
      "https://www.youtube.com".into(),
      "https://www.reddit.com".into(),
      "https://www.twitch.tv".into(),
      "https://www.pinterest.com".into(),
      "https://imgur.com".into(),
      "https://medium.com".into(),
    ],
    _ => vec![
      "https://www.google.com".into(),
      "https://www.bing.com".into(),
      "https://duckduckgo.com".into(),
      "https://www.wikipedia.org".into(),
      "https://www.yahoo.com".into(),
      "https://www.reddit.com".into(),
      "https://www.youtube.com".into(),
      "https://github.com".into(),
      "https://news.ycombinator.com".into(),
      "https://www.bbc.com".into(),
    ],
  }
}

pub async fn execute_cookie_warming(
  app_handle: tauri::AppHandle,
  profile_id: String,
  mut run: CookieBotRun,
  sites: Vec<String>,
  max_minutes: u32,
) {
  let cancel_flag = Arc::new(AtomicBool::new(false));
  {
    ACTIVE_RUNS
      .lock()
      .unwrap()
      .insert(run.id.clone(), cancel_flag.clone());
  }

  // 1. Resolve Profile
  let profile = match crate::profile::ProfileManager::instance()
    .list_profiles()
    .ok()
    .and_then(|profiles| {
      profiles
        .into_iter()
        .find(|p| p.id.to_string() == profile_id)
    }) {
    Some(p) => p,
    None => {
      run.status = "failed".to_string();
      run.outcome_code = Some("profile_not_found".to_string());
      crate::cookie_bot_storage::upsert_run(run.clone());
      ACTIVE_RUNS.lock().unwrap().remove(&run.id);
      return;
    }
  };

  // 2. Launch browser in headless mode
  let gate = crate::launch_gate::FingerprintGate::default();
  let port = match crate::wayfern_manager::WayfernManager::find_free_port().await {
    Ok(p) => p,
    Err(e) => {
      run.status = "failed".to_string();
      run.outcome_code = Some(format!("port_error: {e}"));
      crate::cookie_bot_storage::upsert_run(run.clone());
      ACTIVE_RUNS.lock().unwrap().remove(&run.id);
      return;
    }
  };

  log::info!(
    "Starting headless browser for cookie warming profile={} port={}",
    profile.id,
    port
  );
  let runner = crate::browser_runner::BrowserRunner::instance();
  let launch_res = runner
    .launch_browser_with_debugging(
      app_handle.clone(),
      &profile,
      None,
      Some(port),
      true, // headless mode
      &gate,
    )
    .await;

  if let Err(e) = launch_res {
    log::error!("Failed to launch headless browser for warming: {e}");
    run.status = "failed".to_string();
    run.outcome_code = Some(format!("launch_failed: {e}"));
    crate::cookie_bot_storage::upsert_run(run.clone());
    ACTIVE_RUNS.lock().unwrap().remove(&run.id);
    return;
  }

  // 3. Connect to CDP and resolve page target
  let wayfern = crate::wayfern_manager::WayfernManager::instance();
  let mut page_ws: Option<String> = None;
  let deadline = std::time::Instant::now() + Duration::from_secs(35);
  let http_client = reqwest::Client::new();

  #[derive(serde::Deserialize)]
  struct CdpTargetInfo {
    #[serde(rename = "type")]
    target_type: String,
    #[serde(rename = "webSocketDebuggerUrl")]
    websocket_debugger_url: Option<String>,
  }

  while std::time::Instant::now() < deadline {
    let url = format!("http://127.0.0.1:{port}/json");
    if let Ok(resp) = http_client.get(&url).send().await {
      if let Ok(targets) = resp.json::<Vec<CdpTargetInfo>>().await {
        if let Some(target) = targets.into_iter().find(|t| {
          t.target_type == "page"
            && t
              .websocket_debugger_url
              .as_ref()
              .is_some_and(|u| !u.is_empty())
        }) {
          page_ws = target.websocket_debugger_url;
          break;
        }
      }
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
  }

  // Helper to cleanup browser
  let profiles_dir = runner.profile_manager.get_profiles_dir();
  let profile_data_path =
    crate::ephemeral_dirs::get_effective_profile_path(&profile, &profiles_dir);
  let profile_path_str = profile_data_path.to_string_lossy().to_string();

  let Some(ws_url) = page_ws else {
    log::error!("Could not find page CDP WebSocket on port {port}");
    let _ = wayfern.stop_wayfern(&profile_path_str).await;
    run.status = "failed".to_string();
    run.outcome_code = Some("cdp_connection_timeout".to_string());
    crate::cookie_bot_storage::upsert_run(run.clone());
    ACTIVE_RUNS.lock().unwrap().remove(&run.id);
    return;
  };

  run.status = "running".to_string();
  run.started_at = Some(chrono::Utc::now().to_rfc3339());
  crate::cookie_bot_storage::upsert_run(run.clone());

  let run_start_instant = std::time::Instant::now();
  let max_duration = Duration::from_secs((max_minutes as u64) * 60);

  // 4. Visit sites loop
  for site in &sites {
    if cancel_flag.load(Ordering::SeqCst) || run_start_instant.elapsed() > max_duration {
      break;
    }

    log::info!("Cookie bot warming site={} profile={}", site, profile.id);
    let nav_res = wayfern
      .send_cdp_command(&ws_url, "Page.navigate", json!({ "url": site }))
      .await;

    if nav_res.is_ok() {
      run.sites_visited += 1;
      // Wait for page load and rendering
      tokio::time::sleep(Duration::from_secs(4)).await;

      // Human-like multi-step scrolling: several scroll steps with
      // micro-pauses between them, each a random depth, with an occasional
      // reverse step that reads like re-reading a passage. The cancel flag is
      // checked between every step so a user stop lands within one pause.
      //
      // The whole step plan is drawn up front, inside a scoped block:
      // `ThreadRng` is not `Send`, so the generator must not live across the
      // CDP awaits below.
      let scroll_plan: Vec<(i64, bool, u64)> = {
        let mut rng = rand::rng();
        let steps = rng.random_range(3..=6);
        (0..steps)
          .map(|_| {
            let pause_ms = rng.random_range(500..=1500);
            // 10-20% of steps go back up a little, like re-reading.
            if rng.random_range(0.0..1.0) < 0.15 {
              (100, true, pause_ms)
            } else {
              (rng.random_range(150..=600), false, pause_ms)
            }
          })
          .collect()
      };
      for (depth, reverse, pause_ms) in scroll_plan {
        if cancel_flag.load(Ordering::SeqCst) || run_start_instant.elapsed() > max_duration {
          break;
        }

        let direction = if reverse { "-" } else { "" };
        let scroll_script = format!(
          r#"
            (function() {{
              window.scrollBy({{ top: {direction}{depth}, behavior: 'smooth' }});
            }})()
          "#
        );
        let _ = wayfern
          .send_cdp_command(
            &ws_url,
            "Runtime.evaluate",
            json!({ "expression": scroll_script }),
          )
          .await;

        // Reading pause between steps (500-1500ms), slept in small chunks so
        // a cancel is honoured mid-pause rather than after it.
        let mut waited_ms = 0u64;
        while waited_ms < pause_ms {
          if cancel_flag.load(Ordering::SeqCst) {
            break;
          }
          let chunk = (pause_ms - waited_ms).min(150);
          tokio::time::sleep(Duration::from_millis(chunk)).await;
          waited_ms += chunk;
        }
      }

      // Natural delay per site (8-15 seconds)
      for _ in 0..8 {
        if cancel_flag.load(Ordering::SeqCst) {
          break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
      }
    } else {
      run.sites_failed += 1;
    }

    run.billed_seconds = run_start_instant.elapsed().as_secs();
    crate::cookie_bot_storage::upsert_run(run.clone());
  }

  // 5. Cleanup & stop browser
  log::info!("Cookie bot warming finished for profile={}", profile.id);
  let _ = wayfern
    .send_cdp_command(&ws_url, "Browser.close", json!({}))
    .await;
  tokio::time::sleep(Duration::from_millis(500)).await;
  let _ = wayfern.stop_wayfern(&profile_path_str).await;

  // 6. Finalize run record
  if cancel_flag.load(Ordering::SeqCst) {
    run.status = "cancelled".to_string();
  } else if run.sites_visited > 0 {
    run.status = "succeeded".to_string();
  } else {
    run.status = "failed".to_string();
  }
  run.ended_at = Some(chrono::Utc::now().to_rfc3339());
  run.billed_seconds = run_start_instant.elapsed().as_secs();
  crate::cookie_bot_storage::upsert_run(run.clone());

  // Also update last_run_at in schedule if enrolled
  if let Some(mut sched) = crate::cookie_bot_storage::get_schedule(&profile_id) {
    sched.last_run_at = run.ended_at.clone();
    sched.last_run_id = Some(run.id.clone());
    crate::cookie_bot_storage::upsert_schedule(sched);
  }

  ACTIVE_RUNS.lock().unwrap().remove(&run.id);
}
