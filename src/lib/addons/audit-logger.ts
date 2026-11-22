import type { AuditLogEntry } from "./types";

const MAX_LOG_ENTRIES = 500;
const STORAGE_KEY = "nullframe_addon_audit_log";

class AuditLoggerService {
  private logs: AuditLogEntry[] = [];
  private listeners = new Set<(logs: AuditLogEntry[]) => void>();

  constructor() {
    this.load();
  }

  private load(): void {
    if (typeof window === "undefined") return;
    try {
      const stored = window.localStorage.getItem(STORAGE_KEY);
      if (stored) {
        this.logs = JSON.parse(stored);
      }
    } catch {
      this.logs = [];
    }
  }

  private persist(): void {
    if (typeof window === "undefined") return;
    try {
      window.localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify(this.logs.slice(-MAX_LOG_ENTRIES)),
      );
    } catch {
      // Ignore storage quota errors
    }
  }

  public record(entry: Omit<AuditLogEntry, "id" | "timestamp">): void {
    const fullEntry: AuditLogEntry = {
      id: `audit_${Date.now()}_${Math.random().toString(36).substring(2, 7)}`,
      timestamp: Date.now(),
      ...entry,
    };

    this.logs.push(fullEntry);
    if (this.logs.length > MAX_LOG_ENTRIES) {
      this.logs.shift();
    }
    this.persist();
    this.notify();
  }

  public getLogs(): AuditLogEntry[] {
    return [...this.logs].reverse();
  }

  public clear(): void {
    this.logs = [];
    this.persist();
    this.notify();
  }

  public subscribe(listener: (logs: AuditLogEntry[]) => void): () => void {
    this.listeners.add(listener);
    listener(this.getLogs());
    return () => this.listeners.delete(listener);
  }

  private notify(): void {
    const current = this.getLogs();
    this.listeners.forEach((listener) => listener(current));
  }
}

export const auditLogger = new AuditLoggerService();
