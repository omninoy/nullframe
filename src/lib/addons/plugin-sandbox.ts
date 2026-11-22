import { auditLogger } from "./audit-logger";
import { PermissionGuard } from "./permission-guard";
import type { InstalledAddon, PluginManifest, ToastOptions } from "./types";

interface RpcRequest {
  id: string;
  type: string;
  method: string;
  payload: unknown;
}

interface RpcResponse {
  id: string;
  success: boolean;
  result?: unknown;
  error?: string;
}

export class PluginSandboxInstance {
  private iframe: HTMLIFrameElement | null = null;
  private messageChannel: MessageChannel | null = null;
  private cleanups: Array<() => void> = [];
  private isLoaded = false;
  private isEnabled = false;
  public readonly addon: InstalledAddon<PluginManifest>;
  private readonly onUiAction?: (action: string, payload: unknown) => void;

  constructor(
    addon: InstalledAddon<PluginManifest>,
    onUiAction?: (action: string, payload: unknown) => void,
  ) {
    this.addon = addon;
    this.onUiAction = onUiAction;
  }

  public async start(pluginCode: string): Promise<void> {
    if (typeof document === "undefined") return;

    this.iframe = document.createElement("iframe");
    this.iframe.style.display = "none";
    this.iframe.setAttribute("sandbox", "allow-scripts");
    this.iframe.setAttribute("data-plugin-sandbox", this.addon.manifest.id);

    const channel = new MessageChannel();
    this.messageChannel = channel;

    channel.port1.onmessage = (event: MessageEvent<RpcRequest>) => {
      this.handleRpc(event.data, channel.port1);
    };

    const bootstrapHtml = `
<!DOCTYPE html>
<html>
<head><meta charset="utf-8"></head>
<body>
<script>
(function() {
  // 1. Strip direct access to dangerous global APIs
  try { delete window.localStorage; } catch(e){}
  try { delete window.sessionStorage; } catch(e){}
  try { delete window.indexedDB; } catch(e){}
  try { delete window.open; } catch(e){}
  try { delete window.parent; } catch(e){}
  try { delete window.top; } catch(e){}
  window.eval = function() { throw new Error("eval() is strictly forbidden in plugin sandbox"); };
  window.Function = function() { throw new Error("Function constructor is forbidden in plugin sandbox"); };

  let port = null;
  let rpcCounter = 0;
  const pendingRequests = new Map();

  function callHost(method, payload) {
    return new Promise((resolve, reject) => {
      if (!port) return reject(new Error("RPC channel not initialized"));
      const id = "req_" + (++rpcCounter);
      pendingRequests.set(id, { resolve, reject });
      port.postMessage({ id, method, payload });
    });
  }

  window.addEventListener("message", function(e) {
    if (e.data === "INIT_SANDBOX" && e.ports && e.ports[0]) {
      port = e.ports[0];
      port.onmessage = function(event) {
        const res = event.data;
        if (res.type === "LIFECYCLE") {
          handleLifecycle(res.action, res.payload);
          return;
        }
        const pending = pendingRequests.get(res.id);
        if (pending) {
          pendingRequests.delete(res.id);
          if (res.success) pending.resolve(res.result);
          else pending.reject(new Error(res.error || "RPC Failed"));
        }
      };
      port.postMessage({ id: "handshake", method: "HANDSHAKE_READY", payload: {} });
    }
  });

  // Client Plugin Context
  const ctx = {
    plugin: { id: "${this.addon.manifest.id}", name: "${this.addon.manifest.name}" },
    ui: {
      showToast: function(opts) { return callHost("ui.showToast", opts); },
      createPanel: function(opts) { return callHost("ui.createPanel", opts); },
      addButton: function(opts) { return callHost("ui.addButton", opts); },
    },
    storage: {
      get: function(key, def) { return callHost("storage.get", { key, def }); },
      set: function(key, val) { return callHost("storage.set", { key, val }); },
      delete: function(key) { return callHost("storage.delete", { key }); }
    },
    logger: {
      info: function(msg) { return callHost("logger.log", { level: "info", msg }); },
      warn: function(msg) { return callHost("logger.log", { level: "warn", msg }); },
      error: function(msg) { return callHost("logger.log", { level: "error", msg }); },
    }
  };

  let pluginModule = null;

  function handleLifecycle(action, payload) {
    try {
      if (action === "LOAD") {
        if (pluginModule && typeof pluginModule.onLoad === "function") {
          pluginModule.onLoad(ctx);
        }
      } else if (action === "ENABLE") {
        if (pluginModule && typeof pluginModule.onEnable === "function") {
          pluginModule.onEnable(ctx);
        }
      } else if (action === "DISABLE") {
        if (pluginModule && typeof pluginModule.onDisable === "function") {
          pluginModule.onDisable(ctx);
        }
      } else if (action === "UNLOAD") {
        if (pluginModule && typeof pluginModule.onUnload === "function") {
          pluginModule.onUnload(ctx);
        }
      }
    } catch (err) {
      callHost("logger.log", { level: "error", msg: "Lifecycle error: " + (err.message || String(err)) });
    }
  }

  // Execute provided plugin code securely inside isolated scope
  try {
    const fn = new window.constructor.constructor("ctx", ${JSON.stringify(pluginCode)});
    pluginModule = fn(ctx) || {};
  } catch(err) {
    setTimeout(() => {
      callHost("logger.log", { level: "error", msg: "Plugin compilation failed: " + (err.message || String(err)) });
    }, 50);
  }
})();
</script>
</body>
</html>
`;

    document.body.appendChild(this.iframe);
    this.iframe.srcdoc = bootstrapHtml;

    await new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => {
        reject(
          new Error(
            `Plugin sandbox handshake timeout for ${this.addon.manifest.id}`,
          ),
        );
      }, 5000);

