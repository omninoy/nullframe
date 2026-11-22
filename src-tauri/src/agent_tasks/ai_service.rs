//! Third-party AI integration for NullFrame Agent.
//! Provides configurable providers (OpenAI, Anthropic Claude, Google Gemini, OpenRouter, Ollama, Custom),
//! dynamic model discovery, official NullFrame Agent system prompt, and turn execution.

use super::chat;
use super::model::{Step, TaskStatus};
use super::now_ms;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

pub const DEFAULT_NULLFRAME_SYSTEM_PROMPT: &str = r#"Ты — официальный автономный ИИ-агент браузера NullFrame (NullFrame AI Agent).

Твоя роль и компетенции:
- Ты помогаешь пользователю управлять антидетект-профилями, настраивать прокси, автоматизировать рутинные браузерные действия, анализировать страницы и выполнять веб-задачи.
- Ты глубоко разбираешься в архитектуре NullFrame:
  1. Профили браузера: изолированные инстансы с уникальными отпечатками (Canvas, WebGL, Audio, ClientRects, WebRTC, OS headers), отдельными cookies, LocalStorage и индивидуальными прокси (HTTP, SOCKS5, SSH).
  2. Рутины (Routines): шаблоны и сценарии автоматизации, которые можно запускать на одном или нескольких профилях.
  3. Пакетные запуски (Batch runs): параллельное выполнение автоматизации на группе профилей.
  4. Сеть и безопасность: проверка пинга, валидация прокси и туннелей.

Формат мышления и действий:
При решении сложных задач и инструкций пользователя разбивай процесс на четкие шаги:
1. Если требуется обдумать план, сформулировать гипотезу или шаги — оборачивай свои внутренние размышления в тег <think>...</think>.
2. Если ты планируешь или выполняешь конкретное действие в браузере — оборачивай его в тег <act>...</act>.
   Поддерживаемые браузерные команды (CDP):
   - navigate: <url> — перейти по указанному URL (например: <act>navigate: https://example.com</act>)
   - click: <selector> — нажать на элемент по CSS-селектору (например: <act>click: button.submit</act>)
   - type: <selector> | <text> — ввести текст в поле (например: <act>type: input[name="q"] | NullFrame</act>)
   - scroll: <up|down> <pixels> — прокрутить страницу вверх или вниз (например: <act>scroll: down 500</act>)
   - extract_text: <selector> — извлечь текстовое содержимое элемента (например: <act>extract_text: h1</act>)
   - screenshot — сделать снимок экрана страницы (например: <act>screenshot</act>)
   - eval: <javascript> — выполнить произвольный JS в контексте страницы (например: <act>eval: document.title</act>)
3. Если ты анализируешь результат страницы или полученные данные — оборачивай наблюдения в тег <observe>...</observe>.
4. Итоговый ответ формулируй четко, профессионально и структурированно в Markdown вне этих тегов.
5. Всегда отвечай на том языке, на котором к тебе обратился пользователь (на русском, если запрос на русском)."#;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettings {
  pub provider: String,
  pub api_key: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub base_url: Option<String>,
  pub model: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub custom_system_prompt: Option<String>,
  pub temperature: Option<f32>,
  pub max_tokens: Option<u32>,
  #[serde(default)]
  pub saved_keys: HashMap<String, String>,
  #[serde(default)]
  pub saved_models: HashMap<String, String>,
  #[serde(default)]
  pub saved_base_urls: HashMap<String, String>,
}

