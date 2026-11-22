"use client";

import { useCallback, useEffect, useState } from "react";
import {
  LuBot,
  LuCheck,
  LuChevronDown,
  LuChevronUp,
  LuEye,
  LuEyeOff,
  LuRefreshCw,
  LuSparkles,
} from "react-icons/lu";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import {
  DEFAULT_AI_SETTINGS,
  fetchAiModels,
  getAiSettings,
  saveAiSettings,
} from "@/lib/ai-settings";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import type { AiProvider, AiSettings } from "@/types";

interface AiSettingsDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onSettingsSaved?: (settings: AiSettings) => void;
}

const PROVIDERS: {
  id: AiProvider;
  name: string;
  desc: string;
  icon: string;
}[] = [
  { id: "openai", name: "OpenAI", desc: "GPT-4o, o1, o3-mini", icon: "🟢" },
  {
    id: "anthropic",
    name: "Anthropic Claude",
    desc: "Claude 3.7 Sonnet, 3.5 Haiku",
    icon: "🟣",
  },
  {
    id: "gemini",
    name: "Google Gemini",
    desc: "Gemini 2.0 Flash, 1.5 Pro",
    icon: "🔵",
  },
  {
    id: "openrouter",
    name: "OpenRouter",
    desc: "Все модели через единый API",
    icon: "🌐",
  },
  {
    id: "ollama",
    name: "Ollama / Локально",
    desc: "Локальные модели на ПК",
    icon: "🦙",
  },
  {
    id: "custom",
    name: "Пользовательский (OpenAI API)",
    desc: "Любой OpenAI-совместимый эндпоинт",
    icon: "⚙️",
  },
];