      const frame = this.iframe;
      if (!frame) {
        clearTimeout(timeout);
        reject(new Error("Sandbox iframe not initialized"));
        return;
      }

      frame.onload = () => {
        frame.contentWindow?.postMessage("INIT_SANDBOX", "*", [channel.port2]);
        this.cleanups.push(() => clearTimeout(timeout));
        resolve();
      };
    });

    this.isLoaded = true;
    this.dispatchLifecycle("LOAD");
  }

  public enable(): void {
    if (!this.isLoaded) return;
    this.isEnabled = true;
    this.dispatchLifecycle("ENABLE");
    auditLogger.record({
      addonId: this.addon.manifest.id,
      addonName: this.addon.manifest.name,
      level: "info",
      category: "lifecycle",
      message: `Плагин активирован`,
    });
  }

  public disable(): void {
    if (!this.isEnabled) return;
    this.dispatchLifecycle("DISABLE");
    this.isEnabled = false;
    this.disposeCleanups();
    auditLogger.record({
      addonId: this.addon.manifest.id,
      addonName: this.addon.manifest.name,
      level: "info",
      category: "lifecycle",
      message: `Плагин деактивирован`,
    });
  }

  public terminate(): void {
    this.disable();
    this.dispatchLifecycle("UNLOAD");
    this.disposeCleanups();
    if (this.messageChannel) {
      this.messageChannel.port1.close();
      this.messageChannel = null;
    }
    if (this.iframe) {
      this.iframe.remove();
      this.iframe = null;
    }
    this.isLoaded = false;
  }

  private dispatchLifecycle(
    action: "LOAD" | "ENABLE" | "DISABLE" | "UNLOAD",
  ): void {
    if (this.messageChannel) {
      this.messageChannel.port1.postMessage({
        type: "LIFECYCLE",
        action,
      });
    }
  }

  private async handleRpc(req: RpcRequest, port: MessagePort): Promise<void> {
    const { id, method, payload } = req;
    if (method === "HANDSHAKE_READY") {
      return;
    }

    try {
      let result: unknown = null;

      if (method === "ui.showToast") {
        PermissionGuard.checkPermission(this.addon.manifest, "ui:toast");
        const opts = payload as ToastOptions;
        this.onUiAction?.("toast", opts);
        result = true;
      } else if (method === "ui.createPanel") {
        PermissionGuard.checkPermission(this.addon.manifest, "ui:panel");
        this.onUiAction?.("panel", payload);
        result = { panelId: `panel-${Date.now()}` };
      } else if (method === "ui.addButton") {
        PermissionGuard.checkPermission(this.addon.manifest, "ui:button");
        this.onUiAction?.("button", payload);
        result = { buttonId: `btn-${Date.now()}` };
      } else if (method === "storage.get") {
        PermissionGuard.checkPermission(this.addon.manifest, "storage:local");
        const { key, def } = payload as { key: string; def?: unknown };
        const stored = window.localStorage.getItem(
          `plugin_store_${this.addon.manifest.id}_${key}`,
        );
        result = stored ? JSON.parse(stored) : def;
      } else if (method === "storage.set") {
        PermissionGuard.checkPermission(this.addon.manifest, "storage:local");
        const { key, val } = payload as { key: string; val: unknown };
        window.localStorage.setItem(
          `plugin_store_${this.addon.manifest.id}_${key}`,
          JSON.stringify(val),
        );
        result = true;
      } else if (method === "storage.delete") {
        PermissionGuard.checkPermission(this.addon.manifest, "storage:local");
        const { key } = payload as { key: string };
        window.localStorage.removeItem(
          `plugin_store_${this.addon.manifest.id}_${key}`,
        );
        result = true;
      } else if (method === "logger.log") {
        const { level, msg } = payload as {
          level: "info" | "warn" | "error";
          msg: string;
        };
        auditLogger.record({
          addonId: this.addon.manifest.id,
          addonName: this.addon.manifest.name,
          level: level === "error" ? "error" : "info",
          category: "lifecycle",
          message: `[Plugin Log] ${msg}`,
        });
        result = true;
      } else {
        throw new Error(`Unknown RPC method: ${method}`);
      }

      port.postMessage({ id, success: true, result } satisfies RpcResponse);
    } catch (err: unknown) {
      const errorMessage = (err as Error).message || String(err);
      port.postMessage({
        id,
        success: false,
        error: errorMessage,
      } satisfies RpcResponse);
    }
  }

  private disposeCleanups(): void {
    this.cleanups.forEach((cleanup) => {
      try {
        cleanup();
      } catch {
        // ignore errors on cleanup
      }
    });
    this.cleanups = [];
  }
}
