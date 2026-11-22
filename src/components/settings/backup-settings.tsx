"use client";

import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import * as React from "react";
import { useTranslation } from "react-i18next";
import {
  LuArchiveRestore,
  LuDatabaseBackup,
  LuEye,
  LuEyeOff,
  LuLoaderCircle,
} from "react-icons/lu";
import { LoadingButton } from "@/components/loading-button";
import {
  RowIcon,
  SettingRow,
  SettingsCard,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
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
import { translateBackendError } from "@/lib/backend-errors";
import { formatBytes } from "@/lib/format-bytes";
import { profileList } from "@/lib/profile-list-store";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import type { BrowserProfile } from "@/types";

/** The archive extension the backend names and the file dialogs filter on. */
const BACKUP_EXTENSION = "neobackup";

/** A backup-worthy profile: a disposable one has nothing worth archiving. */
function isBackupWorthy(profile: BrowserProfile): boolean {
  return profile.ephemeral !== true && profile.temporary !== true;
}

function todayStamp(): string {
  return new Date().toISOString().slice(0, 10);
}

/** Backup and restore: profiles and proxies in one portable .neobackup. */
export function BackupSettings() {
  const { t } = useTranslation();
  const [createOpen, setCreateOpen] = React.useState(false);
  const [restoreOpen, setRestoreOpen] = React.useState(false);

  return (
    <>
      <SettingsCard>
        <SettingRow
          target="backup-create"
          label={t("settings.backup.create")}
          description={t("settings.backup.createDescription")}
          icon={
            <RowIcon tone="primary">
              <LuDatabaseBackup />
            </RowIcon>
          }
        >
          <Button
            size="sm"
            variant="soft"
            onClick={() => setCreateOpen(true)}
            data-slot="settings-backup-create"
          >
            {t("settings.backup.createButton")}
          </Button>
        </SettingRow>
        <SettingRow
          target="backup-restore"
          label={t("settings.backup.restore")}
          description={t("settings.backup.restoreDescription")}
          icon={
            <RowIcon tone="muted">
              <LuArchiveRestore />
            </RowIcon>
          }
        >
          <Button
            size="sm"
            variant="soft"
            onClick={() => setRestoreOpen(true)}
            data-slot="settings-backup-restore"
          >
            {t("settings.backup.restoreButton")}
          </Button>
        </SettingRow>
      </SettingsCard>
      <CreateBackupDialog
        isOpen={createOpen}
        onClose={() => setCreateOpen(false)}
      />
      <RestoreBackupDialog
        isOpen={restoreOpen}
        onClose={() => setRestoreOpen(false)}
      />
    </>
  );
}

/** One checked profile per row, with a select-everything header. */
function ProfileChecklist({
  profiles,
  selected,
  onToggle,
  onToggleAll,
}: {
  profiles: BrowserProfile[];
  selected: Set<string>;
  onToggle: (id: string) => void;
  onToggleAll: (checked: boolean) => void;
}) {
  const { t } = useTranslation();
  const allSelected =
    profiles.length > 0 &&
    profiles.every((profile) => selected.has(profile.id));
  const someSelected = profiles.some((profile) => selected.has(profile.id));

  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between gap-2">
        <Label className="flex items-center gap-2 text-sm">
          <Checkbox
            checked={
              allSelected ? true : someSelected ? "indeterminate" : false
            }
            onCheckedChange={(checked) => onToggleAll(checked === true)}
          />
          {t("settings.backup.selectAll")}
        </Label>
        {/* What the archive will actually hold, at the control that decides it. */}
        <Badge variant="soft" className="tabular-nums">
          {t("settings.backup.selectedCount", {
            selected: selected.size,
            total: profiles.length,
          })}
        </Badge>
      </div>
      <div className="max-h-44 overflow-y-auto rounded-md border border-foreground/10 p-1.5">
        {profiles.map((profile) => (
          <Label
            key={profile.id}
            className="flex cursor-pointer items-center gap-2 rounded px-1.5 py-1 text-sm hover:bg-foreground/5"
          >
            <Checkbox
              checked={selected.has(profile.id)}
              onCheckedChange={() => onToggle(profile.id)}
            />
            <span className="truncate">{profile.name}</span>
          </Label>
        ))}
      </div>
    </div>
  );
}