export function AiSettingsDialog({
  open,
  onOpenChange,
  onSettingsSaved,
}: AiSettingsDialogProps) {
  const [settings, setSettings] = useState<AiSettings>(DEFAULT_AI_SETTINGS);
  const [_loading, setLoading] = useState(false);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [showKey, setShowKey] = useState(false);
  const [customModelMode, setCustomModelMode] = useState(false);
  const [showPromptDetails, setShowPromptDetails] = useState(false);

  const loadModelsFor = useCallback(
    async (
      provider: AiProvider,
      apiKey: string,
      baseUrl?: string,
      currentModel?: string,
    ) => {
      setFetchingModels(true);
      try {
        const models = await fetchAiModels(provider, apiKey, baseUrl);
        if (models && models.length > 0) {
          if (currentModel && !models.includes(currentModel)) {
            setAvailableModels([currentModel, ...models]);
          } else {
            setAvailableModels(models);
          }
        }
      } catch (err: unknown) {
        console.warn("Failed to fetch models:", err);
        showErrorToast(
          "Не удалось автоматически загрузить модели. Проверьте ключ или введите модель вручную.",
        );
      } finally {
        setFetchingModels(false);
      }
    },
    [],
  );

  useEffect(() => {
    if (open) {
      setLoading(true);
      getAiSettings()
        .then((s) => {
          setSettings(s);
          // Initial model fetch or fallback
          if (s.provider === "ollama" || s.apiKey.trim()) {
            void loadModelsFor(s.provider, s.apiKey, s.baseUrl, s.model);
          } else {
            setAvailableModels([s.model]);
          }
        })
        .catch(() => {
          setSettings(DEFAULT_AI_SETTINGS);
        })
        .finally(() => {
          setLoading(false);
        });
    }
  }, [open, loadModelsFor]);

  const handleProviderChange = (newProvider: AiProvider) => {
    // Preserve current key, model, baseUrl into saved collections
    const updatedKeys = {
      ...settings.savedKeys,
      [settings.provider]: settings.apiKey,
    };
    const updatedModels = {
      ...settings.savedModels,
      [settings.provider]: settings.model,
    };
    const updatedUrls = {
      ...settings.savedBaseUrls,
      ...(settings.baseUrl ? { [settings.provider]: settings.baseUrl } : {}),
    };

    const nextKey = updatedKeys[newProvider] ?? "";
    const nextModel =
      updatedModels[newProvider] ??
      DEFAULT_AI_SETTINGS.savedModels[newProvider] ??
      "gpt-4o";
    const nextUrl =
      updatedUrls[newProvider] ??
      DEFAULT_AI_SETTINGS.savedBaseUrls[newProvider] ??
      "";

    const nextSettings: AiSettings = {
      ...settings,
      provider: newProvider,
      apiKey: nextKey,
      model: nextModel,
      baseUrl: nextUrl,
      savedKeys: updatedKeys,
      savedModels: updatedModels,
      savedBaseUrls: updatedUrls,
    };

    setSettings(nextSettings);
    setCustomModelMode(false);
    void loadModelsFor(newProvider, nextKey, nextUrl, nextModel);
  };

  const handleSave = async () => {
    try {
      const finalSettings = {
        ...settings,
        savedKeys: {
          ...settings.savedKeys,
          [settings.provider]: settings.apiKey,
        },
        savedModels: {
          ...settings.savedModels,
          [settings.provider]: settings.model,
        },
        savedBaseUrls: {
          ...settings.savedBaseUrls,
          ...(settings.baseUrl
            ? { [settings.provider]: settings.baseUrl }
            : {}),
        },
      };
      await saveAiSettings(finalSettings);
      showSuccessToast("Настройки ИИ-модели успешно сохранены");
      onSettingsSaved?.(finalSettings);
      onOpenChange(false);
    } catch (err: unknown) {
      showErrorToast(`Ошибка сохранения: ${String(err)}`);
    }
  };

  const currentProviderInfo =
    PROVIDERS.find((p) => p.id === settings.provider) ?? PROVIDERS[0];

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-2xl max-h-[90vh] overflow-y-auto">
        <DialogHeader>
          <div className="flex items-center gap-2">
            <div className="flex size-9 items-center justify-center rounded-lg bg-primary/10 text-primary">
              <LuBot className="size-5" />
            </div>
            <div>
              <DialogTitle className="text-lg font-semibold">
                Подключение сторонней ИИ-модели
              </DialogTitle>
              <DialogDescription className="text-xs text-muted-foreground">
                Агент NullFrame работает через сторонние API без привязки к
                облаку
              </DialogDescription>
            </div>
          </div>
        </DialogHeader>

        <div className="flex flex-col gap-4 py-2 text-sm">
          {/* Provider Selection */}
          <div className="flex flex-col gap-1.5">
            <Label className="text-xs font-medium">Провайдер API</Label>
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-2">
              {PROVIDERS.map((p) => {
                const active = settings.provider === p.id;
                return (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => handleProviderChange(p.id)}
                    className={`flex flex-col items-start gap-1 p-2.5 rounded-lg border text-left transition-colors ${
                      active
                        ? "border-primary bg-primary/5 text-foreground ring-1 ring-primary"
                        : "border-border bg-card/50 hover:bg-muted text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    <div className="flex items-center gap-1.5 text-xs font-medium">
                      <span>{p.icon}</span>
                      <span>{p.name}</span>
                    </div>
                    <span className="text-[11px] text-muted-foreground truncate w-full">
                      {p.desc}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          {/* API Key */}
          {settings.provider !== "ollama" && (
            <div className="flex flex-col gap-1.5">
              <div className="flex items-center justify-between">
                <Label className="text-xs font-medium">
                  API Ключ ({currentProviderInfo.name})
                </Label>
                <span className="text-[11px] text-muted-foreground">
                  Сохраняется локально на вашем ПК
                </span>
              </div>
              <div className="relative">
                <Input
                  type={showKey ? "text" : "password"}
                  placeholder={
                    settings.provider === "openai"
                      ? "sk-..."
                      : settings.provider === "anthropic"
                        ? "sk-ant-..."
                        : settings.provider === "gemini"
                          ? "AIza..."
                          : "Вставьте ваш API ключ"
                  }
                  value={settings.apiKey}
                  onChange={(e) =>
                    setSettings({ ...settings, apiKey: e.target.value })
                  }
                  className="pr-10 font-mono text-xs"
                />
                <button
                  type="button"
                  onClick={() => setShowKey(!showKey)}
                  className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                >
                  {showKey ? (
                    <LuEyeOff className="size-4" />
                  ) : (
                    <LuEye className="size-4" />
                  )}
                </button>
              </div>
            </div>
          )}

          {/* Base URL (Optional / Custom) */}
          <div className="flex flex-col gap-1.5">
            <Label className="text-xs font-medium">
              Базовый URL эндпоинта (Base URL)
            </Label>
            <Input
              type="text"
              placeholder={
                settings.provider === "openai"
                  ? "https://api.openai.com/v1"
                  : settings.provider === "openrouter"
                    ? "https://openrouter.ai/api/v1"
                    : settings.provider === "ollama"
                      ? "http://localhost:11434/v1"
                      : "https://api.openai.com/v1"
              }
              value={settings.baseUrl ?? ""}
              onChange={(e) =>
                setSettings({ ...settings, baseUrl: e.target.value })
              }
              className="font-mono text-xs"
            />
          </div>

          {/* Model Selection */}
          <div className="flex flex-col gap-1.5">
            <div className="flex items-center justify-between">
              <Label className="text-xs font-medium">Выбор модели</Label>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={fetchingModels}
                  onClick={() =>
                    loadModelsFor(
                      settings.provider,
                      settings.apiKey,
                      settings.baseUrl,
                      settings.model,
                    )
                  }
                  className="h-6 text-[11px] px-2 text-muted-foreground hover:text-foreground"
                >
                  <LuRefreshCw
                    className={`size-3 mr-1 ${fetchingModels ? "animate-spin" : ""}`}
                  />
                  {fetchingModels ? "Загрузка..." : "Обновить список моделей"}
                </Button>
                <button
                  type="button"
                  onClick={() => setCustomModelMode(!customModelMode)}
                  className="text-[11px] text-primary hover:underline"
                >
                  {customModelMode ? "Выбрать из списка" : "Ввести вручную"}
                </button>
              </div>
            </div>

            {customModelMode ? (
              <Input
                type="text"
                placeholder="Например: gpt-4o, claude-3-7-sonnet, deepseek-chat..."
                value={settings.model}
                onChange={(e) =>
                  setSettings({ ...settings, model: e.target.value })
                }
                className="font-mono text-xs"
              />
            ) : (
              <Select
                value={settings.model}
                onValueChange={(val) =>
                  setSettings({ ...settings, model: val })
                }
              >
                <SelectTrigger className="w-full text-xs font-mono">
                  <SelectValue placeholder="Выберите модель" />
                </SelectTrigger>
                <SelectContent className="max-h-60">
                  {availableModels.map((m) => (
                    <SelectItem key={m} value={m} className="text-xs font-mono">
                      {m}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            )}
          </div>

          {/* System Prompt Customization */}
          <div className="flex flex-col gap-1.5 border rounded-lg p-3 bg-muted/20">
            <button
              type="button"
              onClick={() => setShowPromptDetails(!showPromptDetails)}
              className="flex items-center justify-between text-xs font-medium text-foreground hover:text-primary transition-colors"
            >
              <span className="flex items-center gap-1.5">
                <LuSparkles className="size-3.5 text-primary" />
                Системный промпт (поведение агента NullFrame)
              </span>
              {showPromptDetails ? (
                <LuChevronUp className="size-4" />
              ) : (
                <LuChevronDown className="size-4" />
              )}
            </button>
            {showPromptDetails && (
              <div className="flex flex-col gap-2 mt-2 pt-2 border-t border-border/50">
                <p className="text-[11px] text-muted-foreground">
                  Модели автоматически передается официальный системный промпт
                  NullFrame, задающий роль агента браузера, структурирование
                  мыслей (&lt;think&gt;) и действий (&lt;act&gt;). Вы можете
                  добавить свои правила или переопределить его:
                </p>
                <Textarea
                  rows={6}
                  placeholder="Оставьте пустым для использования стандартного промпта агента NullFrame..."
                  value={settings.customSystemPrompt ?? ""}
                  onChange={(e) =>
                    setSettings({
                      ...settings,
                      customSystemPrompt: e.target.value,
                    })
                  }
                  className="text-xs font-mono"
                />
                {settings.customSystemPrompt && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    className="self-start h-6 text-[11px] px-2 text-muted-foreground"
                    onClick={() =>
                      setSettings({
                        ...settings,
                        customSystemPrompt: undefined,
                      })
                    }
                  >
                    Сбросить к стандарту NullFrame
                  </Button>
                )}
              </div>
            )}
          </div>
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => onOpenChange(false)}
          >
            Отмена
          </Button>
          <Button type="button" size="sm" onClick={handleSave}>
            <LuCheck className="size-4 mr-1.5" />
            Сохранить настройки
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
