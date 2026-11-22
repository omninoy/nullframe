/**
 * Unified Addon System Type Definitions
 * Covers Themes, Custom CSS Snippets, and Sandboxed Plugins.
 */

export type AddonType = "theme" | "plugin" | "css-snippet";

export type PermissionId =
  | "ui:panel"
  | "ui:button"
  | "ui:toast"
  | "ui:modal"
  | "ui:menu"
  | "storage:local"
  | "commands:register"
  | "events:subscribe"
  | "network:http"
  | "clipboard:read"
  | "clipboard:write";

export interface AddonPermission {
  id: PermissionId;
  description: string;
}

export type AddonSettingType =
  | "color"
  | "range"
  | "boolean"
  | "string"
  | "number"
  | "select"
  | "image";

export interface AddonSettingDefinition {
  key: string;
  label: string;
  description?: string;
  type: AddonSettingType;
  default: unknown;
  min?: number;
  max?: number;
  step?: number;
  options?: Array<{ label: string; value: string }>;
}

export interface BaseAddonManifest {
  id: string;
  name: string;
  version: string;
  author: string;
  description: string;
  type: AddonType;
  entry?: string;
  minAppVersion?: string;
  icon?: string;
  preview?: string;
  homepage?: string;
  repository?: string;
  settings?: AddonSettingDefinition[];
}

export interface ThemeManifest extends BaseAddonManifest {
  type: "theme";
  entry: string; // relative path to CSS or embedded CSS string
  schemes?: Array<"light" | "dark">;
}

export interface PluginManifest extends BaseAddonManifest {
  type: "plugin";
  entry: string; // JS code or bundle path
  permissions: AddonPermission[];
}

export interface CustomCssSnippet {
  id: string;
  name: string;
  css: string;
  enabled: boolean;
  order: number;
  createdAt: number;
  updatedAt: number;
}

export interface InstalledAddon<
  T extends BaseAddonManifest = BaseAddonManifest,
> {
  manifest: T;
  enabled: boolean;
  installedAt: number;
  userSettings: Record<string, unknown>;
  hasPendingUpdate?: boolean;
  error?: string | null;
}

export interface AuditLogEntry {
  id: string;
  timestamp: number;
  addonId: string;
  addonName: string;
  level: "info" | "warn" | "error" | "security";
  category: "lifecycle" | "permission" | "storage" | "network" | "ui";
  message: string;
  details?: Record<string, unknown>;
}

export interface Disposable {
  dispose(): void;
}

export interface PanelOptions {
  id: string;
  title: string;
  icon?: string;
  position?: "sidebar" | "bottom" | "top";
  renderHtml?: string;
}

export interface ButtonOptions {
  id: string;
  label: string;
  icon?: string;
  tooltip?: string;
  location?: "header-left" | "header-right" | "status-bar";
  onClickId: string;
}

export interface ToastOptions {
  title: string;
  description?: string;
  variant?: "default" | "success" | "warning" | "destructive";
}

export interface ModalOptions {
  title: string;
  contentHtml: string;
  buttons?: Array<{
    id: string;
    label: string;
    variant?: "default" | "destructive" | "outline";
  }>;
}

export interface CommandOptions {
  id: string;
  name: string;
  hotkey?: string;
  onExecuteId: string;
}
