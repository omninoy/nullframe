//! Browser CDP tool execution engine for AI Agent in NullFrame.
//! Executes `<act>` commands produced by the LLM (navigate, click, type, scroll, extract_text, screenshot, eval)
//! via Chrome DevTools Protocol (CDP) connected to the active browser profile.

use crate::cdp_target::{self, CdpTarget};
use crate::profile::ProfileManager;
use base64::Engine;
use serde_json::json;

/// Resolves the live `CdpTarget` for the given `profile_id`.
/// If the profile is not currently running, returns a clean error indicating it must be launched.
pub async fn resolve_profile_target(profile_id: &str) -> Result<CdpTarget, String> {
  let pid = profile_id.trim();
  if pid.is_empty() {
    return Err("No active browser profile attached to this agent chat. Please select and launch a profile first.".to_string());
  }

  let profiles = ProfileManager::instance()
    .list_profiles()
    .map_err(|e| format!("Failed to list profiles: {e}"))?;

  let profile = profiles
    .into_iter()
    .find(|p| p.id.to_string() == pid)
    .ok_or_else(|| format!("Browser profile '{pid}' not found."))?;

  if profile.browser != "wayfern" {
    return Err(format!(
      "Profile '{}' runs {}, which cannot be driven over CDP.",
      profile.name, profile.browser
    ));
  }

  let target = cdp_target::resolve(&profile).await.map_err(|e| match e {
    cdp_target::ResolveError::NotRunning(_) => format!(
      "Browser profile '{}' is not running. Please launch the profile to execute actions.",
      profile.name
    ),
    cdp_target::ResolveError::Unsupported(msg) => {
      format!("Profile '{}' cannot be driven: {msg}", profile.name)
    }
    cdp_target::ResolveError::Endpoint(msg) => {
      format!(
        "Failed to reach endpoint for profile '{}': {msg}",
        profile.name
      )
    }
  })?;

  Ok(target)
}

/// Executes a browser action string against the specified `profile_id`.
/// Supported commands:
/// - `navigate: <url>`
/// - `click: <selector>`
/// - `type: <selector> | <text>`
/// - `scroll: <up|down> <pixels>`
/// - `extract_text: <selector>`
/// - `screenshot`
/// - `eval: <javascript>`
pub async fn execute_browser_action(profile_id: &str, action: &str) -> Result<String, String> {
  let action_trimmed = action.trim();
  if action_trimmed.is_empty() {
    return Err("Empty action provided".to_string());
  }

  let target = resolve_profile_target(profile_id).await?;

  dispatch_action(&target, action_trimmed).await
}

/// Dispatches an action, supporting multi-line command lists when appropriate.
async fn dispatch_action(target: &CdpTarget, action: &str) -> Result<String, String> {
  // If multi-line and not an eval command, run line-by-line
  if action.contains('\n')
    && !action.starts_with("eval:")
    && !action.starts_with("eval ")
    && !action.starts_with("javascript:")
  {
    let mut outputs = Vec::new();
    for line in action.lines() {
      let trimmed = line.trim();
      if trimmed.is_empty() {
        continue;
      }
      match dispatch_single_action(target, trimmed).await {
        Ok(out) => outputs.push(out),
        Err(err) => return Err(err),
      }
    }
    if outputs.is_empty() {
      return Err("No executable actions found in action block".to_string());
    }
    return Ok(outputs.join("\n"));
  }

  dispatch_single_action(target, action).await
}

