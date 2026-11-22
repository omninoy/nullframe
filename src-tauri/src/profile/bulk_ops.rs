//! Bulk creation of profiles from a pasted or uploaded table, and launching a
//! batch of profiles with a pause between launches (staggered launch).
//!
//! The table arrives already parsed as records — the dialog previews it before
//! sending — but `parse_table_records` stays here as the reference parser for
//! REST, MCP and tests. One row, one profile: name first, then proxy, OS,
//! tags and a note.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::manager::ProfileManager;
use super::patch::ProfilePatch;
use super::types::BrowserProfile;
use crate::proxy_manager::PROXY_MANAGER;

/// One row of the bulk table. `proxy` is a stored proxy's name or its
/// `host:port`; `os` is a fingerprint OS ("windows", "macos", "linux").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct BulkProfileRecord {
  pub name: String,
  #[serde(default)]
  pub proxy: Option<String>,
  #[serde(default)]
  pub os: Option<String>,
  #[serde(default)]
  pub tags: Vec<String>,
  #[serde(default)]
  pub notes: Option<String>,
}

/// What one bulk run made. `error` is why creation stopped before every
/// record was reached, as a coded error JSON; what was made stays.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct BulkTableCreateResult {
  #[schema(value_type = Vec<Object>)]
  pub created: Vec<BrowserProfile>,
  pub error: Option<String>,
  pub cancelled: bool,
}

/// The maximum records in one bulk table run, the same ceiling as templates.
const MAX_BULK_CREATE: usize = crate::profile_templates::MAX_BULK_CREATE as usize;

/// The OS values a fingerprint can target; anything else keeps the default.
fn is_fingerprint_os(value: &str) -> bool {
  matches!(value, "windows" | "macos" | "linux")
}

/// Split a tags cell into tags. The cell may join them with commas or pipes.
fn tags_from_cell(cell: &str) -> Vec<String> {
  cell
    .split([',', '|'])
    .map(str::trim)
    .filter(|tag| !tag.is_empty())
    .map(str::to_string)
    .collect()
}

/// The delimiter a table separates its columns with: whichever of the
/// candidates occurs most often across the non-empty lines. A table none of
/// them joins is one column wide, one name per line.
fn detect_delimiter(text: &str) -> Option<char> {
  let mut counts = [0usize; 3];
  for line in text.lines().filter(|line| !line.trim().is_empty()) {
    for (index, delimiter) in ['\t', ';', ','].into_iter().enumerate() {
      counts[index] += line.matches(delimiter).count();
    }
  }
  let (index, best) = counts
    .iter()
    .copied()
    .enumerate()
    .max_by_key(|&(index, count)| (count, std::cmp::Reverse(index)))?;
  (best > 0).then(|| ['\t', ';', ','][index])
}

/// A row split into cells, honoring quotes the way a spreadsheet exports
/// them: a quoted cell keeps its delimiters, `""` inside is one `"`.
fn split_row(line: &str, delimiter: char) -> Vec<String> {
  let mut cells = Vec::new();
  let mut cell = String::new();
  let mut quoted = false;
  let mut chars = line.chars().peekable();
  while let Some(c) = chars.next() {
    if quoted {
      if c == '"' {
        if chars.peek() == Some(&'"') {
          cell.push('"');
          chars.next();
        } else {
          quoted = false;
        }
      } else {
        cell.push(c);
      }
    } else if c == '"' && cell.is_empty() {
      quoted = true;
    } else if c == delimiter {
      cells.push(std::mem::take(&mut cell));
    } else {
      cell.push(c);
    }
  }
  cells.push(cell);
  cells
}

