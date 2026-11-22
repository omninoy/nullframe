//! Local JSON persistence for Cookie Bot schedules, runs, and user templates.

use crate::cookie_bot::{CookieBotRun, CookieBotSchedule, CookieBotUserTemplate};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

static STORAGE_LOCK: Mutex<()> = Mutex::new(());

fn storage_dir() -> PathBuf {
  let dir = crate::app_dirs::data_dir().join("cookie_bot");
  let _ = std::fs::create_dir_all(&dir);
  dir
}

fn schedules_file() -> PathBuf {
  storage_dir().join("schedules.json")
}

fn runs_file() -> PathBuf {
  storage_dir().join("runs.json")
}

fn templates_file() -> PathBuf {
  storage_dir().join("templates.json")
}

// --- Schedules ---

pub fn load_schedules() -> HashMap<String, CookieBotSchedule> {
  let _guard = STORAGE_LOCK.lock().unwrap();
  let file = schedules_file();
  if let Ok(content) = std::fs::read_to_string(&file) {
    if let Ok(map) = serde_json::from_str::<HashMap<String, CookieBotSchedule>>(&content) {
      return map;
    }
  }
  HashMap::new()
}

pub fn save_schedules(map: &HashMap<String, CookieBotSchedule>) {
  let _guard = STORAGE_LOCK.lock().unwrap();
  let file = schedules_file();
  if let Ok(json) = serde_json::to_string_pretty(map) {
    let _ = std::fs::write(&file, json);
  }
}

pub fn get_schedule(profile_id: &str) -> Option<CookieBotSchedule> {
  let schedules = load_schedules();
  schedules.get(profile_id).cloned()
}

pub fn upsert_schedule(schedule: CookieBotSchedule) {
  let mut schedules = load_schedules();
  schedules.insert(schedule.profile_id.clone(), schedule);
  save_schedules(&schedules);
}

pub fn remove_schedule(profile_id: &str) -> bool {
  let mut schedules = load_schedules();
  let removed = schedules.remove(profile_id).is_some();
  if removed {
    save_schedules(&schedules);
  }
  removed
}

// --- Runs ---

pub fn load_runs() -> Vec<CookieBotRun> {
  let _guard = STORAGE_LOCK.lock().unwrap();
  let file = runs_file();
  if let Ok(content) = std::fs::read_to_string(&file) {
    if let Ok(runs) = serde_json::from_str::<Vec<CookieBotRun>>(&content) {
      return runs;
    }
  }
  Vec::new()
}

pub fn save_runs(runs: &[CookieBotRun]) {
  let _guard = STORAGE_LOCK.lock().unwrap();
  let file = runs_file();
  if let Ok(json) = serde_json::to_string_pretty(runs) {
    let _ = std::fs::write(&file, json);
  }
}

pub fn upsert_run(run: CookieBotRun) {
  let mut runs = load_runs();
  if let Some(pos) = runs.iter().position(|r| r.id == run.id) {
    runs[pos] = run;
  } else {
    runs.insert(0, run);
  }
  // Keep max 200 past runs
  if runs.len() > 200 {
    runs.truncate(200);
  }
  save_runs(&runs);
}

pub fn get_run(run_id: &str) -> Option<CookieBotRun> {
  let runs = load_runs();
  runs.into_iter().find(|r| r.id == run_id)
}

// --- Templates ---

pub fn load_user_templates() -> Vec<CookieBotUserTemplate> {
  let _guard = STORAGE_LOCK.lock().unwrap();
  let file = templates_file();
  if let Ok(content) = std::fs::read_to_string(&file) {
    if let Ok(templates) = serde_json::from_str::<Vec<CookieBotUserTemplate>>(&content) {
      return templates;
    }
  }
  Vec::new()
}

pub fn save_user_templates(templates: &[CookieBotUserTemplate]) {
  let _guard = STORAGE_LOCK.lock().unwrap();
  let file = templates_file();
  if let Ok(json) = serde_json::to_string_pretty(templates) {
    let _ = std::fs::write(&file, json);
  }
}