impl Default for AiSettings {
  fn default() -> Self {
    let mut saved_models = HashMap::new();
    saved_models.insert("openai".to_string(), "gpt-4o".to_string());
    saved_models.insert(
      "anthropic".to_string(),
      "claude-3-7-sonnet-20250219".to_string(),
    );
    saved_models.insert("gemini".to_string(), "gemini-2.0-flash".to_string());
    saved_models.insert(
      "openrouter".to_string(),
      "anthropic/claude-3.5-sonnet".to_string(),
    );
    saved_models.insert("ollama".to_string(), "llama3.2".to_string());
    saved_models.insert("custom".to_string(), "gpt-4o".to_string());

    let mut saved_base_urls = HashMap::new();
    saved_base_urls.insert(
      "openai".to_string(),
      "https://api.openai.com/v1".to_string(),
    );
    saved_base_urls.insert(
      "anthropic".to_string(),
      "https://api.anthropic.com/v1".to_string(),
    );
    saved_base_urls.insert(
      "gemini".to_string(),
      "https://generativelanguage.googleapis.com".to_string(),
    );
    saved_base_urls.insert(
      "openrouter".to_string(),
      "https://openrouter.ai/api/v1".to_string(),
    );
    saved_base_urls.insert(
      "ollama".to_string(),
      "http://localhost:11434/v1".to_string(),
    );
    saved_base_urls.insert(
      "custom".to_string(),
      "https://api.openai.com/v1".to_string(),
    );

    Self {
      provider: "openai".to_string(),
      api_key: String::new(),
      base_url: Some("https://api.openai.com/v1".to_string()),
      model: "gpt-4o".to_string(),
      custom_system_prompt: None,
      temperature: Some(0.7),
      max_tokens: Some(4096),
      saved_keys: HashMap::new(),
      saved_models,
      saved_base_urls,
    }
  }
}

static CACHED_SETTINGS: LazyLock<Mutex<Option<AiSettings>>> = LazyLock::new(|| Mutex::new(None));

fn settings_file() -> PathBuf {
  crate::app_dirs::settings_dir().join("ai_settings.json")
}

pub fn get_settings() -> AiSettings {
  if let Ok(guard) = CACHED_SETTINGS.lock() {
    if let Some(settings) = guard.as_ref() {
      return settings.clone();
    }
  }
  let path = settings_file();
  let loaded = if let Ok(bytes) = std::fs::read(&path) {
    serde_json::from_slice::<AiSettings>(&bytes).unwrap_or_default()
  } else {
    AiSettings::default()
  };
  if let Ok(mut guard) = CACHED_SETTINGS.lock() {
    *guard = Some(loaded.clone());
  }
  loaded
}

pub fn save_settings(settings: AiSettings) -> Result<(), String> {
  let path = settings_file();
  if let Some(parent) = path.parent() {
    let _ = std::fs::create_dir_all(parent);
  }
  let data = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
  std::fs::write(&path, data).map_err(|e| e.to_string())?;
  if let Ok(mut guard) = CACHED_SETTINGS.lock() {
    *guard = Some(settings);
  }
  Ok(())
}

fn fallback_models(provider: &str) -> Vec<String> {
  match provider {
    "openai" => vec![
      "gpt-4o".to_string(),
      "gpt-4o-mini".to_string(),
      "o1".to_string(),
      "o3-mini".to_string(),
      "gpt-4-turbo".to_string(),
      "gpt-3.5-turbo".to_string(),
    ],
    "anthropic" => vec![
      "claude-3-7-sonnet-20250219".to_string(),
      "claude-3-5-sonnet-20241022".to_string(),
      "claude-3-5-haiku-20241022".to_string(),
      "claude-3-opus-20240229".to_string(),
    ],
    "gemini" => vec![
      "gemini-2.0-flash".to_string(),
      "gemini-2.0-flash-lite".to_string(),
      "gemini-1.5-pro".to_string(),
      "gemini-1.5-flash".to_string(),
    ],
    "openrouter" => vec![
      "anthropic/claude-3.5-sonnet".to_string(),
      "openai/gpt-4o".to_string(),
      "deepseek/deepseek-chat".to_string(),
      "google/gemini-2.0-flash-001".to_string(),
      "meta-llama/llama-3.3-70b-instruct".to_string(),
    ],
    "ollama" => vec![
      "llama3.3".to_string(),
      "llama3.2".to_string(),
      "qwen2.5".to_string(),
      "deepseek-r1".to_string(),
      "mistral".to_string(),
    ],
    _ => vec![
      "gpt-4o".to_string(),
      "gpt-4o-mini".to_string(),
      "claude-3-5-sonnet".to_string(),
      "deepseek-chat".to_string(),
    ],
  }
}

