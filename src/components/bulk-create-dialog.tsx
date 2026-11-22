"use client";

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import * as React from "react";
import { useTranslation } from "react-i18next";
import {
  LuChevronRight,
  LuFileSpreadsheet,
  LuFileUp,
  LuListPlus,
  LuLoaderCircle,
  LuUpload,
} from "react-icons/lu";
import {
  AnimatedDisclosureChevron,
  AnimatedDisclosureContent,
} from "@/components/ui/animated-disclosure";
import { AutoHeight } from "@/components/ui/auto-height";
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
import { Progress } from "@/components/ui/progress";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { translateBackendError } from "@/lib/backend-errors";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import { cn } from "@/lib/utils";
import type { BulkCreateResult, BulkProfileRecord } from "@/types";

/** How long typing pauses before the pasted text is parsed for preview. */
const PARSE_DEBOUNCE_MS = 250;
/** How many preview rows render before "and more". */
const PREVIEW_LIMIT = 100;
/** Records in one run, mirroring the backend's own ceiling. */
const MAX_RECORDS = 100;

/**
 * A worked CSV and JSON example beside each other, so the format is readable
 * at a glance instead of reverse-engineered from the placeholder. The data is
 * deliberately identical in both: the same three profiles, so the overlap
 * teaches the two shapes rather than two datasets.
 */
const EXAMPLE_CSV = `name,proxy,os,tags,notes
Shop,pool-1,windows,work|vip,Buyer account
Mail,192.168.1.1:8000,macos,,Spam check`;

const EXAMPLE_JSON = `[
  { "name": "Shop", "proxy": "pool-1", "os": "windows", "tags": ["work", "vip"] },
  { "name": "Mail", "proxy": "192.168.1.1:8000", "os": "macos" }
]`;

interface BulkCreateDialogProps {
  isOpen: boolean;
  onClose: () => void;
}

