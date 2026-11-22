import { invoke } from "@tauri-apps/api/core";
import type { AiProvider, AiSettings } from "@/types";

export const DEFAULT_AI_SETTINGS: AiSettings = {
  provider: "openai",
  apiKey: "",
  baseUrl: "https://api.openai.com/v1",
  model: "gpt-4o",
  temperature: 0.7,
  maxTokens: 4096,
  savedKeys: {},
  savedModels: {
    openai: "gpt-4o",
    anthropic: "claude-3-7-sonnet-20250219",
    gemini: "gemini-2.0-flash",
    openrouter: "anthropic/claude-3.5-sonnet",
    ollama: "llama3.2",
    custom: "gpt-4o",
  },
  savedBaseUrls: {
    openai: "https://api.openai.com/v1",
    anthropic: "https://api.anthropic.com/v1",
    gemini: "https://generativelanguage.googleapis.com",
    openrouter: "https://openrouter.ai/api/v1",
    ollama: "http://localhost:11434/v1",
    custom: "https://api.openai.com/v1",
  },
};

export const getAiSettings = () => invoke<AiSettings>("get_ai_settings");

export const saveAiSettings = (settings: AiSettings) =>
  invoke<void>("save_ai_settings", { settings });

export const fetchAiModels = (
  provider: AiProvider,
  apiKey: string,
  baseUrl?: string,
) =>
  invoke<string[]>("fetch_ai_models", {
    provider,
    apiKey,
    baseUrl: baseUrl || null,
  });
