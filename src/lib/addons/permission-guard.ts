import { auditLogger } from "./audit-logger.ts";
import type { PermissionId, PluginManifest } from "./types.ts";

export class SecurityException extends Error {
  public readonly permission: PermissionId;
  public readonly pluginId: string;

  constructor(permission: PermissionId, pluginId: string) {
    super(
      `[Security Alert] Plugin «${pluginId}» attempted unauthorized access to «${permission}»`,
    );
    this.name = "SecurityException";
    this.permission = permission;
    this.pluginId = pluginId;
  }
}

export const PermissionGuard = {
  checkPermission(manifest: PluginManifest, required: PermissionId): void {
    const hasPermission = manifest.permissions?.some((p) => p.id === required);
    if (!hasPermission) {
      auditLogger.record({
        addonId: manifest.id,
        addonName: manifest.name,
        level: "security",
        category: "permission",
        message: `Заблокирована попытка вызова API без разрешения «${required}»`,
        details: { permission: required },
      });
      throw new SecurityException(required, manifest.id);
    }
  },

  isAllowed(manifest: PluginManifest, required: PermissionId): boolean {
    return !!manifest.permissions?.some((p) => p.id === required);
  },
};