/// Parse a pasted or uploaded table into records. JSON is an array of
/// objects (or one object); anything else is CSV/TSV/TXT with `,`, `;` or
/// tab delimiters. Rows without a name are skipped.
pub fn parse_table_records(text: &str) -> Vec<BulkProfileRecord> {
  let trimmed = text.trim();
  if trimmed.is_empty() {
    return Vec::new();
  }

  if trimmed.starts_with('[') || trimmed.starts_with('{') {
    let value: serde_json::Value = match serde_json::from_str(trimmed) {
      Ok(value) => value,
      Err(e) => {
        log::warn!("Bulk table JSON parse failed err=\"{e}\"");
        return Vec::new();
      }
    };
    let rows = match value {
      serde_json::Value::Array(rows) => rows,
      serde_json::Value::Object(object) => match object.get("profiles") {
        Some(serde_json::Value::Array(rows)) => rows.clone(),
        // A bare object is one record.
        _ => vec![serde_json::Value::Object(object)],
      },
      _ => return Vec::new(),
    };
    return rows
      .into_iter()
      .filter_map(|row| {
        let name = row.get("name")?.as_str()?.trim().to_string();
        if name.is_empty() {
          return None;
        }
        let tags = match row.get("tags") {
          Some(serde_json::Value::Array(tags)) => tags
            .iter()
            .filter_map(|tag| tag.as_str().map(str::trim).map(str::to_string))
            .filter(|tag| !tag.is_empty())
            .collect(),
          Some(serde_json::Value::String(text)) => tags_from_cell(text),
          _ => Vec::new(),
        };
        Some(BulkProfileRecord {
          name,
          proxy: row
            .get("proxy")
            .and_then(|p| p.as_str())
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string),
          os: row
            .get("os")
            .and_then(|os| os.as_str())
            .map(str::trim)
            .filter(|os| !os.is_empty())
            .map(str::to_string),
          tags,
          notes: row
            .get("notes")
            .or_else(|| row.get("note"))
            .and_then(|n| n.as_str())
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string),
        })
      })
      .collect();
  }

  let delimiter = match detect_delimiter(trimmed) {
    Some(delimiter) => delimiter,
    None => {
      // One column: every non-empty line is a name.
      return trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|name| BulkProfileRecord {
          name: name.to_string(),
          proxy: None,
          os: None,
          tags: Vec::new(),
          notes: None,
        })
        .collect();
    }
  };

  fn cell_value(cell: &str) -> Option<String> {
    let trimmed = cell.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
  }

  let mut lines = trimmed
    .lines()
    .skip_while(|line| line.trim().is_empty())
    .peekable();
  // A header row names its first column; a data row would not.
  if let Some(first) = lines.peek() {
    let first_cell = first.trim_start().trim_start_matches('"');
    if first_cell
      .split(delimiter)
      .next()
      .map(str::trim)
      .is_some_and(|cell| cell.eq_ignore_ascii_case("name"))
    {
      lines.next();
    }
  }

  lines
    .filter_map(|line| {
      let cells = split_row(line, delimiter);
      let name = cells.first()?.trim().to_string();
      if name.is_empty() {
        return None;
      }
      let cell = |index: usize| cells.get(index).map(String::as_str).unwrap_or("");
      // Anything past the fifth column belongs to the note, delimiter and all.
      let notes = if cells.len() > 5 {
        let mut notes = cell(4).to_string();
        for extra in &cells[5..] {
          notes.push(delimiter);
          notes.push_str(extra.trim());
        }
        cell_value(&notes)
      } else {
        cell_value(cell(4))
      };
      Some(BulkProfileRecord {
        name,
        proxy: cell_value(cell(1)),
        os: cell_value(cell(2)),
        tags: tags_from_cell(cell(3)),
        notes,
      })
    })
    .collect()
}

/// A stored proxy reduced to what matching a table cell needs.
struct ProxyOption {
  id: String,
  name: String,
  host_port: String,
}

fn proxy_options() -> Vec<ProxyOption> {
  PROXY_MANAGER
    .get_stored_proxies()
    .into_iter()
    .map(|proxy| ProxyOption {
      host_port: format!(
        "{}:{}",
        proxy.proxy_settings.host, proxy.proxy_settings.port
      ),
      id: proxy.id,
      name: proxy.name,
    })
    .collect()
}

/// The proxy a record routes through: the named one, else the one listening
/// on `host:port`, else the proxy id itself. `None` leaves the profile direct.
fn match_proxy(requested: &str, options: &[ProxyOption]) -> Option<String> {
  let wanted = requested.trim();
  if wanted.is_empty() {
    return None;
  }
  options
    .iter()
    .find(|option| {
      option.name.eq_ignore_ascii_case(wanted) || option.host_port.eq_ignore_ascii_case(wanted)
    })
    .or_else(|| options.iter().find(|option| option.id == wanted))
    .map(|option| option.id.clone())
}

#[derive(Clone, Serialize)]
struct BulkTableProgress {
  done: usize,
  total: usize,
}

static BULK_TABLE_RUNNING: AtomicBool = AtomicBool::new(false);
static BULK_TABLE_CANCEL: AtomicBool = AtomicBool::new(false);

struct BulkTableRun;

impl Drop for BulkTableRun {
  fn drop(&mut self) {
    BULK_TABLE_RUNNING.store(false, Ordering::SeqCst);
  }
}

