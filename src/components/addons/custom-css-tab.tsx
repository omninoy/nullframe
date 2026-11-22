"use client";

import { useCallback, useEffect, useState } from "react";
import {
  LuCode,
  LuDownload,
  LuPlus,
  LuSave,
  LuSparkles,
  LuTrash2,
  LuUpload,
} from "react-icons/lu";
import { AnimatedSwitch } from "@/components/ui/animated-switch";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { customCssService } from "@/lib/addons/custom-css-store";
import type { CustomCssSnippet } from "@/lib/addons/types";

const SUGGESTED_TOKENS = [
  "--background",
  "--foreground",
  "--surface",
  "--surface-elevated",
  "--primary",
  "--primary-hover",
  "--secondary",
  "--accent",
  "--destructive",
  "--success",
  "--warning",
  "--info",
  "--border",
  "--divider",
  "--radius",
  "--scrollbar-thumb",
];

export function CustomCssTab() {
  const [snippets, setSnippets] = useState<CustomCssSnippet[]>([]);
  const [activeSnippetId, setActiveSnippetId] = useState<string | null>(null);
  const [editorName, setEditorName] = useState("");
  const [editorCss, setEditorCss] = useState("");
  const [isDirty, setIsDirty] = useState(false);
  const [livePreview, setLivePreview] = useState(true);

  const selectSnippet = useCallback((snippet: CustomCssSnippet) => {
    setActiveSnippetId(snippet.id);
    setEditorName(snippet.name);
    setEditorCss(snippet.css);
    setIsDirty(false);
  }, []);

  useEffect(() => {
    const unsub = customCssService.subscribe((list) => {
      setSnippets(list);
      if (!activeSnippetId && list.length > 0) {
        selectSnippet(list[0]);
      }
    });
    return unsub;
  }, [activeSnippetId, selectSnippet]);

  const handleCreateNew = () => {
    const created = customCssService.addSnippet(
      "Новый стиль",
      "/* Введите ваш CSS код здесь */\n",
    );
    selectSnippet(created);
  };

  const handleSave = () => {
    if (!activeSnippetId) return;
    customCssService.updateSnippet(activeSnippetId, {
      name: editorName,
      css: editorCss,
    });
    setIsDirty(false);
  };

  const handleCodeChange = (newCode: string) => {
    setEditorCss(newCode);
    setIsDirty(true);
    if (livePreview && activeSnippetId) {
      customCssService.updateSnippet(activeSnippetId, {
        css: newCode,
      });
    }
  };

  const handleToggle = (snippet: CustomCssSnippet) => {
    customCssService.setEnabled(snippet.id, !snippet.enabled);
  };

  const handleDelete = (id: string) => {
    customCssService.deleteSnippet(id);
    if (activeSnippetId === id) {
      const remaining = snippets.filter((s) => s.id !== id);
      if (remaining.length > 0) selectSnippet(remaining[0]);
      else {
        setActiveSnippetId(null);
        setEditorName("");
        setEditorCss("");
      }
    }
  };

  const handleExport = () => {
    const json = customCssService.exportSnippetsJson();
    const blob = new Blob([json], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `nullframe-custom-css-${Date.now()}.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleImport = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = (event) => {
      const content = event.target?.result as string;
      if (content) {
        customCssService.importSnippetsJson(content);
      }
    };
    reader.readAsText(file);
  };

  const activeSnippet = snippets.find((s) => s.id === activeSnippetId);

  return (
    <div className="grid grid-cols-1 gap-6 lg:grid-cols-12 text-sm">
      {/* Left List of Snippets */}
      <div className="flex flex-col gap-3 lg:col-span-4">
        <div className="flex items-center justify-between">
          <span className="font-semibold text-foreground text-sm">
            Сниппеты ({snippets.length})
          </span>
          <div className="flex items-center gap-1">
            <Button
              variant="outline"
              size="sm"
              className="h-7 px-2 text-xs"
              onClick={handleCreateNew}
            >
              <LuPlus className="size-3.5 mr-1" />
              Добавить
            </Button>
            <label className="cursor-pointer">
              <input
                type="file"
                accept=".json"
                onChange={handleImport}
                className="hidden"
              />
              <Button
                variant="ghost"
                size="sm"
                asChild
                className="h-7 px-2 text-xs"
              >
                <span>
                  <LuUpload className="size-3.5" />
                </span>
              </Button>
            </label>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 px-2 text-xs"
              onClick={handleExport}
            >
              <LuDownload className="size-3.5" />
            </Button>
          </div>
        </div>

        <div className="flex flex-col gap-1.5 overflow-y-auto max-h-[500px] pr-1">
          {snippets.map((snippet) => {
            const isSelected = snippet.id === activeSnippetId;
            return (
              <div
                key={snippet.id}
                className={`flex items-center justify-between gap-2 rounded-lg border p-2.5 transition-all ${
                  isSelected
                    ? "border-primary bg-primary/10 text-foreground"
                    : "border-border bg-card hover:border-foreground/20 text-muted-foreground"
                }`}
              >
                <button
                  type="button"
                  onClick={() => selectSnippet(snippet)}
                  className="flex items-center gap-2 truncate flex-1 text-left cursor-pointer"
                >
                  <LuCode className="size-4 shrink-0 text-primary" />
                  <span className="truncate text-xs font-medium text-foreground">
                    {snippet.name}
                  </span>
                </button>
                <div className="flex items-center gap-2 shrink-0">
                  <AnimatedSwitch
                    checked={snippet.enabled}
                    onCheckedChange={() => handleToggle(snippet)}
                    aria-label={`Включить ${snippet.name}`}
                  />
                  <button
                    type="button"
                    onClick={() => handleDelete(snippet.id)}
                    className="p-1 hover:text-destructive text-muted-foreground transition-colors cursor-pointer"
                    aria-label={`Удалить ${snippet.name}`}
                  >
                    <LuTrash2 className="size-3.5" />
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Right Editor Panel */}
      <div className="flex flex-col gap-3 rounded-lg border border-border bg-card p-4 lg:col-span-8">
        {activeSnippet ? (
          <>
            <div className="flex items-center justify-between gap-3 border-b border-border pb-3">
              <Input
                value={editorName}
                onChange={(e) => {
                  setEditorName(e.target.value);
                  setIsDirty(true);
                }}
                className="h-8 max-w-sm text-xs font-medium"
                placeholder="Название стиля"
              />
              <div className="flex items-center gap-2">
                <div className="flex items-center gap-1.5 text-xs text-muted-foreground select-none">
                  <AnimatedSwitch
                    checked={livePreview}
                    onCheckedChange={setLivePreview}
                    aria-label="Live Preview"
                  />
                  <span>Live Preview</span>
                </div>
                <Button
                  size="sm"
                  variant={isDirty ? "default" : "outline"}
                  className="h-8 gap-1.5 text-xs"
                  onClick={handleSave}
                >
                  <LuSave className="size-3.5" />
                  Сохранить
                </Button>
              </div>
            </div>

            {/* Token Suggestions Helper Bar */}
            <div className="flex items-center gap-1.5 overflow-x-auto py-1 scrollbar-none text-[11px]">
              <span className="flex items-center gap-1 font-semibold text-muted-foreground shrink-0">
                <LuSparkles className="size-3 text-primary" /> Токены:
              </span>
              {SUGGESTED_TOKENS.map((token) => (
                <button
                  key={token}
                  type="button"
                  onClick={() => handleCodeChange(`${editorCss}\n  ${token}: `)}
                  className="rounded bg-foreground/5 hover:bg-foreground/10 px-1.5 py-0.5 font-mono text-muted-foreground hover:text-foreground transition-colors shrink-0"
                >
                  var({token})
                </button>
              ))}
            </div>

            {/* Code Editor Area */}
            <div className="relative flex flex-1 min-h-[360px] rounded-md border border-input bg-black/40 font-mono text-xs overflow-hidden">
              <textarea
                value={editorCss}
                onChange={(e) => handleCodeChange(e.target.value)}
                placeholder="/* Вставьте ваш CSS сюда. Поддерживается live-preview. */"
                spellCheck={false}
                className="w-full h-full resize-none p-3 bg-transparent text-foreground outline-none leading-relaxed"
              />
            </div>
            <div className="flex items-center justify-between text-[11px] text-muted-foreground">
              <span>Символов: {editorCss.length}</span>
              <span>Изменения применяются мгновенно без перезагрузки</span>
            </div>
          </>
        ) : (
          <div className="flex h-64 items-center justify-center text-muted-foreground">
            Выберите или создайте CSS-сниппет для редактирования
          </div>
        )}
      </div>
    </div>
  );
}