pub async fn fetch_models(
  provider: &str,
  api_key: &str,
  base_url: Option<&str>,
) -> Result<Vec<String>, String> {
  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(12))
    .build()
    .map_err(|e| e.to_string())?;

  let trimmed_key = api_key.trim();

  let fetched: Result<Vec<String>, String> = match provider {
    "openai" | "openrouter" | "custom" => {
      let default_url = if provider == "openrouter" {
        "https://openrouter.ai/api/v1"
      } else {
        "https://api.openai.com/v1"
      };
      let base = base_url.unwrap_or(default_url).trim_end_matches('/');
      let url = format!("{base}/models");

      let mut req = client.get(&url);
      if !trimmed_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {trimmed_key}"));
      }
      if provider == "openrouter" {
        req = req
          .header("HTTP-Referer", "https://nullframe.app")
          .header("X-Title", "NullFrame");
      }

      let resp = req.send().await.map_err(|e| e.to_string())?;
      if !resp.status().is_success() {
        return Err(format!("API returned status {}", resp.status()));
      }
      let body: Value = resp.json().await.map_err(|e| e.to_string())?;
      let mut models = Vec::new();
      if let Some(data) = body.get("data").and_then(Value::as_array) {
        for item in data {
          if let Some(id) = item.get("id").and_then(Value::as_str) {
            models.push(id.to_string());
          }
        }
      }
      models.sort();
      Ok(models)
    }
    "anthropic" => {
      let base = base_url
        .unwrap_or("https://api.anthropic.com/v1")
        .trim_end_matches('/');
      let url = format!("{base}/models");

      let resp = client
        .get(&url)
        .header("x-api-key", trimmed_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| e.to_string())?;

      if !resp.status().is_success() {
        return Err(format!("Anthropic API returned status {}", resp.status()));
      }
      let body: Value = resp.json().await.map_err(|e| e.to_string())?;
      let mut models = Vec::new();
      if let Some(data) = body.get("data").and_then(Value::as_array) {
        for item in data {
          if let Some(id) = item.get("id").and_then(Value::as_str) {
            models.push(id.to_string());
          }
        }
      }
      models.sort();
      Ok(models)
    }
    "gemini" => {
      let url =
        format!("https://generativelanguage.googleapis.com/v1beta/models?key={trimmed_key}");
      let resp = client.get(&url).send().await.map_err(|e| e.to_string())?;
      if !resp.status().is_success() {
        return Err(format!("Gemini API returned status {}", resp.status()));
      }
      let body: Value = resp.json().await.map_err(|e| e.to_string())?;
      let mut models = Vec::new();
      if let Some(list) = body.get("models").and_then(Value::as_array) {
        for item in list {
          if let Some(name) = item.get("name").and_then(Value::as_str) {
            let clean = name.strip_prefix("models/").unwrap_or(name);
            let supported = item
              .get("supportedGenerationMethods")
              .and_then(Value::as_array)
              .map(|arr| arr.iter().any(|m| m.as_str() == Some("generateContent")))
              .unwrap_or(true);
            if supported {
              models.push(clean.to_string());
            }
          }
        }
      }
      models.sort();
      Ok(models)
    }
    "ollama" => {
      let base = base_url
        .unwrap_or("http://localhost:11434/v1")
        .trim_end_matches('/');
      // Try /api/tags first (native ollama), then /models (openai compat)
      let tags_url = if base.ends_with("/v1") {
        format!("{}/api/tags", base.strip_suffix("/v1").unwrap())
      } else {
        format!("{base}/api/tags")
      };

      if let Ok(resp) = client.get(&tags_url).send().await {
        if resp.status().is_success() {
          if let Ok(body) = resp.json::<Value>().await {
            if let Some(models_arr) = body.get("models").and_then(Value::as_array) {
              let mut names = Vec::new();
              for m in models_arr {
                if let Some(n) = m.get("name").and_then(Value::as_str) {
                  names.push(n.to_string());
                }
              }
              if !names.is_empty() {
                names.sort();
                return Ok(names);
              }
            }
          }
        }
      }

      let models_url = format!("{base}/models");
      let resp = client
        .get(&models_url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
      let body: Value = resp.json().await.map_err(|e| e.to_string())?;
      let mut models = Vec::new();
      if let Some(data) = body.get("data").and_then(Value::as_array) {
        for item in data {
          if let Some(id) = item.get("id").and_then(Value::as_str) {
            models.push(id.to_string());
          }
        }
      }
      models.sort();
      Ok(models)
    }
    _ => Err("Unknown provider".to_string()),
  };

  match fetched {
    Ok(list) if !list.is_empty() => Ok(list),
    _ => Ok(fallback_models(provider)),
  }
}