/// Make one profile per record. Everything a record leaves out takes the
/// default: no proxy, the host OS fingerprint, no tags or note.
pub async fn create_profiles_from_table(
  app_handle: &tauri::AppHandle,
  records: Vec<BulkProfileRecord>,
  auto_assign_proxies: bool,
) -> Result<BulkTableCreateResult, String> {
  if records.is_empty() {
    return Err(serde_json::json!({ "code": "BULK_TABLE_EMPTY" }).to_string());
  }
  if records.len() > MAX_BULK_CREATE {
    return Err(
      serde_json::json!({
        "code": "BULK_CREATE_COUNT_INVALID",
        "params": { "max": MAX_BULK_CREATE.to_string() }
      })
      .to_string(),
    );
  }
  if records.iter().any(|record| record.name.trim().is_empty()) {
    return Err(serde_json::json!({ "code": "NAME_CANNOT_BE_EMPTY" }).to_string());
  }
  if BULK_TABLE_RUNNING.swap(true, Ordering::SeqCst) {
    return Err(serde_json::json!({ "code": "BULK_CREATE_BUSY" }).to_string());
  }
  let _run = BulkTableRun;
  BULK_TABLE_CANCEL.store(false, Ordering::SeqCst);

  let version = crate::profile_templates::latest_wayfern_version().ok_or_else(|| {
    serde_json::json!({ "code": "BROWSER_NOT_DOWNLOADED", "params": { "browser": "Wayfern" } })
      .to_string()
  })?;

  let options = proxy_options();
  let manager = ProfileManager::instance();
  let started = std::time::Instant::now();
  let total = records.len();
  let mut created = Vec::with_capacity(total);
  let mut error = None;
  let mut cancelled = false;

  for (index, record) in records.into_iter().enumerate() {
    if BULK_TABLE_CANCEL.load(Ordering::SeqCst) {
      cancelled = true;
      break;
    }
    // Round-robin over every stored proxy when auto-assigning; otherwise the
    // record names its own, and an unknown name routes direct.
    let proxy_id = if auto_assign_proxies {
      (!options.is_empty()).then(|| options[index % options.len()].id.clone())
    } else {
      record
        .proxy
        .as_deref()
        .and_then(|requested| match_proxy(requested, &options))
    };
    let os = record
      .os
      .as_deref()
      .map(str::trim)
      .filter(|os| is_fingerprint_os(os))
      .map(str::to_string);
    let config = os.map(|os| crate::wayfern_manager::WayfernConfig {
      os: Some(os),
      ..Default::default()
    });

    let made = manager
      .create_profile_with_group(
        app_handle,
        record.name.trim(),
        "wayfern",
        &version,
        "stable",
        proxy_id,
        None,
        config,
        None,
        false,
        None,
        None,
        None,
      )
      .await
      .and_then(|profile| {
        manager.finish_new_profile(
          profile,
          &ProfilePatch {
            tags_add: record.tags,
            note: Some(record.notes),
            ..Default::default()
          },
        )
      });
    match made {
      Ok(profile) => created.push(profile),
      Err(e) => {
        error = Some(crate::wrap_backend_error(e, "Failed to create profile"));
        break;
      }
    }
    let progress = BulkTableProgress {
      done: index + 1,
      total,
    };
    if let Err(e) = crate::events::emit("profiles-bulk-table-progress", progress) {
      log::debug!("Event emit failed event=profiles-bulk-table-progress err=\"{e}\"");
    }
  }

  let elapsed_ms = started.elapsed().as_millis();
  log::info!(
    "Profiles created from a table requested={total} created={} cancelled={} elapsed_ms={elapsed_ms}",
    created.len(),
    cancelled
  );
  if created.is_empty() {
    if let Some(error) = error {
      return Err(error);
    }
  }
  Ok(BulkTableCreateResult {
    created,
    error,
    cancelled,
  })
}

/// Stop a running bulk table creation after the profile being made now.
#[tauri::command]
pub fn cancel_bulk_table_create() {
  BULK_TABLE_CANCEL.store(true, Ordering::SeqCst);
}

/// Parse a pasted or uploaded table into records, for the dialog's preview.
#[tauri::command]
pub fn bulk_parse_table(text: String) -> Vec<BulkProfileRecord> {
  parse_table_records(&text)
}

#[tauri::command]
pub async fn bulk_create_profiles_from_table(
  app_handle: tauri::AppHandle,
  records: Vec<BulkProfileRecord>,
  auto_assign_proxies: bool,
) -> Result<BulkTableCreateResult, String> {
  create_profiles_from_table(&app_handle, records, auto_assign_proxies).await
}