/** A password input with a show/hide eye, for the archive passphrase. */
function PasswordField({
  id,
  value,
  onChange,
  disabled,
}: {
  id: string;
  value: string;
  onChange: (next: string) => void;
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [visible, setVisible] = React.useState(false);
  const label = visible
    ? t("settings.backup.hidePassword")
    : t("settings.backup.showPassword");
  return (
    <div className="relative">
      <Input
        id={id}
        type={visible ? "text" : "password"}
        value={value}
        placeholder={t("settings.backup.passwordPlaceholder")}
        onChange={(event) => onChange(event.target.value)}
        disabled={disabled}
        autoComplete="off"
        className="pr-9"
      />
      <button
        type="button"
        aria-label={label}
        aria-pressed={visible}
        title={label}
        onClick={() => setVisible((current) => !current)}
        className="absolute top-1/2 right-1 inline-flex size-7 -translate-y-1/2 cursor-pointer items-center justify-center rounded-md text-muted-foreground transition-colors duration-100 hover:bg-foreground/8 hover:text-foreground"
      >
        {visible ? (
          <LuEyeOff className="size-4" />
        ) : (
          <LuEye className="size-4" />
        )}
      </button>
    </div>
  );
}

/** The spinner line shown while the archive is being packed or unpacked. */
function BackupBusyNote({ label }: { label: string }) {
  return (
    <p
      role="status"
      aria-live="polite"
      className="flex items-center gap-2 text-xs text-muted-foreground"
    >
      <LuLoaderCircle className="size-3.5 shrink-0 animate-spin" />
      {label}
    </p>
  );
}

function CreateBackupDialog({
  isOpen,
  onClose,
}: {
  isOpen: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [profiles, setProfiles] = React.useState<BrowserProfile[] | null>(null);
  const [selected, setSelected] = React.useState<Set<string>>(new Set());
  const [includeProxies, setIncludeProxies] = React.useState(true);
  const [password, setPassword] = React.useState("");
  const [isCreating, setIsCreating] = React.useState(false);

  React.useEffect(() => {
    if (!isOpen) return;
    let active = true;
    void profileList
      .load()
      .then((loaded) => {
        if (!active) return;
        const worthy = loaded
          .filter(isBackupWorthy)
          .sort((a, b) => a.name.localeCompare(b.name));
        setProfiles(worthy);
        setSelected(new Set(worthy.map((profile) => profile.id)));
      })
      .catch(() => {
        if (active) setProfiles([]);
      });
    return () => {
      active = false;
    };
  }, [isOpen]);

  const toggle = (id: string) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const create = async () => {
    // The save dialog names the file; the backend appends the extension when
    // the name dropped it.
    const target = await save({
      title: t("settings.backup.saveTitle"),
      defaultPath: `nullframe-backup-${todayStamp()}.${BACKUP_EXTENSION}`,
      filters: [
        {
          name: t("settings.backup.archiveFilter"),
          extensions: [BACKUP_EXTENSION, "zip"],
        },
      ],
    });
    if (!target) return;
    setIsCreating(true);
    try {
      const allSelected = (profiles ?? []).every((profile) =>
        selected.has(profile.id),
      );
      const result = await invoke<{
        path: string;
        profile_count: number;
        size_bytes: number;
        encrypted: boolean;
      }>("create_backup", {
        // Every profile: the backend takes None; the ids otherwise.
        profileIds: allSelected ? undefined : [...selected],
        includeProxies,
        targetPath: target,
        password: password.length > 0 ? password : undefined,
      });
      showSuccessToast(
        t("toasts.success.backupCreated", {
          count: result.profile_count,
          size: formatBytes(result.size_bytes),
        }),
        {
          description: result.encrypted
            ? t("settings.backup.createdEncrypted")
            : undefined,
        },
      );
      setPassword("");
      onClose();
    } catch (error) {
      showErrorToast(translateBackendError(t, error));
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (isCreating) return;
        onClose();
        if (!open) setPassword("");
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("settings.backup.createDialogTitle")}</DialogTitle>
          <DialogDescription>
            {t("settings.backup.createDialogDescription")}
          </DialogDescription>
        </DialogHeader>
        {profiles === null ? (
          <p className="py-6 text-center text-sm text-muted-foreground">
            {t("settings.backup.loadingProfiles")}
          </p>
        ) : profiles.length === 0 ? (
          <p className="py-6 text-center text-sm text-muted-foreground">
            {t("settings.backup.noProfiles")}
          </p>
        ) : (
          <div className="flex flex-col gap-3">
            <ProfileChecklist
              profiles={profiles}
              selected={selected}
              onToggle={toggle}
              onToggleAll={(checked) =>
                setSelected(
                  checked
                    ? new Set(profiles.map((profile) => profile.id))
                    : new Set(),
                )
              }
            />
            <Label className="flex items-center gap-2 text-sm">
              <Checkbox
                checked={includeProxies}
                onCheckedChange={(checked) =>
                  setIncludeProxies(checked === true)
                }
              />
              {t("settings.backup.includeProxies")}
            </Label>
            <div className="flex flex-col gap-1">
              <Label htmlFor="backup-password" className="text-sm">
                {t("settings.backup.password")}
              </Label>
              <PasswordField
                id="backup-password"
                value={password}
                onChange={setPassword}
                disabled={isCreating}
              />
              <p className="text-xs text-muted-foreground">
                {t("settings.backup.passwordHint")}
              </p>
            </div>
          </div>
        )}
        {isCreating && (
          <BackupBusyNote label={t("settings.backup.creatingStatus")} />
        )}
        <DialogFooter>
          <Button
            variant="soft"
            onClick={() => {
              onClose();
              setPassword("");
            }}
          >
            {t("common.buttons.cancel")}
          </Button>
          <LoadingButton
            isLoading={isCreating}
            disabled={profiles === null || selected.size === 0}
            onClick={() => void create()}
            data-slot="settings-backup-create-confirm"
          >
            {t("settings.backup.createConfirm")}
          </LoadingButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function RestoreBackupDialog({
  isOpen,
  onClose,
}: {
  isOpen: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [archivePath, setArchivePath] = React.useState<string | null>(null);
  const [password, setPassword] = React.useState("");
  const [overwrite, setOverwrite] = React.useState(false);
  const [isRestoring, setIsRestoring] = React.useState(false);

  const pickFile = async () => {
    const picked = await open({
      title: t("settings.backup.openTitle"),
      multiple: false,
      filters: [
        {
          name: t("settings.backup.archiveFilter"),
          extensions: [BACKUP_EXTENSION, "zip"],
        },
      ],
    });
    if (typeof picked === "string") setArchivePath(picked);
  };

  const restore = async () => {
    if (!archivePath) return;
    setIsRestoring(true);
    try {
      const result = await invoke<{
        profiles_restored: number;
        profiles_skipped: number;
        proxies_restored: number;
      }>("restore_backup", {
        archivePath,
        password: password.length > 0 ? password : undefined,
        overwrite,
      });
      showSuccessToast(
        t("toasts.success.backupRestored", {
          count: result.profiles_restored,
        }),
        {
          description:
            result.profiles_skipped > 0
              ? t("settings.backup.restoreSkipped", {
                  count: result.profiles_skipped,
                })
              : undefined,
        },
      );
      setArchivePath(null);
      setPassword("");
      setOverwrite(false);
      onClose();
    } catch (error) {
      showErrorToast(translateBackendError(t, error));
    } finally {
      setIsRestoring(false);
    }
  };

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (isRestoring) return;
        onClose();
        if (!open) {
          setArchivePath(null);
          setPassword("");
          setOverwrite(false);
        }
      }}
    >
      <DialogContent className="max-w-md">
        <DialogHeader>
          <DialogTitle>{t("settings.backup.restoreDialogTitle")}</DialogTitle>
          <DialogDescription>
            {t("settings.backup.restoreDialogDescription")}
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-2">
            <Button variant="soft" size="sm" onClick={() => void pickFile()}>
              {t("settings.backup.chooseFile")}
            </Button>
            <span className="min-w-0 flex-1 truncate text-sm text-muted-foreground">
              {archivePath
                ? archivePath.split(/[\\/]/).pop()
                : t("settings.backup.noFileChosen")}
            </span>
          </div>
          <div className="flex flex-col gap-1">
            <Label htmlFor="restore-password" className="text-sm">
              {t("settings.backup.password")}
            </Label>
            <PasswordField
              id="restore-password"
              value={password}
              onChange={setPassword}
              disabled={isRestoring}
            />
          </div>
          <Label className="flex items-start gap-2 text-sm">
            <Checkbox
              checked={overwrite}
              onCheckedChange={(checked) => setOverwrite(checked === true)}
              className="mt-0.5"
            />
            <span>
              {t("settings.backup.overwrite")}
              <span className="block text-xs text-muted-foreground">
                {t("settings.backup.overwriteHint")}
              </span>
            </span>
          </Label>
        </div>
        {isRestoring && (
          <BackupBusyNote label={t("settings.backup.restoringStatus")} />
        )}
        <DialogFooter>
          <Button
            variant="soft"
            onClick={() => {
              onClose();
              setArchivePath(null);
              setPassword("");
              setOverwrite(false);
            }}
          >
            {t("common.buttons.cancel")}
          </Button>
          <LoadingButton
            isLoading={isRestoring}
            disabled={!archivePath}
            onClick={() => void restore()}
            data-slot="settings-backup-restore-confirm"
          >
            {t("settings.backup.restoreConfirm")}
          </LoadingButton>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
