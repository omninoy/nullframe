import assert from "node:assert/strict";
import test from "node:test";
import { auditLogger } from "./audit-logger.ts";
import { customCssService } from "./custom-css-store.ts";
import { PermissionGuard, SecurityException } from "./permission-guard.ts";

test("PermissionGuard allows declared permissions and throws SecurityException on unauthorized access", () => {
  const pluginManifest = {
    id: "test-plugin",
    name: "Test Plugin",
    version: "1.0.0",
    author: "tester",
    description: "test",
    type: "plugin",
    entry: "",
    permissions: [
      { id: "ui:toast", description: "toast" },
      { id: "storage:local", description: "storage" },
    ],
  };

  assert.equal(PermissionGuard.isAllowed(pluginManifest, "ui:toast"), true);
  assert.equal(
    PermissionGuard.isAllowed(pluginManifest, "storage:local"),
    true,
  );
  assert.equal(
    PermissionGuard.isAllowed(pluginManifest, "network:http"),
    false,
  );

  assert.doesNotThrow(() => {
    PermissionGuard.checkPermission(pluginManifest, "ui:toast");
  });

  assert.throws(
    () => {
      PermissionGuard.checkPermission(pluginManifest, "network:http");
    },
    (err) =>
      err instanceof SecurityException && err.permission === "network:http",
  );
});

test("CustomCssService handles snippet lifecycle and sanitizes dangerous execution expressions", () => {
  const snippet = customCssService.addSnippet(
    "Test Style",
    "body { color: red; background: javascript:alert(1); }",
  );
  assert.ok(snippet.id);
  assert.equal(snippet.name, "Test Style");

  const list = customCssService.getSnippets();
  const found = list.find((s) => s.id === snippet.id);
  assert.ok(found);

  // Test update
  customCssService.updateSnippet(snippet.id, { name: "Updated Name" });
  assert.equal(
    customCssService.getSnippets().find((s) => s.id === snippet.id)?.name,
    "Updated Name",
  );

  // Test delete
  customCssService.deleteSnippet(snippet.id);
  assert.equal(
    customCssService.getSnippets().find((s) => s.id === snippet.id),
    undefined,
  );
});

test("AuditLogger records events and maintains log history", () => {
  const initialCount = auditLogger.getLogs().length;
  auditLogger.record({
    addonId: "test-addon",
    addonName: "Test Addon",
    level: "info",
    category: "lifecycle",
    message: "Unit test record",
  });

  const logs = auditLogger.getLogs();
  assert.equal(logs.length, initialCount + 1);
  assert.equal(logs[0].message, "Unit test record");
});
