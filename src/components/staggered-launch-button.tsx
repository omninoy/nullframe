"use client";

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import * as React from "react";
import { useTranslation } from "react-i18next";
import { LuTimer } from "react-icons/lu";
import { ACTION_BAR_BUTTON_CLASS } from "@/components/data-table-action-bar";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Progress } from "@/components/ui/progress";
import { translateBackendError } from "@/lib/backend-errors";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import { cn } from "@/lib/utils";

/** The pause between launches, in seconds. */
const DEFAULT_DELAY_SECS = 3;
const MIN_DELAY_SECS = 1;
const MAX_DELAY_SECS = 600;

/**
 * One-tap pauses that cover the common "don't hammer the machine" choices.
 * The field itself still takes any value in range; these only fill it.
 */
const DELAY_PRESET_SECS = [1, 3, 5, 10, 30, 60];

interface StaggeredProgress {
  done: number;
  total: number;
  launched: number;
  failed: number;
  cancelled: boolean;
}

/**
 * Launches the selected profiles one at a time with a pause between launches,
 * so the machine and the network never take the whole batch at once. The
 * popover takes the pause in seconds; while the queue runs it shows its
 * progress and a cancel, and the launch itself continues in the backend.
 */
export function StaggeredLaunchButton({
  profileIds,
  profileNames,
  disabled,
}: {
  profileIds: string[];
  /** Names parallel to `profileIds`, for naming the profile the queue is on. */
  profileNames?: string[];
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = React.useState(false);
  const [delaySecs, setDelaySecs] = React.useState(String(DEFAULT_DELAY_SECS));
  const [isLaunching, setIsLaunching] = React.useState(false);
  const [progress, setProgress] = React.useState<StaggeredProgress | null>(
    null,
  );
  // The pause the running queue was started with, kept as its own state so
  // the countdown below survives edits to the field while a queue runs.
  const [activeDelayMs, setActiveDelayMs] = React.useState(
    DEFAULT_DELAY_SECS * 1000,
  );
  // Seconds until the queue launches its next profile. Reset on every progress
  // event (each one lands right after a launch), so it always counts down the
  // same pause the backend sleeps.
  const [secondsToNext, setSecondsToNext] = React.useState<number | null>(null);

  // The queue outlives the popover and this table: the listener follows the
  // run from start to finish, and the popover reopens onto its progress.
  React.useEffect(() => {
    if (!isLaunching) return;
    const stop = listen<StaggeredProgress>(
      "profiles-staggered-launch-progress",
      (event) => setProgress(event.payload),
    );
    return () => {
      void stop.then((unlisten) => unlisten());
    };
  }, [isLaunching]);

  // The "next profile in Ns" tick. Restarted by every progress event because
  // `progress` is a fresh object each time; a 250 ms step keeps the last
  // second from lingering on screen for a whole interval.
  React.useEffect(() => {
    if (!isLaunching || !progress || progress.done >= progress.total) {
      setSecondsToNext(null);
      return;
    }
    const totalSecs = Math.max(1, Math.round(activeDelayMs / 1000));
    setSecondsToNext(totalSecs);
    const startedAt = Date.now();
    const timer = window.setInterval(() => {
      const elapsed = Math.floor((Date.now() - startedAt) / 1000);
      const left = Math.max(0, totalSecs - elapsed);
      setSecondsToNext(left);
      if (left === 0) window.clearInterval(timer);
    }, 250);
    return () => {
      window.clearInterval(timer);
    };
  }, [isLaunching, progress, activeDelayMs]);

  const start = async () => {
    const parsed = Number.parseInt(delaySecs, 10);
    const delayMs =
      Math.max(
        MIN_DELAY_SECS,
        Math.min(
          MAX_DELAY_SECS,
          Number.isNaN(parsed) ? DEFAULT_DELAY_SECS : parsed,
        ),
      ) * 1000;
    setActiveDelayMs(delayMs);
    setIsLaunching(true);
    setProgress({
      done: 0,
      total: profileIds.length,
      launched: 0,
      failed: 0,
      cancelled: false,
    });
    try {
      await invoke("staggered_launch_profiles", {
        profileIds,
        delayMs,
      });
      showSuccessToast(t("profiles.staggered.finishedTitle"), {
        description: t("profiles.staggered.finishedBody", {
          count: profileIds.length,
        }),
      });
    } catch (error) {
      showErrorToast(
        t("profiles.staggered.launchFailed", {
          error: translateBackendError(t, error),
        }),
      );
    } finally {
      setIsLaunching(false);
      setProgress(null);
      setSecondsToNext(null);
    }
  };

  const cancel = () => {
    void invoke("cancel_staggered_launch");
  };

  const done = progress?.done ?? 0;
  const total = progress?.total ?? profileIds.length;
  const percent = total > 0 ? Math.round((done / total) * 100) : 0;
  // The queue walks the ids in order, so the entry just past the last done is
  // the launch in waiting; hidden the moment the queue is finished.
  const nextProfileName =
    progress && progress.done < progress.total
      ? (profileNames?.[progress.done] ?? null)
      : null;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <span className="relative inline-flex">
        <PopoverTrigger asChild>
          <Button
            data-bulk-action="staggered-launch"
            variant="subtle"
            size="sm"
            className={cn(
              "gap-1.5 rounded-lg text-xs [&>svg]:size-3.5 h-7",
              ACTION_BAR_BUTTON_CLASS,
            )}
            aria-label={t("profiles.actionBar.staggeredLaunch")}
            disabled={disabled}
          >
            <LuTimer />
            <span>{t("profiles.actionBar.staggeredLaunch")}</span>
          </Button>
        </PopoverTrigger>
      </span>
      <PopoverContent align="center" className="w-64 p-3">
        {isLaunching ? (
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">
              {t("profiles.staggered.progress", { done, total })}
            </p>
            <Progress value={percent} />
            <div className="flex items-center justify-between gap-2 text-xs tabular-nums">
              <p className="text-muted-foreground">
                {t("profiles.staggered.counts", {
                  launched: progress?.launched ?? 0,
                  failed: progress?.failed ?? 0,
                })}
              </p>
              {secondsToNext !== null && (
                <span className="inline-flex shrink-0 items-center gap-1 text-foreground">
                  <LuTimer className="size-3 text-muted-foreground" />
                  {t("profiles.staggered.nextIn", { secs: secondsToNext })}
                </span>
              )}
            </div>
            {nextProfileName && (
              <p className="truncate text-xs text-muted-foreground">
                {t("profiles.staggered.nextProfile", {
                  name: nextProfileName,
                })}
              </p>
            )}
            <Button variant="outline" size="sm" onClick={cancel}>
              {t("profiles.staggered.cancel")}
            </Button>
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            <label
              htmlFor="staggered-delay"
              className="text-xs text-muted-foreground"
            >
              {t("profiles.staggered.delayLabel")}
            </label>
            <Input
              id="staggered-delay"
              type="number"
              min={MIN_DELAY_SECS}
              max={MAX_DELAY_SECS}
              value={delaySecs}
              onChange={(event) => setDelaySecs(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void start();
                }
              }}
            />
            {/* One-tap pauses: filling the field beats typing it, and the
                active chip doubles as "this is what will run". */}
            <div
              role="group"
              aria-label={t("profiles.staggered.delayLabel")}
              className="flex flex-wrap gap-1"
            >
              {DELAY_PRESET_SECS.map((preset) => {
                const active = delaySecs === String(preset);
                return (
                  <button
                    key={preset}
                    type="button"
                    aria-pressed={active}
                    title={t("profiles.staggered.secondsShort", {
                      secs: preset,
                    })}
                    onClick={() => setDelaySecs(String(preset))}
                    className={cn(
                      "h-6 cursor-pointer rounded-md px-2 text-[11px] font-medium tabular-nums transition-colors duration-100",
                      active
                        ? "bg-foreground text-background"
                        : "bg-foreground/5 text-muted-foreground hover:bg-foreground/12 hover:text-foreground",
                    )}
                  >
                    {t("profiles.staggered.secondsShort", { secs: preset })}
                  </button>
                );
              })}
            </div>
            <p className="text-xs text-muted-foreground">
              {t("profiles.staggered.delayHint")}
            </p>
            <Button
              size="sm"
              onClick={() => void start()}
              disabled={profileIds.length === 0}
            >
              {t("profiles.staggered.start", { count: profileIds.length })}
            </Button>
          </div>
        )}
      </PopoverContent>
    </Popover>
  );
}