export function BulkCreateDialog({ isOpen, onClose }: BulkCreateDialogProps) {
  const { t } = useTranslation();
  const [tab, setTab] = React.useState("paste");
  const [text, setText] = React.useState("");
  const [fileName, setFileName] = React.useState<string | null>(null);
  const [dragOver, setDragOver] = React.useState(false);
  const [records, setRecords] = React.useState<BulkProfileRecord[]>([]);
  const [autoAssignProxies, setAutoAssignProxies] = React.useState(true);
  const [examplesOpen, setExamplesOpen] = React.useState(false);
  const [isParsing, setIsParsing] = React.useState(false);
  const [isCreating, setIsCreating] = React.useState(false);
  const [progress, setProgress] = React.useState<{
    done: number;
    total: number;
  } | null>(null);
  const parseTokenRef = React.useRef(0);

  // Every change to the text — typed or loaded from a file — re-parses for
  // the preview. The last request wins, so slow parses never race.
  React.useEffect(() => {
    if (!isOpen) return;
    const trimmed = text.trim();
    const token = ++parseTokenRef.current;
    if (!trimmed) {
      setRecords([]);
      setIsParsing(false);
      return;
    }
    setIsParsing(true);
    const timer = window.setTimeout(async () => {
      try {
        const parsed = await invoke<BulkProfileRecord[]>("bulk_parse_table", {
          text,
        });
        if (token === parseTokenRef.current) setRecords(parsed);
      } catch {
        if (token === parseTokenRef.current) setRecords([]);
      } finally {
        if (token === parseTokenRef.current) setIsParsing(false);
      }
    }, PARSE_DEBOUNCE_MS);
    return () => {
      window.clearTimeout(timer);
    };
  }, [text, isOpen]);

  React.useEffect(() => {
    if (isOpen) return;
    setTab("paste");
    setText("");
    setFileName(null);
    setRecords([]);
    setAutoAssignProxies(true);
    setIsCreating(false);
    setProgress(null);
  }, [isOpen]);

  const readFile = React.useCallback(
    async (path: string, name: string) => {
      try {
        const { readTextFile } = await import("@tauri-apps/plugin-fs");
        const content = await readTextFile(path);
        setFileName(name);
        setText(content);
        setTab("paste");
      } catch (error) {
        console.error("Failed to read the bulk file:", error);
        showErrorToast(t("bulkCreate.fileReadFailed"));
      }
    },
    [t],
  );

  const pickFile = async () => {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        title: t("bulkCreate.filePickerTitle"),
        filters: [
          {
            name: t("bulkCreate.filePickerFilter"),
            extensions: ["csv", "tsv", "txt", "json"],
          },
        ],
      });
      if (selected && typeof selected === "string") {
        await readFile(selected, selected.split(/[\\/]/).pop() ?? selected);
      }
    } catch (error) {
      console.error("Failed to open the file picker:", error);
    }
  };

  const onDrop = async (event: React.DragEvent) => {
    event.preventDefault();
    setDragOver(false);
    const file = event.dataTransfer.files?.[0];
    if (!file) return;
    // A drag gives a File, not a path; read it in the page context. The
    // Tauri fs plugin reads paths, so the browser File API serves here.
    const content = await file.text();
    setFileName(file.name);
    setText(content);
    setTab("paste");
  };

  const create = async () => {
    if (records.length === 0 || isCreating) return;
    setIsCreating(true);
    setProgress({ done: 0, total: records.length });
    const stopListening = await listen<{ done: number; total: number }>(
      "profiles-bulk-table-progress",
      (event) => setProgress(event.payload),
    );
    try {
      const result = await invoke<BulkCreateResult>(
        "bulk_create_profiles_from_table",
        {
          records,
          autoAssignProxies,
        },
      );
      const created = result.created.length;
      const outcome = result.cancelled
        ? t("bulkCreate.cancelledSummary", { count: created })
        : result.error
          ? t("bulkCreate.stoppedEarly", {
              count: created,
              error: translateBackendError(t, result.error),
            })
          : t("bulkCreate.created", { count: created });
      if (result.error && created === 0) showErrorToast(outcome);
      else showSuccessToast(outcome);
      onClose();
    } catch (error) {
      showErrorToast(
        t("bulkCreate.createFailed", {
          error: translateBackendError(t, error),
        }),
      );
    } finally {
      stopListening();
      setIsCreating(false);
      setProgress(null);
    }
  };

  const cancel = () => {
    void invoke("cancel_bulk_table_create");
  };

  const total = records.length;
  const overLimit = total > MAX_RECORDS;
  const previewRows = records.slice(0, PREVIEW_LIMIT);
  const done = progress?.done ?? 0;
  const percent =
    progress && progress.total > 0
      ? Math.round((done / progress.total) * 100)
      : 0;

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => !open && !isCreating && onClose()}
    >
      <DialogContent
        className="flex max-h-[85vh] w-[min(720px,calc(100vw-2rem))] max-w-[720px] flex-col"
        data-slot="bulk-create-dialog"
      >
        <DialogHeader className="shrink-0">
          <DialogTitle>{t("bulkCreate.title")}</DialogTitle>
          <DialogDescription>{t("bulkCreate.description")}</DialogDescription>
        </DialogHeader>

        {isCreating ? (
          <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 py-8">
            <p className="text-sm text-muted-foreground">
              {t("bulkCreate.creating", {
                done,
                total: progress?.total ?? total,
              })}
            </p>
            <Progress value={percent} className="w-full" />
            <Button variant="outline" size="sm" onClick={cancel}>
              {t("bulkCreate.cancel")}
            </Button>
          </div>
        ) : (
          <>
            {/* Scrollable middle: on short windows the dialog contents must
                scroll as a whole instead of letting flex shrink tabs and
                overlap them with the rows below. */}
            <div className="min-h-0 flex-1 overflow-y-auto">
              <Tabs
                value={tab}
                onValueChange={setTab}
                className="@container flex flex-col"
              >
                <TabsList className="w-fit">
                  <TabsTrigger value="paste">
                    <LuListPlus className="mr-1.5 size-3.5" />
                    {t("bulkCreate.pasteTab")}
                  </TabsTrigger>
                  <TabsTrigger value="file">
                    <LuFileUp className="mr-1.5 size-3.5" />
                    {t("bulkCreate.fileTab")}
                  </TabsTrigger>
                </TabsList>

                <TabsContent value="paste" className="mt-2 flex flex-col">
                  <Textarea
                    value={text}
                    onChange={(event) => setText(event.target.value)}
                    placeholder={t("bulkCreate.pastePlaceholder")}
                    className="h-36 resize-none font-mono text-xs"
                    spellCheck={false}
                  />
                  <p className="mt-1.5 text-xs text-muted-foreground">
                    {t("bulkCreate.formatHint")}
                  </p>

                  {/* A worked example in both accepted shapes, collapsible so it
                    never pushes the preview off screen. Same three profiles in
                    each, so the two formats teach themselves by comparison. */}
                  <div className="mt-2 rounded-xl bg-foreground/5">
                    <button
                      type="button"
                      onClick={() => setExamplesOpen((open) => !open)}
                      aria-expanded={examplesOpen}
                      className="flex w-full cursor-pointer items-center gap-2 rounded-xl px-3 py-2 text-left text-xs font-medium text-foreground transition-colors duration-100 hover:bg-foreground/5"
                    >
                      <AnimatedDisclosureChevron open={examplesOpen}>
                        <LuChevronRight className="size-3.5" />
                      </AnimatedDisclosureChevron>
                      {t("bulkCreate.examplesToggle")}
                    </button>
                    <AutoHeight deps={[examplesOpen]}>
                      <AnimatedDisclosureContent
                        open={examplesOpen}
                        className="grid grid-cols-1 gap-2 border-t border-foreground/8 px-3 py-3 @sm:grid-cols-2"
                      >
                        <div className="min-w-0">
                          <p className="mb-1 text-[11px] font-medium text-muted-foreground">
                            {t("bulkCreate.exampleCsvTitle")}
                          </p>
                          <pre className="overflow-x-auto rounded-lg bg-background px-2.5 py-2 font-mono text-[11px] leading-relaxed text-foreground">
                            {EXAMPLE_CSV}
                          </pre>
                        </div>
                        <div className="min-w-0">
                          <p className="mb-1 text-[11px] font-medium text-muted-foreground">
                            {t("bulkCreate.exampleJsonTitle")}
                          </p>
                          <pre className="overflow-x-auto rounded-lg bg-background px-2.5 py-2 font-mono text-[11px] leading-relaxed text-foreground">
                            {EXAMPLE_JSON}
                          </pre>
                        </div>
                      </AnimatedDisclosureContent>
                    </AutoHeight>
                  </div>
                </TabsContent>

                <TabsContent value="file" className="mt-2">
                  <div
                    role="button"
                    tabIndex={0}
                    aria-label={t("bulkCreate.filePickerTitle")}
                    onClick={() => void pickFile()}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" || event.key === " ") {
                        event.preventDefault();
                        void pickFile();
                      }
                    }}
                    onDragOver={(event) => {
                      event.preventDefault();
                      setDragOver(true);
                    }}
                    onDragLeave={() => setDragOver(false)}
                    onDrop={(event) => void onDrop(event)}
                    className={cn(
                      "flex h-36 cursor-pointer flex-col items-center justify-center gap-2 rounded-lg border border-dashed border-border text-muted-foreground transition-colors hover:bg-foreground/5",
                      dragOver && "border-primary bg-primary/5 text-foreground",
                    )}
                  >
                    <LuUpload className="size-6" />
                    <p className="text-sm">{t("bulkCreate.dropHere")}</p>
                    <p className="text-xs">{t("bulkCreate.fileFormats")}</p>
                  </div>
                </TabsContent>
              </Tabs>

              <div className="mt-3 flex items-center gap-2">
                <Checkbox
                  id="bulk-create-auto-proxy"
                  checked={autoAssignProxies}
                  onCheckedChange={(checked) =>
                    setAutoAssignProxies(checked === true)
                  }
                />
                <label
                  htmlFor="bulk-create-auto-proxy"
                  className="text-sm text-muted-foreground"
                >
                  {t("bulkCreate.autoAssignProxies")}
                </label>
              </div>

              <div className="mt-3 shrink-0 overflow-hidden rounded-lg border border-border/60">
                <div className="flex items-center justify-between gap-2 border-b border-border/60 px-3 py-1.5">
                  <span className="flex min-w-0 items-center gap-1.5 text-xs font-medium text-muted-foreground">
                    <LuFileSpreadsheet className="size-3.5 shrink-0" />
                    <span className="truncate">
                      {fileName
                        ? t("bulkCreate.previewFromFile", { file: fileName })
                        : t("bulkCreate.previewTitle")}
                    </span>
                  </span>
                  {/* The parse verdict as a badge: green-ish while it reads,
                    spinning while it parses, red the moment the run would be
                    refused — the state the confirm button acts on. */}
                  {isParsing ? (
                    <Badge
                      variant="soft"
                      className="shrink-0 gap-1.5 tabular-nums"
                    >
                      <LuLoaderCircle className="size-3 animate-spin" />
                      {t("bulkCreate.parsing")}
                    </Badge>
                  ) : overLimit ? (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Badge
                          variant="destructive"
                          className="shrink-0 cursor-default tabular-nums"
                        >
                          {t("bulkCreate.recordCount", { count: total })}
                        </Badge>
                      </TooltipTrigger>
                      <TooltipContent>
                        {t("bulkCreate.tooManyRecords", { max: MAX_RECORDS })}
                      </TooltipContent>
                    </Tooltip>
                  ) : (
                    <Badge variant="soft" className="shrink-0 tabular-nums">
                      {t("bulkCreate.recordCount", { count: total })}
                    </Badge>
                  )}
                </div>
                <div className="max-h-48 overflow-y-auto">
                  <Table>
                    <TableHeader className="sticky top-0 bg-background">
                      <TableRow>
                        <TableHead className="h-8 w-2/5">
                          {t("bulkCreate.column.name")}
                        </TableHead>
                        <TableHead className="h-8 w-1/5">
                          {t("bulkCreate.column.proxy")}
                        </TableHead>
                        <TableHead className="h-8 w-1/6">
                          {t("bulkCreate.column.os")}
                        </TableHead>
                        <TableHead className="h-8">
                          {t("bulkCreate.column.tags")}
                        </TableHead>
                        <TableHead className="h-8">
                          {t("bulkCreate.column.notes")}
                        </TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {previewRows.map((record, index) => (
                        <TableRow key={`${record.name}-${index}`}>
                          <TableCell className="h-8 truncate py-1 text-xs">
                            {record.name}
                          </TableCell>
                          <TableCell className="h-8 truncate py-1 text-xs">
                            {record.proxy || "—"}
                          </TableCell>
                          <TableCell className="h-8 truncate py-1 text-xs">
                            {record.os || "—"}
                          </TableCell>
                          <TableCell className="h-8 truncate py-1 text-xs">
                            {record.tags && record.tags.length > 0
                              ? record.tags.join(", ")
                              : "—"}
                          </TableCell>
                          <TableCell className="h-8 truncate py-1 text-xs">
                            {record.notes || "—"}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                  {total === 0 && (
                    <div className="flex flex-col items-center gap-1.5 px-3 py-6 text-center">
                      <LuListPlus className="size-5 text-muted-foreground/60" />
                      <p className="text-xs text-muted-foreground">
                        {isParsing
                          ? t("bulkCreate.parsing")
                          : t("bulkCreate.noRecords")}
                      </p>
                    </div>
                  )}
                  {total > PREVIEW_LIMIT && (
                    <p className="border-t border-border/60 px-3 py-2 text-center text-xs text-muted-foreground">
                      {t("bulkCreate.moreRecords", {
                        count: total - PREVIEW_LIMIT,
                      })}
                    </p>
                  )}
                  {overLimit && (
                    <p className="border-t border-border/60 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                      {t("bulkCreate.tooManyRecords", { max: MAX_RECORDS })}
                    </p>
                  )}
                </div>
              </div>
            </div>

            <DialogFooter className="mt-4 shrink-0">
              <Button variant="outline" onClick={onClose}>
                {t("common.buttons.cancel")}
              </Button>
              <Button
                onClick={() => void create()}
                disabled={total === 0 || overLimit || isParsing}
              >
                {t("bulkCreate.create", { count: total })}
              </Button>
            </DialogFooter>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
