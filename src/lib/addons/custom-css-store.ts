import { auditLogger } from "./audit-logger.ts";
import type { CustomCssSnippet } from "./types.ts";

const STORAGE_KEY = "nullframe_custom_css_snippets";
const STYLE_TAG_PREFIX = "nullframe-custom-css-";

class CustomCssService {
  private snippets: CustomCssSnippet[] = [];
  private listeners = new Set<(snippets: CustomCssSnippet[]) => void>();

  constructor() {
    this.load();
    if (typeof window !== "undefined") {
      this.reapplyAll();
    }
  }

  private load(): void {
    if (typeof window === "undefined") return;
    try {
      const raw = window.localStorage.getItem(STORAGE_KEY);
      if (raw) {
        this.snippets = JSON.parse(raw);
      } else {
        // Seed initial example snippet
        this.snippets = [
          {
            id: "snippet-compact-ui",
            name: "Компактные отступы таблицы",
            css: "/* Пример пользовательского CSS */\n.data-table > tbody > tr[data-table-row] > td {\n  padding-top: 6px;\n  padding-bottom: 6px;\n}",
            enabled: false,
            order: 0,
            createdAt: Date.now(),
            updatedAt: Date.now(),
          },
        ];
        this.persist();
      }
    } catch {
      this.snippets = [];
    }
  }

  private persist(): void {
    if (typeof window === "undefined") return;
    try {
      window.localStorage.setItem(STORAGE_KEY, JSON.stringify(this.snippets));
    } catch {
      // quota handling
    }
  }

  public getSnippets(): CustomCssSnippet[] {
    return [...this.snippets].sort((a, b) => a.order - b.order);
  }

  public addSnippet(name: string, css: string): CustomCssSnippet {
    const newSnippet: CustomCssSnippet = {
      id: `snippet-${Date.now()}-${Math.random().toString(36).substring(2, 6)}`,
      name,
      css,
      enabled: true,
      order: this.snippets.length,
      createdAt: Date.now(),
      updatedAt: Date.now(),
    };
    this.snippets.push(newSnippet);
    this.persist();
    this.applySnippet(newSnippet);
    this.notify();
    auditLogger.record({
      addonId: newSnippet.id,
      addonName: newSnippet.name,
      level: "info",
      category: "ui",
      message: `CSS-сниппет создан и активирован`,
    });
    return newSnippet;
  }

  public updateSnippet(
    id: string,
    updates: Partial<
      Pick<CustomCssSnippet, "name" | "css" | "enabled" | "order">
    >,
  ): void {
    const index = this.snippets.findIndex((s) => s.id === id);
    if (index === -1) return;

    this.snippets[index] = {
      ...this.snippets[index],
      ...updates,
      updatedAt: Date.now(),
    };
    this.persist();
    this.applySnippet(this.snippets[index]);
    this.notify();
  }

  public deleteSnippet(id: string): void {
    const snippet = this.snippets.find((s) => s.id === id);
    this.snippets = this.snippets.filter((s) => s.id !== id);
    this.persist();
    this.removeStyleTag(id);
    this.notify();
    if (snippet) {
      auditLogger.record({
        addonId: id,
        addonName: snippet.name,
        level: "info",
        category: "ui",
        message: `CSS-сниппет удален`,
      });
    }
  }

  public setEnabled(id: string, enabled: boolean): void {
    this.updateSnippet(id, { enabled });
  }

  public reorder(activeId: string, overId: string): void {
    const oldIndex = this.snippets.findIndex((s) => s.id === activeId);
    const newIndex = this.snippets.findIndex((s) => s.id === overId);
    if (oldIndex === -1 || newIndex === -1) return;

    const [moved] = this.snippets.splice(oldIndex, 1);
    this.snippets.splice(newIndex, 0, moved);
    this.snippets.forEach((item, index) => {
      item.order = index;
    });
    this.persist();
    this.reapplyAll();
    this.notify();
  }

  public applySnippet(snippet: CustomCssSnippet): void {
    if (typeof document === "undefined") return;
    const tagId = `${STYLE_TAG_PREFIX}${snippet.id}`;
    let styleEl = document.getElementById(tagId) as HTMLStyleElement | null;

    if (!snippet.enabled) {
      if (styleEl) styleEl.remove();
      return;
    }

    if (!styleEl) {
      styleEl = document.createElement("style");
      styleEl.id = tagId;
      styleEl.setAttribute("data-addon-type", "custom-css");
      styleEl.setAttribute("data-snippet-id", snippet.id);
      document.head.appendChild(styleEl);
    }

    // Sanitize basic XSS vectors (e.g. javascript: urls inside css expressions)
    const sanitizedCss = this.sanitizeCss(snippet.css);
    styleEl.textContent = sanitizedCss;
  }

  public reapplyAll(): void {
    if (typeof document === "undefined") return;
    this.snippets
      .sort((a, b) => a.order - b.order)
      .forEach((snippet) => {
        this.applySnippet(snippet);
      });
  }

  public removeAllStyles(): void {
    if (typeof document === "undefined") return;
    document
      .querySelectorAll(`style[data-addon-type="custom-css"]`)
      .forEach((el) => el.remove());
  }

  private removeStyleTag(id: string): void {
    if (typeof document === "undefined") return;
    const styleEl = document.getElementById(`${STYLE_TAG_PREFIX}${id}`);
    if (styleEl) styleEl.remove();
  }

  private sanitizeCss(css: string): string {
    // Strip javascript: expressions, behavior properties, vbscript, and dangerous bindings
    return css
      .replace(/javascript:/gi, "blocked-javascript:")
      .replace(/vbscript:/gi, "blocked-vbscript:")
      .replace(/-moz-binding/gi, "blocked-binding")
      .replace(/expression\s*\(/gi, "blocked-expression(");
  }

  public exportSnippetsJson(): string {
    return JSON.stringify(this.snippets, null, 2);
  }

  public importSnippetsJson(jsonString: string): {
    success: boolean;
    count: number;
    error?: string;
  } {
    try {
      const parsed = JSON.parse(jsonString);
      if (!Array.isArray(parsed)) {
        return { success: false, count: 0, error: "Ожидался массив сниппетов" };
      }
      let count = 0;
      for (const item of parsed) {
        if (typeof item.name === "string" && typeof item.css === "string") {
          this.addSnippet(item.name, item.css);
          count++;
        }
      }
      return { success: true, count };
    } catch (err: unknown) {
      return { success: false, count: 0, error: (err as Error).message };
    }
  }

  public subscribe(
    listener: (snippets: CustomCssSnippet[]) => void,
  ): () => void {
    this.listeners.add(listener);
    listener(this.getSnippets());
    return () => this.listeners.delete(listener);
  }

  private notify(): void {
    const list = this.getSnippets();
    this.listeners.forEach((listener) => listener(list));
  }
}

export const customCssService = new CustomCssService();