struct ParsedResponse {
  thoughts: Vec<String>,
  actions: Vec<(String, String)>,
  observations: Vec<String>,
  clean_text: String,
}

fn extract_tag_blocks(input: &str, tag: &str) -> (Vec<String>, String) {
  let open_tag = format!("<{tag}>");
  let close_tag = format!("</{tag}>");
  let lower_open = open_tag.to_lowercase();
  let lower_close = close_tag.to_lowercase();

  let mut items = Vec::new();
  let mut clean = String::new();
  let mut cursor = 0;

  while cursor < input.len() {
    let slice_lower = input[cursor..].to_lowercase();
    if let Some(open_rel) = slice_lower.find(&lower_open) {
      let open_pos = cursor + open_rel;
      clean.push_str(&input[cursor..open_pos]);
      let content_start = open_pos + open_tag.len();

      let after_lower = input[content_start..].to_lowercase();
      if let Some(close_rel) = after_lower.find(&lower_close) {
        let content_end = content_start + close_rel;
        let content = input[content_start..content_end].trim();
        if !content.is_empty() {
          items.push(content.to_string());
        }
        cursor = content_end + close_tag.len();
      } else {
        // Tag not closed; consume the rest
        let content = input[content_start..].trim();
        if !content.is_empty() {
          items.push(content.to_string());
        }
        cursor = input.len();
      }
    } else {
      clean.push_str(&input[cursor..]);
      break;
    }
  }

  (items, clean)
}

fn parse_llm_output(raw: &str) -> ParsedResponse {
  let (thoughts, rem1) = extract_tag_blocks(raw, "think");
  let (raw_actions, rem2) = extract_tag_blocks(&rem1, "act");
  let (observations, rem3) = extract_tag_blocks(&rem2, "observe");

  let mut actions = Vec::new();
  for act in raw_actions {
    actions.push(("action".to_string(), act));
  }

  let clean_text = rem3.trim().to_string();

  ParsedResponse {
    thoughts,
    actions,
    observations,
    clean_text,
  }
}