/// Dispatches a single action command against the target.
async fn dispatch_single_action(target: &CdpTarget, action: &str) -> Result<String, String> {
  if let Some(url) = strip_command_prefix(action, "navigate") {
    navigate_to_url(target, url).await
  } else if let Some(url) = strip_command_prefix(action, "goto") {
    navigate_to_url(target, url).await
  } else if let Some(url) = strip_command_prefix(action, "open") {
    navigate_to_url(target, url).await
  } else if let Some(sel) = strip_command_prefix(action, "click") {
    click_element(target, sel).await
  } else if let Some(rem) = strip_command_prefix(action, "type") {
    let (sel, text) = if let Some((s, t)) = rem.split_once('|') {
      (s.trim(), t.trim())
    } else if let Some((s, t)) = rem.split_once(' ') {
      (s.trim(), t.trim())
    } else {
      return Err("Type action requires format: `type: <selector> | <text>`".to_string());
    };
    type_into_element(target, sel, text).await
  } else if let Some(rem) = strip_command_prefix(action, "scroll") {
    let parts: Vec<&str> = rem.split_whitespace().collect();
    let (dir, px) = match parts.as_slice() {
      [d, p] => (*d, p.parse::<i64>().unwrap_or(500)),
      [p] if p.parse::<i64>().is_ok() => {
        let v = p.parse::<i64>().unwrap();
        if v >= 0 {
          ("down", v)
        } else {
          ("up", -v)
        }
      }
      [d] if d.eq_ignore_ascii_case("up") || d.eq_ignore_ascii_case("down") => (*d, 500),
      _ => ("down", 500),
    };
    scroll_page(target, dir, px).await
  } else if let Some(sel) = strip_command_prefix(action, "extract_text") {
    extract_element_text(target, sel).await
  } else if let Some(sel) = strip_command_prefix(action, "extract") {
    extract_element_text(target, sel).await
  } else if action.eq_ignore_ascii_case("screenshot")
    || strip_command_prefix(action, "screenshot").is_some()
  {
    capture_page_screenshot(target).await
  } else if let Some(expr) = strip_command_prefix(action, "eval") {
    evaluate_javascript(target, expr).await
  } else if let Some(expr) = strip_command_prefix(action, "javascript") {
    evaluate_javascript(target, expr).await
  } else if action.starts_with("http://") || action.starts_with("https://") {
    navigate_to_url(target, action).await
  } else {
    Err(format!(
      "Unsupported browser action: '{action}'. Available commands: navigate: <url>, click: <selector>, type: <selector> | <text>, scroll: <up|down> <pixels>, extract_text: <selector>, screenshot, eval: <javascript>"
    ))
  }
}

/// Helper to strip command prefix case-insensitively, supporting both ':' and space delimiter.
fn strip_command_prefix<'a>(action: &'a str, cmd: &str) -> Option<&'a str> {
  let trimmed = action.trim();
  let cmd_len = cmd.len();
  if trimmed.len() >= cmd_len && trimmed[..cmd_len].eq_ignore_ascii_case(cmd) {
    let rest = &trimmed[cmd_len..];
    if let Some(after) = rest.strip_prefix(':') {
      return Some(after.trim());
    }
    if rest.starts_with(' ') {
      return Some(rest.trim());
    }
  }
  None
}

/// Navigates the page to the given URL.
async fn navigate_to_url(target: &CdpTarget, raw_url: &str) -> Result<String, String> {
  let url_trimmed = raw_url.trim();
  if url_trimmed.is_empty() {
    return Err("Missing URL for navigate action".to_string());
  }
  let url = if url_trimmed.starts_with("http://")
    || url_trimmed.starts_with("https://")
    || url_trimmed.starts_with("about:")
    || url_trimmed.starts_with("data:")
    || url_trimmed.starts_with("chrome://")
    || url_trimmed.starts_with("file://")
  {
    url_trimmed.to_string()
  } else {
    format!("https://{url_trimmed}")
  };

  cdp_target::navigate(target, &url, 30)
    .await
    .map_err(|e| format!("Navigation to '{url}' failed: {e}"))?;

  Ok(format!("Navigated to {url}"))
}

/// Finds an element matching `selector` and simulates a click.
async fn click_element(target: &CdpTarget, selector: &str) -> Result<String, String> {
  let sel = selector.trim().trim_matches(|c| c == '\'' || c == '"');
  if sel.is_empty() {
    return Err("Missing CSS selector for click action".to_string());
  }

  let sel_json = serde_json::to_string(sel).map_err(|e| e.to_string())?;
  let js = format!(
    r#"(() => {{
      const el = document.querySelector({sel_json});
      if (!el) throw new Error("Element not found: " + {sel_json});
      el.scrollIntoView({{ block: 'center', inline: 'center', behavior: 'instant' }});
      el.click();
      return true;
    }})()"#
  );

  let result = cdp_target::run_command_awaiting_load(
    target,
    "Runtime.evaluate",
    json!({
      "expression": js,
      "returnByValue": true,
      "awaitPromise": true,
    }),
    10,
    false,
  )
  .await
  .map_err(|e| format!("Failed to click element '{sel}': {e}"))?;

  if let Some(exception) = result.get("exceptionDetails") {
    let msg = exception
      .get("exception")
      .and_then(|e| e.get("description"))
      .or_else(|| exception.get("text"))
      .and_then(|v| v.as_str())
      .unwrap_or("Click failed");
    return Err(format!("Click failed for '{sel}': {msg}"));
  }

  Ok(format!("Clicked element '{sel}'"))
}