static STAGGERED_RUNNING: AtomicBool = AtomicBool::new(false);
static STAGGERED_CANCEL: AtomicBool = AtomicBool::new(false);

struct StaggeredRun;

impl Drop for StaggeredRun {
  fn drop(&mut self) {
    STAGGERED_RUNNING.store(false, Ordering::SeqCst);
  }
}

#[derive(Clone, Serialize)]
struct StaggeredLaunchProgress {
  done: usize,
  total: usize,
  launched: usize,
  failed: usize,
  cancelled: bool,
}

/// Launch the profiles one at a time, waiting `delay_ms` between launches so
/// the machine and the network never take the whole batch at once. A profile
/// that cannot start counts as failed and does not stop the batch; a cancel
/// stops it after the launch in flight. Already-running profiles are skipped.
#[tauri::command]
pub async fn staggered_launch_profiles(
  app_handle: tauri::AppHandle,
  profile_ids: Vec<String>,
  delay_ms: u64,
) -> Result<(), String> {
  if profile_ids.is_empty() {
    return Ok(());
  }
  if STAGGERED_RUNNING.swap(true, Ordering::SeqCst) {
    return Err(serde_json::json!({ "code": "STAGGERED_LAUNCH_BUSY" }).to_string());
  }
  let _run = StaggeredRun;
  STAGGERED_CANCEL.store(false, Ordering::SeqCst);

  let delay = Duration::from_millis(delay_ms.min(3_600_000));
  let total = profile_ids.len();
  let mut launched = 0usize;
  let mut failed = 0usize;
  let mut cancelled = false;
  let started = std::time::Instant::now();

  for (index, profile_id) in profile_ids.into_iter().enumerate() {
    if STAGGERED_CANCEL.load(Ordering::SeqCst) {
      cancelled = true;
      break;
    }
    // The load error is turned into a count before any await: a
    // `Box<dyn Error>` is not `Send` and may not cross the launch.
    let loaded = match ProfileManager::instance().find_profile(&profile_id) {
      Ok(profile) => Some(profile),
      Err(e) => {
        failed += 1;
        log::warn!("Staggered launch skipped a missing profile profile={profile_id} err=\"{e}\"");
        None
      }
    };
    if let Some(profile) = loaded {
      if crate::browser_runner::browser_is_running_for(&profile_id) {
        log::debug!("Staggered launch skipped a running profile profile={profile_id}");
      } else {
        let options = crate::browser_runner::LaunchOptions::default();
        match crate::browser_runner::launch_browser_profile_impl(
          app_handle.clone(),
          profile,
          None,
          options,
        )
        .await
        {
          Ok(_) => launched += 1,
          Err(e) => {
            failed += 1;
            log::warn!("Staggered launch failed profile={profile_id} err=\"{e}\"");
          }
        }
      }
    }
    let progress = StaggeredLaunchProgress {
      done: index + 1,
      total,
      launched,
      failed,
      cancelled,
    };
    if let Err(e) = crate::events::emit("profiles-staggered-launch-progress", progress) {
      log::debug!("Event emit failed event=profiles-staggered-launch-progress err=\"{e}\"");
    }
    // A pause separates launches; the last launch has nothing after it.
    if index + 1 < total {
      tokio::time::sleep(delay).await;
    }
  }

  let elapsed_ms = started.elapsed().as_millis();
  log::info!(
    "Staggered launch finished total={total} launched={launched} failed={failed} cancelled={cancelled} delay_ms={delay_ms} elapsed_ms={elapsed_ms}"
  );
  Ok(())
}