pub async fn run_ai_turn(chat_id: String, turn_id: String) {
  let settings = get_settings();

  let (profile_id, text, history) = {
    let store = chat::chats();
    let Ok(chat) = store.get(&chat_id) else {
      return;
    };
    let Some(current_turn) = chat.turns.iter().find(|t| t.id == turn_id) else {
      return;
    };

    let mut hist = Vec::new();
    for turn in &chat.turns {
      if turn.id == turn_id {
        break;
      }
      hist.push(("user".to_string(), turn.text.clone()));
      if let Some(result) = &turn.task.result {
        hist.push(("assistant".to_string(), result.clone()));
      }
    }
    (chat.profile_id.clone(), current_turn.text.clone(), hist)
  };

  // If no API key configured and provider is not ollama
  if settings.provider != "ollama" && settings.api_key.trim().is_empty() {
    let mut store = chat::chats();
    if let Ok(chat) = store.get_mut(&chat_id) {
      if let Some(turn) = chat.turns.iter_mut().find(|t| t.id == turn_id) {
        turn.task.status = TaskStatus::Done;
        turn.task.finished_at = Some(now_ms());
        turn.task.steps.push_back(Step {
          index: 0,
          kind: "observe".to_string(),
          text: "Сторонний API ключ не настроен".to_string(),
          url: None,
          tool: None,
          at: now_ms(),
        });
        turn.task.result = Some(
          "👋 Для работы с агентом необходимо указать API ключ сторонней модели.\n\n\
           Поддерживаются: **OpenAI**, **Anthropic Claude**, **Google Gemini**, **OpenRouter**, **Ollama** (локально) и любые OpenAI-совместимые API.\n\n\
           Нажмите на плашку выбора модели в верхней части экрана или откройте настройки агента, чтобы ввести ключ и выбрать модель."
            .to_string(),
        );
      }
      store.changed(&chat_id, false);
    }
    return;
  }

  // Mark running
  {
    let mut store = chat::chats();
    if let Ok(chat) = store.get_mut(&chat_id) {
      if let Some(turn) = chat.turns.iter_mut().find(|t| t.id == turn_id) {
        turn.task.status = TaskStatus::Running;
        turn.task.started_at = Some(now_ms());
        turn.task.task_id = Some(uuid::Uuid::new_v4().to_string());
        turn.task.steps.push_back(Step {
          index: 0,
          kind: "think".to_string(),
          text: format!(
            "Подключение к {} ({}) и подготовка ответа...",
            settings.provider, settings.model
          ),
          url: None,
          tool: None,
          at: now_ms(),
        });
      }
      store.changed(&chat_id, false);
    }
  }

  let system_prompt = settings
    .custom_system_prompt
    .as_deref()
    .filter(|s| !s.trim().is_empty())
    .unwrap_or(DEFAULT_NULLFRAME_SYSTEM_PROMPT);

  let client = reqwest::Client::builder()
    .timeout(Duration::from_secs(90))
    .build()
    .unwrap_or_default();

  let outcome: Result<String, String> = async {
    let key = settings.api_key.trim();
    let model = settings.model.trim();

    match settings.provider.as_str() {
      "openai" | "openrouter" | "ollama" | "custom" => {
        let default_url = if settings.provider == "openrouter" {
          "https://openrouter.ai/api/v1"
        } else if settings.provider == "ollama" {
          "http://localhost:11434/v1"
        } else {
          "https://api.openai.com/v1"
        };
        let base = settings
          .base_url
          .as_deref()
          .unwrap_or(default_url)
          .trim_end_matches('/');
        let url = format!("{base}/chat/completions");

        let mut messages = Vec::new();
        messages.push(json!({ "role": "system", "content": system_prompt }));
        for (role, msg) in &history {
          messages.push(json!({ "role": role, "content": msg }));
        }
        messages.push(json!({ "role": "user", "content": text }));

        let mut req = client.post(&url);
        if !key.is_empty() {
          req = req.header("Authorization", format!("Bearer {key}"));
        }
        if settings.provider == "openrouter" {
          req = req
            .header("HTTP-Referer", "https://nullframe.app")
            .header("X-Title", "NullFrame");
        }

        let body = json!({
          "model": model,
          "messages": messages,
          "temperature": settings.temperature.unwrap_or(0.7),
          "max_tokens": settings.max_tokens.unwrap_or(4096),
        });

        let resp = req.json(&body).send().await.map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
          let err_text = resp.text().await.unwrap_or_default();
          return Err(format!("API Error ({status}): {err_text}"));
        }

        let resp_json: Value = resp.json().await.map_err(|e| e.to_string())?;
        let answer = resp_json["choices"][0]["message"]["content"]
          .as_str()
          .ok_or_else(|| "Empty or invalid response from AI model".to_string())?;
        Ok(answer.to_string())
      }
      "anthropic" => {
        let base = settings
          .base_url
          .as_deref()
          .unwrap_or("https://api.anthropic.com/v1")
          .trim_end_matches('/');
        let url = format!("{base}/messages");

        let mut messages = Vec::new();
        for (role, msg) in &history {
          messages.push(json!({ "role": role, "content": msg }));
        }
        messages.push(json!({ "role": "user", "content": text }));

        let body = json!({
          "model": model,
          "system": system_prompt,
          "messages": messages,
          "max_tokens": settings.max_tokens.unwrap_or(4096),
          "temperature": settings.temperature.unwrap_or(0.7),
        });

        let resp = client
          .post(&url)
          .header("x-api-key", key)
          .header("anthropic-version", "2023-06-01")
          .json(&body)
          .send()
          .await
          .map_err(|e| e.to_string())?;

        let status = resp.status();
        if !status.is_success() {
          let err_text = resp.text().await.unwrap_or_default();
          return Err(format!("Anthropic API Error ({status}): {err_text}"));
        }

        let resp_json: Value = resp.json().await.map_err(|e| e.to_string())?;
        let text_part = resp_json["content"]
          .as_array()
          .and_then(|arr| arr.iter().find(|part| part["type"] == "text"))
          .and_then(|part| part["text"].as_str())
          .ok_or_else(|| "Empty text from Anthropic response".to_string())?;
        Ok(text_part.to_string())
      }
      "gemini" => {
        let url = format!(
          "https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={key}"
        );

        let mut contents = Vec::new();
        for (role, msg) in &history {
          let g_role = if role == "assistant" { "model" } else { "user" };
          contents.push(json!({
            "role": g_role,
            "parts": [{ "text": msg }]
          }));
        }
        contents.push(json!({
          "role": "user",
          "parts": [{ "text": text }]
        }));

        let body = json!({
          "system_instruction": {
            "parts": [{ "text": system_prompt }]
          },
          "contents": contents,
          "generationConfig": {
            "temperature": settings.temperature.unwrap_or(0.7),
            "maxOutputTokens": settings.max_tokens.unwrap_or(4096),
          }
        });

        let resp = client
          .post(&url)
          .json(&body)
          .send()
          .await
          .map_err(|e| e.to_string())?;

        let status = resp.status();
        if !status.is_success() {
          let err_text = resp.text().await.unwrap_or_default();
          return Err(format!("Gemini API Error ({status}): {err_text}"));
        }

        let resp_json: Value = resp.json().await.map_err(|e| e.to_string())?;
        let part_text = resp_json["candidates"][0]["content"]["parts"][0]["text"]
          .as_str()
          .ok_or_else(|| "Empty text from Gemini response".to_string())?;
        Ok(part_text.to_string())
      }
      _ => Err(format!("Неизвестный провайдер: {}", settings.provider)),
    }
  }
  .await;

  match outcome {
    Ok(raw_text) => {
      let parsed = parse_llm_output(&raw_text);
      let mut step_index = {
        let store = chat::chats();
        if let Ok(chat) = store.get(&chat_id) {
          if let Some(turn) = chat.turns.iter().find(|t| t.id == turn_id) {
            turn.task.steps.len() as u64
          } else {
            1u64
          }
        } else {
          1u64
        }
      };

      // Add thoughts
      if !parsed.thoughts.is_empty() {
        let mut store = chat::chats();
        if let Ok(chat) = store.get_mut(&chat_id) {
          if let Some(turn) = chat.turns.iter_mut().find(|t| t.id == turn_id) {
            for thought in &parsed.thoughts {
              turn.task.steps.push_back(Step {
                index: step_index,
                kind: "think".to_string(),
                text: thought.clone(),
                url: None,
                tool: None,
                at: now_ms(),
              });
              step_index += 1;
            }
            store.changed(&chat_id, false);
          }
        }
      }

      // Execute and record actions via browser CDP executor
      let target_profile_id = profile_id.as_str();
      for (tool, action_text) in &parsed.actions {
        // Record the action step
        {
          let mut store = chat::chats();
          if let Ok(chat) = store.get_mut(&chat_id) {
            if let Some(turn) = chat.turns.iter_mut().find(|t| t.id == turn_id) {
              turn.task.steps.push_back(Step {
                index: step_index,
                kind: "act".to_string(),
                text: action_text.clone(),
                url: None,
                tool: Some(tool.clone()),
                at: now_ms(),
              });
              step_index += 1;
              store.changed(&chat_id, false);
            }
          }
        }

        // Execute CDP browser action
        let obs_text =
          match super::cdp_executor::execute_browser_action(target_profile_id, action_text).await {
            Ok(obs) => obs,
            Err(err) => format!("CDP Action Error: {err}"),
          };

        // Record the observation step
        {
          let mut store = chat::chats();
          if let Ok(chat) = store.get_mut(&chat_id) {
            if let Some(turn) = chat.turns.iter_mut().find(|t| t.id == turn_id) {
              turn.task.steps.push_back(Step {
                index: step_index,
                kind: "observe".to_string(),
                text: obs_text,
                url: None,
                tool: None,
                at: now_ms(),
              });
              step_index += 1;
              store.changed(&chat_id, false);
            }
          }
        }
      }

      // Add observations from LLM output
      if !parsed.observations.is_empty() {
        let mut store = chat::chats();
        if let Ok(chat) = store.get_mut(&chat_id) {
          if let Some(turn) = chat.turns.iter_mut().find(|t| t.id == turn_id) {
            for obs in &parsed.observations {
              turn.task.steps.push_back(Step {
                index: step_index,
                kind: "observe".to_string(),
                text: obs.clone(),
                url: None,
                tool: None,
                at: now_ms(),
              });
              step_index += 1;
            }
            store.changed(&chat_id, false);
          }
        }
      }

      let final_result = if !parsed.clean_text.is_empty() {
        parsed.clean_text
      } else {
        raw_text
      };

      {
        let mut store = chat::chats();
        if let Ok(chat) = store.get_mut(&chat_id) {
          if let Some(turn) = chat.turns.iter_mut().find(|t| t.id == turn_id) {
            let now = now_ms();
            turn.task.status = TaskStatus::Done;
            turn.task.result = Some(final_result);
            turn.task.credits_used += 1;
            turn.task.finished_at = Some(now);
            chat.updated_at = now;
            store.changed(&chat_id, false);
          }
        }
      }
    }
    Err(err) => {
      let mut store = chat::chats();
      if let Ok(chat) = store.get_mut(&chat_id) {
        if let Some(turn) = chat.turns.iter_mut().find(|t| t.id == turn_id) {
          let now = now_ms();
          turn.task.status = TaskStatus::Failed;
          turn.task.error = Some(err);
          turn.task.finished_at = Some(now);
          chat.updated_at = now;
          store.changed(&chat_id, false);
        }
      }
    }
  }
}

#[tauri::command]
pub async fn get_ai_settings() -> Result<AiSettings, String> {
  Ok(get_settings())
}

#[tauri::command]
pub async fn save_ai_settings(settings: AiSettings) -> Result<(), String> {
  save_settings(settings)
}

#[tauri::command]
pub async fn fetch_ai_models(
  provider: String,
  api_key: String,
  base_url: Option<String>,
) -> Result<Vec<String>, String> {
  fetch_models(&provider, &api_key, base_url.as_deref()).await
}