/// Types text into an element matching `selector`.
async fn type_into_element(
  target: &CdpTarget,
  selector: &str,
  text: &str,
) -> Result<String, String> {
  let sel = selector.trim().trim_matches(|c| c == '\'' || c == '"');
  if sel.is_empty() {
    return Err("Missing CSS selector for type action".to_string());
  }

  let sel_json = serde_json::to_string(sel).map_err(|e| e.to_string())?;
  let text_json = serde_json::to_string(text).map_err(|e| e.to_string())?;
  let js = format!(
    r#"(() => {{
      const el = document.querySelector({sel_json});
      if (!el) throw new Error("Element not found: " + {sel_json});
      el.focus();
      el.scrollIntoView({{ block: 'center', inline: 'center', behavior: 'instant' }});
      el.value = {text_json};
      el.dispatchEvent(new Event('input', {{ bubbles: true }}));
      el.dispatchEvent(new Event('change', {{ bubbles: true }}));
      return true;
    }})()"#
  );

  let result = cdp_target::run_command(
    target,
    "Runtime.evaluate",
    json!({
      "expression": js,
      "returnByValue": true,
      "awaitPromise": true,
    }),
  )
  .await
  .map_err(|e| format!("Failed to type into '{sel}': {e}"))?;

  if let Some(exception) = result.get("exceptionDetails") {
    let msg = exception
      .get("exception")
      .and_then(|e| e.get("description"))
      .or_else(|| exception.get("text"))
      .and_then(|v| v.as_str())
      .unwrap_or("Typing failed");
    return Err(format!("Typing failed for '{sel}': {msg}"));
  }

  Ok(format!("Typed into '{sel}': \"{text}\""))
}

/// Scrolls the page up or down by the specified number of pixels.
async fn scroll_page(target: &CdpTarget, direction: &str, pixels: i64) -> Result<String, String> {
  let is_up = direction.eq_ignore_ascii_case("up");
  let delta = if is_up { -pixels.abs() } else { pixels.abs() };
  let dir_label = if is_up { "up" } else { "down" };

  let js = format!("window.scrollBy(0, {delta});");
  let result = cdp_target::run_command(
    target,
    "Runtime.evaluate",
    json!({
      "expression": js,
      "returnByValue": true,
    }),
  )
  .await
  .map_err(|e| format!("Failed to scroll page: {e}"))?;

  if let Some(exception) = result.get("exceptionDetails") {
    let msg = exception
      .get("exception")
      .and_then(|e| e.get("description"))
      .or_else(|| exception.get("text"))
      .and_then(|v| v.as_str())
      .unwrap_or("Scroll failed");
    return Err(format!("Scroll failed: {msg}"));
  }

  Ok(format!("Scrolled {dir_label} by {}px", pixels.abs()))
}

/// Extracts text content from element matching `selector`.
async fn extract_element_text(target: &CdpTarget, selector: &str) -> Result<String, String> {
  let sel = selector.trim().trim_matches(|c| c == '\'' || c == '"');
  let sel_clean = if sel.is_empty() { "body" } else { sel };
  let sel_json = serde_json::to_string(sel_clean).map_err(|e| e.to_string())?;

  let js = format!(
    r#"(() => {{
      const el = document.querySelector({sel_json});
      if (!el) throw new Error("Element not found: " + {sel_json});
      return (el.innerText || el.textContent || "").trim();
    }})()"#
  );

  let result = cdp_target::run_command(
    target,
    "Runtime.evaluate",
    json!({
      "expression": js,
      "returnByValue": true,
      "awaitPromise": true,
    }),
  )
  .await
  .map_err(|e| format!("Failed to extract text from '{sel_clean}': {e}"))?;

  if let Some(exception) = result.get("exceptionDetails") {
    let msg = exception
      .get("exception")
      .and_then(|e| e.get("description"))
      .or_else(|| exception.get("text"))
      .and_then(|v| v.as_str())
      .unwrap_or("Text extraction failed");
    return Err(format!("Extract text failed for '{sel_clean}': {msg}"));
  }

  let text = result
    .get("result")
    .and_then(|r| r.get("value"))
    .and_then(|v| v.as_str())
    .unwrap_or("");

  let truncated = if text.chars().count() > 3000 {
    let part: String = text.chars().take(3000).collect();
    format!(
      "{part}...\n[Truncated {} total characters]",
      text.chars().count()
    )
  } else {
    text.to_string()
  };

  Ok(format!("Extracted text from '{sel_clean}':\n{truncated}"))
}

