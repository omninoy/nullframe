"use client";

import { useState } from "react";
import {
  LuBookOpen,
  LuCheck,
  LuCopy,
  LuFileCode,
  LuPlus,
} from "react-icons/lu";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { addonManager } from "@/lib/addons/addon-manager";
import type { PluginManifest } from "@/lib/addons/types";

const THEME_TEMPLATE = `/* My Custom Theme */
:root {
  --background: #0f111a;
  --surface: #181a26;
  --primary: #8b5cf6;
  --accent: #ec4899;
  --radius: 12px;
}

/* Стилизация карточек */
.data-table > tbody > tr[data-table-row]:hover > td {
  background-color: rgba(139, 92, 246, 0.12) !important;
}
`;

const PLUGIN_TEMPLATE = `// My Custom Plugin
return {
  id: "my-custom-plugin",
  name: "My Custom Plugin",
  version: "1.0.0",

  onLoad: function(ctx) {
    ctx.logger.info("Plugin loaded into memory");
  },

  onEnable: function(ctx) {
    ctx.ui.showToast({
      title: "Плагин успешно запущен",
      description: "Все хуки и таймеры активны",
      variant: "success"
    });
  },

  onDisable: function(ctx) {
    ctx.logger.info("Plugin cleanly stopped");
  }
};
`;

export function DevtoolsTab() {
  const [copiedSection, setCopiedSection] = useState<string | null>(null);
  const [manifestId, setManifestId] = useState("my-cool-plugin");
  const [manifestName, setManifestName] = useState("My Cool Plugin");
  const [authorName, setAuthorName] = useState("Developer");
  const [pluginCodeInput, setPluginCodeInput] = useState(PLUGIN_TEMPLATE);

  const copyToClipboard = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopiedSection(id);
    setTimeout(() => setCopiedSection(null), 2000);
  };

  const handleTestRunPlugin = () => {
    const manifest: PluginManifest = {
      id: manifestId,
      name: manifestName,
      author: authorName,
      version: "1.0.0",
      description: "Тестовый плагин, созданный через DevTools",
      type: "plugin",
      entry: "index.js",
      permissions: [
        { id: "ui:toast", description: "Показ уведомлений" },
        { id: "storage:local", description: "Локальное хранилище" },
      ],
    };

    addonManager.installAddon(manifest, pluginCodeInput);
    addonManager.setAddonEnabled(manifest.id, true);
  };

  return (
    <div className="flex flex-col gap-6 text-sm">
      {/* 1. Interactive Manifest & Code Tester */}
      <section className="flex flex-col gap-3 rounded-lg border border-border bg-card p-4">
        <div className="flex items-center gap-2">
          <LuFileCode className="size-4 text-primary" />
          <h3 className="font-semibold text-foreground text-base">
            Быстрый тест плагина (Hot Sandbox Test)
          </h3>
        </div>
        <p className="text-xs text-muted-foreground">
          Создайте и запустите плагин прямо в рантайме браузера без компиляции и
          сборки.
        </p>

        <div className="grid grid-cols-1 gap-3 md:grid-cols-3 mt-1">
          <Input
            value={manifestId}
            onChange={(e) => setManifestId(e.target.value)}
            placeholder="ID (latin/kebab-case)"
            className="h-8 text-xs font-mono"
          />
          <Input
            value={manifestName}
            onChange={(e) => setManifestName(e.target.value)}
            placeholder="Название плагина"
            className="h-8 text-xs"
          />
          <Input
            value={authorName}
            onChange={(e) => setAuthorName(e.target.value)}
            placeholder="Автор"
            className="h-8 text-xs"
          />
        </div>

        <div className="mt-2 rounded-md border border-input bg-black/40 font-mono text-xs overflow-hidden">
          <textarea
            value={pluginCodeInput}
            onChange={(e) => setPluginCodeInput(e.target.value)}
            rows={10}
            spellCheck={false}
            className="w-full h-full resize-none p-3 bg-transparent text-foreground outline-none leading-relaxed"
          />
        </div>

        <div className="flex justify-end">
          <Button
            size="sm"
            className="h-8 text-xs gap-1.5"
            onClick={handleTestRunPlugin}
          >
            <LuPlus className="size-3.5" />
            Загрузить и активировать в песочнице
          </Button>
        </div>
      </section>

      {/* 2. Plugin API Quick Reference */}
      <section className="flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <LuBookOpen className="size-4 text-primary" />
          <h3 className="font-semibold text-foreground text-base">
            Документация Plugin API
          </h3>
        </div>

        <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
          <div className="flex flex-col gap-2 rounded-lg border border-border bg-card p-4">
            <div className="flex items-center justify-between">
              <span className="font-medium text-foreground text-xs">
                Шаблон плагина
              </span>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 px-2 text-xs gap-1"
                onClick={() => copyToClipboard(PLUGIN_TEMPLATE, "plugin")}
              >
                {copiedSection === "plugin" ? (
                  <LuCheck className="size-3 text-success" />
                ) : (
                  <LuCopy className="size-3" />
                )}
                Копировать
              </Button>
            </div>
            <pre className="rounded bg-black/30 p-2.5 font-mono text-[11px] text-muted-foreground overflow-x-auto">
              {PLUGIN_TEMPLATE}
            </pre>
          </div>

          <div className="flex flex-col gap-2 rounded-lg border border-border bg-card p-4">
            <div className="flex items-center justify-between">
              <span className="font-medium text-foreground text-xs">
                Шаблон темы
              </span>
              <Button
                variant="ghost"
                size="sm"
                className="h-6 px-2 text-xs gap-1"
                onClick={() => copyToClipboard(THEME_TEMPLATE, "theme")}
              >
                {copiedSection === "theme" ? (
                  <LuCheck className="size-3 text-success" />
                ) : (
                  <LuCopy className="size-3" />
                )}
                Копировать
              </Button>
            </div>
            <pre className="rounded bg-black/30 p-2.5 font-mono text-[11px] text-muted-foreground overflow-x-auto">
              {THEME_TEMPLATE}
            </pre>
          </div>
        </div>
      </section>
    </div>
  );
}