/// Stop a staggered launch queue: the launch in flight finishes, the rest of
/// the queue never starts.
#[tauri::command]
pub fn cancel_staggered_launch() {
  STAGGERED_CANCEL.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn an_empty_table_parses_to_nothing() {
    assert!(parse_table_records("").is_empty());
    assert!(parse_table_records("   \n  ").is_empty());
  }

  #[test]
  fn a_name_per_line_is_a_one_column_table() {
    let records = parse_table_records("Shop A\nShop B\n");
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].name, "Shop A");
    assert_eq!(records[1].name, "Shop B");
    assert!(records[0].proxy.is_none() && records[0].tags.is_empty());
  }

  #[test]
  fn a_header_row_is_skipped_by_name() {
    let records = parse_table_records("name;proxy;os;tags;notes\nShop;pool-1;windows;a,b;login\n");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "Shop");
    assert_eq!(records[0].proxy.as_deref(), Some("pool-1"));
    assert_eq!(records[0].os.as_deref(), Some("windows"));
    assert_eq!(records[0].tags, ["a", "b"]);
    assert_eq!(records[0].notes.as_deref(), Some("login"));
  }

  #[test]
  fn a_data_row_without_a_header_is_taken_whole() {
    let records = parse_table_records("Shop,us-proxy,macos,shop|work,buyer account\n");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].proxy.as_deref(), Some("us-proxy"));
    assert_eq!(records[0].os.as_deref(), Some("macos"));
    assert_eq!(records[0].tags, ["shop", "work"]);
    assert_eq!(records[0].notes.as_deref(), Some("buyer account"));
  }

  #[test]
  fn the_delimiter_is_what_the_table_uses_most() {
    let comma = parse_table_records("A,b,c\nD,e,f\n");
    assert_eq!(comma.len(), 2);
    let semicolon = parse_table_records("A;b\nD;e\n");
    assert_eq!(semicolon.len(), 2);
    assert_eq!(semicolon[0].proxy.as_deref(), Some("b"));
  }

  #[test]
  fn quoted_cells_keep_their_delimiters() {
    // A quoted comma survives inside its cell; tags still split on commas
    // and pipes, so the quote matters where text is free-form.
    let records = parse_table_records("name,proxy,os,tags,notes\nShop,,,\"a|b\",\"two, words\"\n");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tags, ["a", "b"]);
    assert_eq!(records[0].notes.as_deref(), Some("two, words"));
  }

  #[test]
  fn doubled_quotes_inside_a_cell_are_one_quote() {
    let records = parse_table_records("name,proxy,os,tags,notes\nShop,,,,\"say \"\"hi\"\"\"\n");
    assert_eq!(records[0].notes.as_deref(), Some("say \"hi\""));
  }

  #[test]
  fn rows_without_a_name_are_skipped() {
    let records = parse_table_records(",proxy\nShop,\n");
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "Shop");
  }

  #[test]
  fn a_json_array_parses_with_arrays_of_tags() {
    let records = parse_table_records(
      r#"[{"name":"Shop","proxy":"pool-1","os":"linux","tags":["a","b"],"notes":"note"}]"#,
    );
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tags, ["a", "b"]);
    assert_eq!(records[0].os.as_deref(), Some("linux"));
    assert_eq!(records[0].notes.as_deref(), Some("note"));
  }

  #[test]
  fn a_json_object_wraps_itself_and_tags_may_be_a_string() {
    let records = parse_table_records(r#"{"name":"Shop","tags":"a|b"}"#);
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].tags, ["a", "b"]);
  }

  #[test]
  fn a_json_profiles_array_parses() {
    let records = parse_table_records(r#"{"profiles":[{"name":"A"},{"name":"B"}]}"#);
    assert_eq!(records.len(), 2);
  }

  #[test]
  fn broken_json_yields_no_records() {
    assert!(parse_table_records("[{broken").is_empty());
  }

  #[test]
  fn a_proxy_matches_by_name_then_host_then_id() {
    let options = vec![
      ProxyOption {
        id: "id-1".to_string(),
        name: "Pool One".to_string(),
        host_port: "1.1.1.1:8000".to_string(),
      },
      ProxyOption {
        id: "id-2".to_string(),
        name: "pool-2".to_string(),
        host_port: "2.2.2.2:8000".to_string(),
      },
    ];
    assert_eq!(match_proxy("pool one", &options).as_deref(), Some("id-1"));
    assert_eq!(
      match_proxy("2.2.2.2:8000", &options).as_deref(),
      Some("id-2")
    );
    assert_eq!(match_proxy("id-2", &options).as_deref(), Some("id-2"));
    assert_eq!(match_proxy("unknown", &options), None);
    assert_eq!(match_proxy("  ", &options), None);
  }

  #[test]
  fn only_fingerprint_oses_are_taken_from_a_cell() {
    for os in ["windows", "macos", "linux"] {
      assert!(is_fingerprint_os(os));
    }
    for os in ["android", "Windows", "", "chromeos"] {
      assert!(!is_fingerprint_os(os));
    }
  }

  #[test]
  fn tags_come_apart_on_commas_and_pipes() {
    assert_eq!(tags_from_cell("a, b | c  ,"), ["a", "b", "c"]);
    assert!(tags_from_cell(" , | ").is_empty());
  }
}