/// Captures a screenshot of the active page and saves it to the cache directory.
async fn capture_page_screenshot(target: &CdpTarget) -> Result<String, String> {
  let shot = cdp_target::run_command(target, "Page.captureScreenshot", json!({ "format": "png" }))
    .await
    .map_err(|e| format!("Failed to capture screenshot: {e}"))?;

  let data = shot
    .get("data")
    .and_then(|v| v.as_str())
    .ok_or_else(|| "CDP returned no image data for screenshot".to_string())?;

  let bytes = base64::engine::general_purpose::STANDARD
    .decode(data)
    .map_err(|e| format!("Failed to decode screenshot base64: {e}"))?;

  let cache_dir = crate::app_dirs::cache_dir().join("agent-screenshots");
  let _ = std::fs::create_dir_all(&cache_dir);
  let filename = format!(
    "screenshot_{}_{}.png",
    chrono::Utc::now().timestamp_millis(),
    &uuid::Uuid::new_v4().to_string()[..8]
  );
  let path = cache_dir.join(&filename);
  std::fs::write(&path, &bytes).map_err(|e| format!("Failed to save screenshot: {e}"))?;

  Ok(format!(
    "Screenshot captured and saved to file://{} ({} KB PNG)",
    path.display(),
    bytes.len() / 1024
  ))
}

/// Evaluates arbitrary JavaScript in the page context.
async fn evaluate_javascript(target: &CdpTarget, expression: &str) -> Result<String, String> {
  let expr = expression.trim();
  if expr.is_empty() {
    return Err("Missing javascript expression for eval action".to_string());
  }

  let result = cdp_target::run_command(
    target,
    "Runtime.evaluate",
    json!({
      "expression": expr,
      "returnByValue": true,
      "awaitPromise": true,
    }),
  )
  .await
  .map_err(|e| format!("JavaScript evaluation error: {e}"))?;

  if let Some(exception) = result.get("exceptionDetails") {
    let msg = exception
      .get("exception")
      .and_then(|e| e.get("description"))
      .or_else(|| exception.get("text"))
      .and_then(|v| v.as_str())
      .unwrap_or("Evaluation failed");
    return Err(format!("JavaScript exception: {msg}"));
  }

  let val_str = if let Some(r) = result.get("result") {
    if let Some(val) = r.get("value") {
      if val.is_string() {
        val.as_str().unwrap().to_string()
      } else {
        val.to_string()
      }
    } else if let Some(desc) = r.get("description").and_then(|v| v.as_str()) {
      desc.to_string()
    } else {
      "undefined".to_string()
    }
  } else {
    "undefined".to_string()
  };

  Ok(format!("JS Result: {val_str}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_strip_command_prefix() {
    assert_eq!(
      strip_command_prefix("navigate: https://example.com", "navigate"),
      Some("https://example.com")
    );
    assert_eq!(
      strip_command_prefix("Navigate: https://example.com", "navigate"),
      Some("https://example.com")
    );
    assert_eq!(
      strip_command_prefix("NAVIGATE: https://example.com", "navigate"),
      Some("https://example.com")
    );
    assert_eq!(
      strip_command_prefix("click button.submit", "click"),
      Some("button.submit")
    );
    assert_eq!(strip_command_prefix("click: #id", "click"), Some("#id"));
    assert_eq!(
      strip_command_prefix("type: input | hello", "type"),
      Some("input | hello")
    );
    assert_eq!(
      strip_command_prefix("scroll: down 500", "scroll"),
      Some("down 500")
    );
    assert_eq!(strip_command_prefix("eval: 1 + 1", "eval"), Some("1 + 1"));
    assert_eq!(strip_command_prefix("something else", "click"), None);
  }
}
