"use client";

import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { motion } from "motion/react";
import type * as React from "react";
import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useTranslation } from "react-i18next";
import {
  LuActivity,
  LuCheck,
  LuChevronDown,
  LuChevronRight,
  LuClock,
  LuEllipsis,
  LuFlame,
  LuFolder,
  LuGlobe,
  LuLayoutTemplate,
  LuLink,
  LuLoaderCircle,
  LuLock,
  LuLockOpen,
  LuMinus,
  LuMonitor,
  LuPlus,
  LuShieldCheck,
  LuTrash2,
  LuTriangleAlert,
  LuX,
} from "react-icons/lu";
import {
  nextProfileName,
  useCreateWhenBrowserReady,
} from "@/components/create-profile-wait";
import { DnsServerField } from "@/components/dns-server-field";
import { EphemeralStorageNotice } from "@/components/ephemeral-storage-notice";
import { FlagIcon } from "@/components/flag-icon";
import { LoadingButton } from "@/components/loading-button";
import { ProxyFormDialog } from "@/components/proxy-form-dialog";
import {
  AnimatedDisclosureChevron,
  AnimatedDisclosureContent,
} from "@/components/ui/animated-disclosure";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
} from "@/components/ui/command";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { SoftFields } from "@/components/ui/field-variant";
import { FOCUS_RING } from "@/components/ui/focus-ring";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { ProBadge } from "@/components/ui/pro-badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import {
  getCachedVpnResult,
  loadAllCachedVpnChecks,
  useVpnCheckVersion,
} from "@/components/vpn-check-button";
import { WayfernConfigForm } from "@/components/wayfern-config-form";
import { useBrowserDownload } from "@/hooks/use-browser-download";
import { useGroupEvents } from "@/hooks/use-group-events";
import { useMotionAllowed } from "@/hooks/use-motion-allowed";
import { useProfileTemplates } from "@/hooks/use-profile-templates";
import { useProxyChains } from "@/hooks/use-proxy-chains";
import { useProxyEvents } from "@/hooks/use-proxy-events";
import { useEntityTrash } from "@/hooks/use-trash-events";
import { useVpnEvents } from "@/hooks/use-vpn-events";
import { useVpnSubscriptions } from "@/hooks/use-vpn-subscriptions";
import { translateBackendError } from "@/lib/backend-errors";
import { getOSDisplayName, getOSIcon } from "@/lib/browser-utils";
import { DNS_BLOCKLIST_LEVELS } from "@/lib/dns-blocklist-levels";
import { extractCountryCode } from "@/lib/flag-utils";
import { MOTION_DURATION, MOTION_EASE_OUT, MOTION_SPRING } from "@/lib/motion";
import {
  getCachedProxyResult,
  loadAllCachedProxyChecks,
  useProxyCheckVersion,
} from "@/lib/proxy-check-store";
import { startPagesFromText } from "@/lib/start-pages";
import { notifySystem } from "@/lib/system-notify";
import { showErrorToast, showSuccessToast } from "@/lib/toast-utils";
import { cn } from "@/lib/utils";
import type {
  BrowserReleaseTypes,
  BulkCreateResult,
  ProfileTemplate,
  ScreenSize,
  TemplateSettings,
  WayfernConfig,
  WayfernOS,
} from "@/types";

const PASSWORD_MIN_LEN = 8;
/** The most profiles one bulk creation makes; the backend holds the same cap. */
const MAX_BULK_CREATE = 100;

const OS_OPTIONS: WayfernOS[] = ["windows", "macos", "linux", "android", "ios"];
/** The claims whose screen can be chosen; a phone's screen follows its model. */
const SCREEN_CHOICE_OS: WayfernOS[] = ["windows", "macos", "linux"];
/** More screens than this get a search field. */
const SCREEN_SEARCH_FROM = 8;

const getCurrentOS = (): WayfernOS => {
  if (typeof navigator === "undefined") return "linux";
  const platform = navigator.platform.toLowerCase();
  if (platform.includes("win")) return "windows";
  if (platform.includes("mac")) return "macos";
  return "linux";
};

const defaultWayfernConfig = (): WayfernConfig => ({
  os: getCurrentOS(),
  screen_max_width:
    typeof window === "undefined" ? undefined : window.screen.width,
  screen_max_height:
    typeof window === "undefined" ? undefined : window.screen.height,
});

interface CreateProfileDialogProps {
  isOpen: boolean;
  onClose: () => void;
  onCreateProfile: (profileData: {
    name: string;
    browserStr: "wayfern";
    version: string;
    releaseType: string;
    proxyId?: string;
    relayProxyId?: string;
    vpnId?: string;
    wayfernConfig?: WayfernConfig;
    /** `null`: the person picked "No group". */
    groupId?: string | null;
    extensionGroupId?: string;
    ephemeral?: boolean;
    dnsBlocklist?: string;
    dnsServer?: string;
    startUrls?: string[];
    launchHook?: string;
    password?: string;
  }) => Promise<void>;
  selectedGroupId?: string;
  crossOsUnlocked?: boolean;
  /** Names already taken, for the "Profile N" the name field starts with. */
  profileNames?: readonly string[];
}

export function CreateProfileDialog({
  isOpen,
  onClose,
  onCreateProfile,
  selectedGroupId,
  crossOsUnlocked = false,
  profileNames = [],
}: CreateProfileDialogProps) {
  const { t } = useTranslation();
  const proxyListboxId = useId();
  const proxyLabelId = useId();
  const proxyValueId = useId();
  const osListboxId = useId();
  const osLabelId = useId();
  const osValueId = useId();
  const advancedOptionsId = useId();
  const motionAllowed = useMotionAllowed();
  const groupListboxId = useId();
  const groupValueId = useId();
  const [profileName, setProfileName] = useState("");
  const nameInputRef = useRef<HTMLInputElement>(null);
  // The "Profile N" the field started with; a template's name replaces it.
  const prefilledNameRef = useRef("");
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [selectedProxyId, setSelectedProxyId] = useState<string>();
  const [proxyPopoverOpen, setProxyPopoverOpen] = useState(false);
  const [proxyPopoverTab, setProxyPopoverTab] = useState<"proxies" | "vpn">(
    "proxies",
  );
  const { chains } = useProxyChains();
  const {
    subscriptions,
    nodes: subscriptionNodes,
    pingSingleNode,
  } = useVpnSubscriptions();
  const [expandedSubIds, setExpandedSubIds] = useState<Record<string, boolean>>(
    {},
  );
  const [pingingNodeId, setPingingNodeId] = useState<string | null>(null);
  const [morePopoverOpen, setMorePopoverOpen] = useState(false);
  // Where the new profiles go: the group the list shows, until changed.
  const [destinationGroupId, setDestinationGroupId] = useState<string | null>(
    null,
  );
  const [osPopoverOpen, setOsPopoverOpen] = useState(false);
  const [showProxyForm, setShowProxyForm] = useState(false);
  const [dnsBlocklist, setDnsBlocklist] = useState("");
  const [dnsServer, setDnsServer] = useState("");
  const [dnsServerValid, setDnsServerValid] = useState(true);
  const [startPages, setStartPages] = useState("");
  const [launchHook, setLaunchHook] = useState("");
  const [wayfernConfig, setWayfernConfig] =
    useState<WayfernConfig>(defaultWayfernConfig);
  const [isCreating, setIsCreating] = useState(false);
  const [ephemeral, setEphemeral] = useState(false);
  const templates = useProfileTemplates();
  const { request: requestTemplateTrash, remove: removeTemplates } =
    useEntityTrash("template");
  const [appliedTemplate, setAppliedTemplate] =
    useState<ProfileTemplate | null>(null);
  const [count, setCount] = useState(1);
  const [progress, setProgress] = useState<{
    done: number;
    total: number;
  } | null>(null);
  const [allowance, setAllowance] = useState<ProfileCreationAllowance | null>(
    null,
  );

  const loadAllowance = useCallback(async () => {
    try {
      setAllowance(
        await invoke<ProfileCreationAllowance>(
          "get_profile_creation_allowance",
        ),
      );
    } catch (error) {
      console.error("Failed to read the profile creation allowance:", error);
    }
  }, []);

  useEffect(() => {
    if (isOpen) void loadAllowance();
  }, [isOpen, loadAllowance]);

  // The name starts as the next free "Profile N", selected, so typing
  // replaces it and Create works at once. It is set while rendering the
  // opening, so the field has it when it mounts and takes the focus.
  const [openedFor, setOpenedFor] = useState(false);
  if (isOpen !== openedFor) {
    setOpenedFor(isOpen);
    if (isOpen) {
      const name = nextProfileName(profileNames, (n) =>
        t("createProfile.defaultName", { n }),
      );
      prefilledNameRef.current = name;
      setProfileName((current) => current || name);
    }
  }
  const selectPrefilledName = (input: HTMLInputElement | null) => {
    if (input && input.value === prefilledNameRef.current) input.select();
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: opening is the trigger
  useLayoutEffect(() => {
    if (isOpen) selectPrefilledName(nameInputRef.current);
  }, [isOpen]);

  // A browser that starts or stops changes whether a fingerprint can be made.
  useEffect(() => {
    if (!isOpen) return;
    let closed = false;
    let unlisten: (() => void) | undefined;
    void listen("profile-running-changed", () => void loadAllowance()).then(
      (stop) => {
        if (closed) stop();
        else unlisten = stop;
      },
    );
    return () => {
      closed = true;
      unlisten?.();
    };
  }, [isOpen, loadAllowance]);

  // The window frees a slot by itself, so look again once it should have.
  useEffect(() => {
    const wait = allowance?.retry_after_secs;
    if (!isOpen || wait == null) return;
    const timer = window.setTimeout(
      () => void loadAllowance(),
      wait * 1000 + 500,
    );
    return () => window.clearTimeout(timer);
  }, [isOpen, allowance, loadAllowance]);

  const overAllowance =
    allowance !== null && allowance.used >= allowance.per_hour;
  const hourlyCapReached = overAllowance && allowance.enforcement === "hard";
  const creatingQuickly = overAllowance && allowance.enforcement === "soft";
  const closeABrowser = allowance?.close_a_browser === true;

  // The wait in the hourly-limit notice counts down by the minute.
  useEffect(() => {
    if (!isOpen || !hourlyCapReached) return;
    const timer = window.setInterval(() => void loadAllowance(), 60_000);
    return () => window.clearInterval(timer);
  }, [isOpen, hourlyCapReached, loadAllowance]);

  // A batch stops at the hourly allowance on every plan, so it is never
  // offered past what is left.
  const maxCount =
    allowance === null
      ? MAX_BULK_CREATE
      : Math.max(1, Math.min(MAX_BULK_CREATE, allowance.remaining));
  const many = count > 1;
  useEffect(() => {
    setCount((current) => Math.min(current, maxCount));
  }, [maxCount]);
  const [enablePassword, setEnablePassword] = useState(false);
  const [password, setPassword] = useState("");
  const [passwordConfirm, setPasswordConfirm] = useState("");
  const [passwordError, setPasswordError] = useState<string | null>(null);
  const [selectedExtensionGroupId, setSelectedExtensionGroupId] =
    useState<string>();
  const [extensionGroups, setExtensionGroups] = useState<
    { id: string; name: string; extension_ids: string[] }[]
  >([]);
  const [releaseTypes, setReleaseTypes] = useState<BrowserReleaseTypes>({});
  const [isLoadingReleaseTypes, setIsLoadingReleaseTypes] = useState(false);
  const [releaseTypesError, setReleaseTypesError] = useState(false);
  const versionRequestRef = useRef(0);
  // The dialog stays mounted; its lists load only while it is open.
  const { storedProxies } = useProxyEvents({ enabled: isOpen });
  const { vpnConfigs } = useVpnEvents({ enabled: isOpen });
  const proxyCheckVersion = useProxyCheckVersion();
  const vpnCheckVersion = useVpnCheckVersion();

  useEffect(() => {
    if (isOpen) {
      void loadAllCachedProxyChecks();
      void loadAllCachedVpnChecks();
    }
  }, [isOpen]);

  const sortedProxies = useMemo(() => {
    void proxyCheckVersion;
    return [...storedProxies].sort((a, b) => {
      const resA = getCachedProxyResult(a.id);
      const resB = getCachedProxyResult(b.id);
      const latA =
        resA?.is_valid && typeof resA.latency_ms === "number"
          ? resA.latency_ms
          : Infinity;
      const latB =
        resB?.is_valid && typeof resB.latency_ms === "number"
          ? resB.latency_ms
          : Infinity;
      if (latA !== latB) return latA - latB;
      return a.name.localeCompare(b.name);
    });
  }, [storedProxies, proxyCheckVersion]);

  const sortedVpns = useMemo(() => {
    void vpnCheckVersion;
    return [...vpnConfigs].sort((a, b) => {
      const resA = getCachedVpnResult(a.id);
      const resB = getCachedVpnResult(b.id);
      const latA =
        resA?.is_valid && typeof resA.latency_ms === "number"
          ? resA.latency_ms
          : Infinity;
      const latB =
        resB?.is_valid && typeof resB.latency_ms === "number"
          ? resB.latency_ms
          : Infinity;
      if (latA !== latB) return latA - latB;
      return a.name.localeCompare(b.name);
    });
  }, [vpnConfigs, vpnCheckVersion]);
  const { groups, isLoading: isLoadingGroups } = useGroupEvents({
    enabled: isOpen,
  });
  const {
    isBrowserDownloading,
    downloadBrowser,
    loadDownloadedVersions,
    downloadedVersionsMap,
  } = useBrowserDownload();

  const loadReleaseTypes = useCallback(async () => {
    const request = ++versionRequestRef.current;
    setIsLoadingReleaseTypes(true);
    setReleaseTypesError(false);
    try {
      const releases = await invoke<BrowserReleaseTypes>(
        "get_browser_release_types",
        {
          browserStr: "wayfern",
        },
      );
      await loadDownloadedVersions("wayfern");
      if (versionRequestRef.current === request) {
        setReleaseTypes(releases.stable ? { stable: releases.stable } : {});
      }
    } catch (error) {
      console.error("Failed to load Wayfern release types:", error);
      try {
        const downloaded = await loadDownloadedVersions("wayfern");
        if (versionRequestRef.current === request) {
          setReleaseTypes(downloaded.length ? { stable: downloaded[0] } : {});
          setReleaseTypesError(downloaded.length === 0);
        }
      } catch (error) {
        console.error("Failed to load downloaded Wayfern versions:", error);
        if (versionRequestRef.current === request) setReleaseTypesError(true);
      }
    } finally {
      if (versionRequestRef.current === request)
        setIsLoadingReleaseTypes(false);
    }
  }, [loadDownloadedVersions]);

  useEffect(() => {
    if (isOpen) setDestinationGroupId(groupFromFilter(selectedGroupId));
  }, [isOpen, selectedGroupId]);

  useEffect(() => {
    if (!isOpen) return;
    void loadReleaseTypes();
    void invoke<{ id: string; name: string; extension_ids: string[] }[]>(
      "list_extension_groups",
    )
      .then(setExtensionGroups)
      .catch(() => setExtensionGroups([]));
    void invoke<boolean>("is_geoip_database_available")
      .then(async (available) => {
        if (!available) await invoke("download_geoip_database");
      })
      .catch((error) =>
        console.error("Failed to prepare GeoIP database:", error),
      );
    return () => {
      versionRequestRef.current += 1;
    };
  }, [isOpen, loadReleaseTypes]);

  const downloadedVersions = downloadedVersionsMap.wayfern ?? [];
  const latestVersion = releaseTypes.stable;
  const creatableVersion =
    latestVersion && downloadedVersions.includes(latestVersion)
      ? latestVersion
      : downloadedVersions[0];
  const downloading = isBrowserDownloading("wayfern");
  // Several profiles, or one from a template, go to one batch, which stops at
  // the allowance even where a single profile past it is only a warning.
  const batchRefused =
    allowance !== null &&
    (many || appliedTemplate !== null) &&
    count > allowance.remaining;
  const browserReady = Boolean(creatableVersion) && !downloading;
  const isCreateDisabled =
    !profileName.trim() ||
    (!creatableVersion && !latestVersion && !downloading) ||
    isCreating ||
    hourlyCapReached ||
    closeABrowser ||
    batchRefused ||
    !dnsServerValid;
  const allSubNodes = useMemo(() => {
    return Object.values(subscriptionNodes).flat();
  }, [subscriptionNodes]);
  const selectedVpn = selectedProxyId?.startsWith("vpn-")
    ? vpnConfigs.find((vpn) => vpn.id === selectedProxyId.slice(4))
    : undefined;
  const selectedChain = selectedProxyId?.startsWith("chain-")
    ? chains.find((c) => c.id === selectedProxyId)
    : undefined;
  const selectedSubNode = selectedProxyId?.startsWith("subnode-")
    ? allSubNodes.find((n) => n.id === selectedProxyId.slice(8))
    : undefined;
  const selectedProxy = storedProxies.find(
    (proxy) => proxy.id === selectedProxyId,
  );
  const selectedProxyResult = selectedProxy
    ? getCachedProxyResult(selectedProxy.id)
    : null;
  const selectedCountryCode = selectedProxy
    ? (selectedProxyResult?.country_code ??
      extractCountryCode(selectedProxy.geo_country, selectedProxy.name))
    : selectedVpn
      ? (getCachedVpnResult(selectedVpn.id)?.country_code ??
        extractCountryCode(undefined, selectedVpn.name))
      : selectedSubNode
        ? (selectedSubNode.country_code ??
          extractCountryCode(undefined, selectedSubNode.name))
        : undefined;
  const proxyLabel = selectedChain
    ? `Цепочка — ${selectedChain.name}`
    : selectedVpn
      ? `WG — ${selectedVpn.name}`
      : selectedSubNode
        ? `VPN — ${selectedSubNode.name}`
        : (selectedProxy?.name ?? t("createProfile.proxy.noProxy"));
  const destinationGroup = groups.find(
    (group) => group.id === destinationGroupId,
  );
  // A group deleted since it was picked is no destination; while the list
  // loads, the picked id is kept.
  const groupIdForCreate = destinationGroup
    ? destinationGroup.id
    : isLoadingGroups
      ? destinationGroupId
      : null;
  const hostOs = getCurrentOS();
  const selectedOs = wayfernConfig.os ?? hostOs;
  const crossOs = selectedOs !== hostOs;
  const SelectedOsIcon = getOSIcon(selectedOs);

  const clearPassword = () => {
    setEnablePassword(false);
    setPassword("");
    setPasswordConfirm("");
    setPasswordError(null);
  };

  const handleClose = () => {
    versionRequestRef.current += 1;
    browserWait.cancel();
    setProfileName("");
    setAdvancedOpen(false);
    setSelectedProxyId(undefined);
    setProxyPopoverOpen(false);
    setMorePopoverOpen(false);
    setOsPopoverOpen(false);
    setShowProxyForm(false);
    setDnsBlocklist("");
    setDnsServer("");
    setStartPages("");
    setLaunchHook("");
    setSelectedExtensionGroupId(undefined);
    setReleaseTypes({});
    setIsLoadingReleaseTypes(false);
    setReleaseTypesError(false);
    setWayfernConfig(defaultWayfernConfig());
    setEphemeral(false);
    setAppliedTemplate(null);
    setCount(1);
    setProgress(null);
    clearPassword();
    onClose();
  };

  /** The form as template settings, over the applied template's own extras. */
  const formSettings = (): TemplateSettings => {
    const base = appliedTemplate?.settings;
    const isVpn = selectedProxyId?.startsWith("vpn-") ?? false;
    const proxyId = isVpn ? null : (selectedProxyId ?? null);
    const spread = base?.proxy_ids ?? [];
    return {
      ...base,
      name_prefix: profileName.trim() || null,
      proxy_id: proxyId,
      vpn_id: isVpn ? (selectedProxyId?.slice(4) ?? null) : null,
      // A template that spreads its profiles over several proxies keeps
      // doing so while its first proxy is still the one chosen.
      proxy_ids: proxyId && spread[0] === proxyId ? spread : [],
      group_id: groupIdForCreate,
      extension_group_id: selectedExtensionGroupId ?? null,
      dns_blocklist: dnsBlocklist || null,
      dns_server: dnsServer.trim() || null,
      start_urls: startPagesFromText(startPages),
      launch_hook: launchHook.trim() || null,
      ephemeral,
      wayfern_config: wayfernConfig,
    };
  };

  const applyTemplate = (template: ProfileTemplate | null) => {
    setAppliedTemplate(template);
    if (!template) return;
    const settings = template.settings;
    setSelectedProxyId(
      settings.vpn_id
        ? `vpn-${settings.vpn_id}`
        : (settings.proxy_ids?.[0] ?? settings.proxy_id ?? undefined),
    );
    setWayfernConfig({
      ...defaultWayfernConfig(),
      ...(settings.wayfern_config ?? {}),
    });
    setDnsBlocklist(settings.dns_blocklist ?? "");
    setDnsServer(settings.dns_server ?? "");
    setStartPages((settings.start_urls ?? []).join("\n"));
    setLaunchHook(settings.launch_hook ?? "");
    setSelectedExtensionGroupId(settings.extension_group_id ?? undefined);
    setEphemeral(settings.ephemeral === true);
    if (settings.group_id) setDestinationGroupId(settings.group_id);
    if (settings.ephemeral) clearPassword();
    const name = profileName.trim();
    if (!name || name === prefilledNameRef.current) {
      setProfileName(settings.name_prefix ?? template.name);
    }
  };

  const saveTemplate = async (name: string) => {
    const existing = templates.find(
      (template) => template.name.toLowerCase() === name.toLowerCase(),
    );
    try {
      const saved = existing
        ? await invoke<ProfileTemplate>("update_profile_template", {
            templateId: existing.id,
            name,
            settings: formSettings(),
          })
        : await invoke<ProfileTemplate>("create_profile_template", {
            name,
            settings: formSettings(),
          });
      setAppliedTemplate(saved);
      showSuccessToast(t("templates.saved", { name: saved.name }));
      return true;
    } catch (error) {
      showErrorToast(translateBackendError(t, error));
      return false;
    }
  };

  const afterTemplatesRemoved = (ids: string[]) => {
    if (appliedTemplate && ids.includes(appliedTemplate.id)) {
      setAppliedTemplate(null);
    }
  };

  // A template goes to the trash at once and the toast offers Undo. Only
  // "Delete at once" asks first, inline in the picker.
  const deleteTemplate = async (
    template: ProfileTemplate,
    confirm: () => void,
  ) => {
    afterTemplatesRemoved(await requestTemplateTrash([template], confirm));
  };

  const deleteTemplateNow = async (template: ProfileTemplate) => {
    afterTemplatesRemoved(await removeTemplates([template]));
  };

  const changeCount = (next: number) => {
    setCount(next);
    if (next > 1) clearPassword();
  };

  /** Several profiles, or one from a template: one backend run for both. */
  const createFromSettings = async () => {
    setIsCreating(true);
    setProgress({ done: 0, total: count });
    const stopListening = await listen<{ done: number; total: number }>(
      "profiles-bulk-create-progress",
      (event) => setProgress(event.payload),
    );
    try {
      const result = await invoke<BulkCreateResult>("bulk_create_profiles", {
        settings: formSettings(),
        name: profileName.trim(),
        count,
      });
      const [first] = result.created;
      if (count === 1 && first && enablePassword && !ephemeral) {
        try {
          await invoke("set_profile_password", {
            profileId: first.id,
            password,
          });
        } catch (error) {
          showErrorToast(
            t("errors.setProfilePasswordFailed", {
              error: translateBackendError(t, error),
            }),
          );
        }
      }
      const outcome = result.error
        ? t("templates.stoppedEarly", {
            created: result.created.length,
            total: count,
            error: translateBackendError(t, result.error),
          })
        : t("templates.created", { count: result.created.length });
      if (result.error) showErrorToast(outcome);
      else showSuccessToast(outcome);
      if (count > 1) {
        void notifySystem({ title: t("templates.notifyTitle"), body: outcome });
      }
      handleClose();
    } catch (error) {
      showErrorToast(
        t("errors.createProfileFailed", {
          error: translateBackendError(t, error),
        }),
      );
      void loadAllowance();
    } finally {
      stopListening();
      setIsCreating(false);
      setProgress(null);
    }
  };

  const handleCreate = async () => {
    if (isCreateDisabled) return;
    if (enablePassword && !ephemeral) {
      if (password.length < PASSWORD_MIN_LEN) {
        setPasswordError(
          t("profilePassword.errors.tooShort", { min: PASSWORD_MIN_LEN }),
        );
        return;
      }
      if (password !== passwordConfirm) {
        setPasswordError(t("profilePassword.errors.mismatch"));
        return;
      }
    }
    setPasswordError(null);
    if (!browserReady || !creatableVersion) {
      browserWait.request();
      return;
    }
    if (many || appliedTemplate) {
      await createFromSettings();
      return;
    }
    setIsCreating(true);
    const isVpnSelection = selectedProxyId?.startsWith("vpn-") ?? false;
    const isChainSelection = selectedProxyId?.startsWith("chain-") ?? false;

    let effectiveProxyId: string | undefined = selectedProxyId;
    let effectiveRelayProxyId: string | undefined;

    if (isChainSelection) {
      const chosenChain = chains.find((c) => c.id === selectedProxyId);
      if (chosenChain) {
        effectiveProxyId = chosenChain.hop2_proxy_id;
        effectiveRelayProxyId = chosenChain.hop1_proxy_id;
      }
    }

    try {
      await onCreateProfile({
        name: profileName.trim(),
        browserStr: "wayfern",
        version: creatableVersion,
        releaseType: "stable",
        proxyId: isVpnSelection ? undefined : effectiveProxyId,
        relayProxyId: effectiveRelayProxyId,
        vpnId: isVpnSelection ? selectedProxyId?.slice(4) : undefined,
        wayfernConfig,
        groupId: groupIdForCreate,
        extensionGroupId: selectedExtensionGroupId,
        ephemeral,
        dnsBlocklist: dnsBlocklist || undefined,
        dnsServer: dnsServer.trim() || undefined,
        startUrls: startPagesFromText(startPages),
        launchHook: launchHook.trim() || undefined,
        password: enablePassword && !ephemeral ? password : undefined,
      });
      handleClose();
    } catch (error) {
      console.error("Failed to create profile:", error);
      void loadAllowance();
    } finally {
      setIsCreating(false);
    }
  };

  const handleDownload = async () => {
    if (!latestVersion) return;
    try {
      await downloadBrowser("wayfern", latestVersion);
    } catch (error) {
      console.error("Failed to download Wayfern:", error);
    }
  };

  const browserWait = useCreateWhenBrowserReady({
    ready: browserReady,
    downloading,
    startDownload: () => void handleDownload(),
    recheck: async () => (await loadDownloadedVersions("wayfern")).length,
    create: () => void handleCreate(),
  });

  const updateWayfernConfig = useCallback(
    (key: keyof WayfernConfig, value: unknown) => {
      setWayfernConfig((previous) => ({ ...previous, [key]: value }));
    },
    [],
  );

  const chooseScreen = useCallback((screen: ScreenSize | undefined) => {
    setWayfernConfig((previous) => {
      if (sameScreen(previous.screen, screen)) return previous;
      // A fingerprint made in advance has its own screen, so the new choice
      // makes the profile mint its identity again.
      return {
        ...previous,
        screen,
        identity_id: undefined,
        location: undefined,
        fingerprint: undefined,
        identity_overrides: undefined,
      };
    });
  }, []);

  const animateEntry = motionAllowed;
  const rise = (delay: number) =>
    animateEntry
      ? {
          initial: { y: 6 },
          animate: { y: 0 },
          transition: { ...MOTION_SPRING.page, delay },
        }
      : {};
  const countHint: CountHintLine[] = [];
  if (many) {
    countHint.push({
      text: t("templates.namesHint", {
        name: profileName.trim() || t("createProfile.profileName"),
      }),
    });
  }
  if (allowance !== null && allowance.remaining > 0) {
    countHint.push({
      text: t("templates.allowanceLeft", { count: allowance.remaining }),
      limit: maxCount < MAX_BULK_CREATE,
    });
  }
  if (maxCount === MAX_BULK_CREATE) {
    countHint.push({
      text: t("backendErrors.bulkCreateCountInvalid", {
        max: MAX_BULK_CREATE,
      }),
      limit: true,
      atLimitOnly: true,
    });
  }

  const reveal = animateEntry
    ? {
        initial: { y: -6, scale: 0.985 },
        animate: { y: 0, scale: 1 },
        transition: MOTION_SPRING.page,
      }
    : {};

  const createButton = (
    <LoadingButton
      data-slot="profile-create-submit"
      onClick={handleCreate}
      isLoading={isCreating || browserWait.waiting}
      disabled={isCreateDisabled}
      className="h-9 rounded-md px-4 transition-[opacity,box-shadow,transform] duration-200"
    >
      {t("common.buttons.create")}
    </LoadingButton>
  );

  return (
    <Dialog
      open={isOpen}
      onOpenChange={(open) => {
        if (!open && !isCreating) handleClose();
      }}
    >
      <DialogContent
        className="flex max-h-[calc(100dvh-max(1.5rem,12vh)-1.5rem)] max-w-lg flex-col gap-4 overflow-hidden rounded-2xl border-0 p-5 shadow-2xl ring-1 ring-foreground/10"
        // Anchored near the top rather than centered, so options that open
        // below push the footer down instead of moving the whole dialog.
        style={
          {
            top: "max(1.5rem, 12vh)",
            "--tw-translate-y": "0px",
          } as React.CSSProperties
        }
        aria-describedby={undefined}
        dismissible={!isCreating}
        hideClose
      >
        <DialogHeader className="shrink-0 flex-row items-center justify-between gap-2 space-y-0 pr-0">
          <DialogTitle className="text-base">
            {t("createProfile.title")}
          </DialogTitle>
          <div className="flex shrink-0 items-center gap-1.5">
            <TemplatePicker
              templates={templates}
              applied={appliedTemplate}
              onApply={applyTemplate}
              onSave={saveTemplate}
              onDelete={deleteTemplate}
              onDeleteNow={deleteTemplateNow}
              disabled={isCreating}
              animate={animateEntry}
            />
            <DialogClose
              data-slot="profile-create-close"
              aria-label={t("common.buttons.close")}
              className={cn(optionPill(false), "size-7 justify-center px-0")}
              disabled={isCreating}
            >
              <LuX />
            </DialogClose>
          </div>
        </DialogHeader>

        <div
          data-slot="profile-create-fields"
          className="-mx-1 flex min-h-0 flex-col gap-3 overflow-y-auto px-1 pt-1 pb-2"
        >
          <motion.div
            data-slot="profile-name-field"
            className="flex items-center gap-2 rounded-xl bg-foreground/5 p-2 transition-[background-color,box-shadow] duration-150 focus-within:bg-foreground/8 focus-within:ring-2 focus-within:ring-foreground/10 hover:bg-foreground/8"
            {...rise(0)}
          >
            <label
              htmlFor="profile-name"
              className="flex min-w-0 flex-1 cursor-text items-center ps-2"
            >
              <span className="sr-only">{t("createProfile.profileName")}</span>
              <Input
                ref={nameInputRef}
                onFocus={(event) => selectPrefilledName(event.currentTarget)}
                id="profile-name"
                variant="bare"
                autoFocus
                autoComplete="off"
                spellCheck={false}
                value={profileName}
                onChange={(event) => setProfileName(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                    event.preventDefault();
                    void handleCreate();
                  }
                }}
                placeholder={t("createProfile.profileNamePlaceholder")}
                disabled={isCreating}
                className="h-9 text-base font-medium md:text-base"
              />
            </label>
            <span id={osLabelId} className="sr-only">
              {t("fingerprint.osLabel")}
            </span>
            <Popover open={osPopoverOpen} onOpenChange={setOsPopoverOpen}>
              <PopoverTrigger asChild>
                <button
                  id="profile-os"
                  type="button"
                  role="combobox"
                  aria-expanded={osPopoverOpen}
                  aria-controls={osListboxId}
                  aria-labelledby={`${osLabelId} ${osValueId}`}
                  className={cn(optionPill(crossOs), "shrink-0")}
                  disabled={isCreating}
                >
                  <PillIcon on={crossOs} animate={animateEntry}>
                    <SelectedOsIcon />
                  </PillIcon>
                  <PillLabel
                    id={osValueId}
                    label={getOSDisplayName(selectedOs)}
                    animate={animateEntry}
                  />
                  <LuChevronDown
                    className={cn(
                      "-mr-0.5 opacity-60 transition-transform duration-200 motion-reduce:transition-none",
                      osPopoverOpen && "rotate-180",
                    )}
                    aria-hidden="true"
                  />
                </button>
              </PopoverTrigger>
              <PopoverContent className="w-52 p-0" align="end" sideOffset={6}>
                <Command>
                  <CommandList id={osListboxId}>
                    <CommandGroup>
                      {OS_OPTIONS.map((os) => {
                        const locked = os !== hostOs && !crossOsUnlocked;
                        const Icon = getOSIcon(os);
                        return (
                          <CommandItem
                            key={os}
                            value={os}
                            disabled={locked}
                            onSelect={() => {
                              setWayfernConfig((previous) => ({
                                ...previous,
                                os,
                                screen:
                                  previous.os === os
                                    ? previous.screen
                                    : undefined,
                              }));
                              setOsPopoverOpen(false);
                            }}
                          >
                            <LuCheck
                              className={cn(
                                "size-4 shrink-0",
                                selectedOs === os ? "opacity-100" : "opacity-0",
                              )}
                              aria-hidden="true"
                            />
                            <Icon
                              className="size-3.5 shrink-0"
                              aria-hidden="true"
                            />
                            <span className="truncate">
                              {getOSDisplayName(os)}
                            </span>
                            {locked && <ProBadge className="ml-auto" />}
                          </CommandItem>
                        );
                      })}
                    </CommandGroup>
                  </CommandList>
                </Command>
              </PopoverContent>
            </Popover>
            {creatableVersion && SCREEN_CHOICE_OS.includes(selectedOs) && (
              <ScreenPicker
                version={creatableVersion}
                os={selectedOs}
                screen={wayfernConfig.screen}
                onChange={chooseScreen}
                disabled={isCreating}
                animate={animateEntry}
              />
            )}
          </motion.div>

          <motion.div className="flex items-center gap-1.5" {...rise(0.04)}>
            <span id={proxyLabelId} className="sr-only">
              {t("createProfile.proxy.title")}
            </span>
            <Popover open={proxyPopoverOpen} onOpenChange={setProxyPopoverOpen}>
              <PopoverTrigger asChild>
                <button
                  id="profile-proxy"
                  type="button"
                  role="combobox"
                  aria-expanded={proxyPopoverOpen}
                  aria-controls={proxyListboxId}
                  aria-labelledby={`${proxyLabelId} ${proxyValueId}`}
                  className={cn(
                    optionPill(!!selectedProxyId),
                    "grow justify-center",
                  )}
                  disabled={isCreating}
                >
                  <PillIcon on={!!selectedProxyId} animate={animateEntry}>
                    {selectedCountryCode ? (
                      <FlagIcon
                        countryCode={selectedCountryCode}
                        className="h-3 shrink-0 rounded-[2px]"
                      />
                    ) : selectedVpn ? (
                      <LuShieldCheck />
                    ) : (
                      <LuGlobe />
                    )}
                  </PillIcon>
                  <PillLabel
                    id={proxyValueId}
                    label={proxyLabel}
                    animate={animateEntry}
                  />
                  <LuChevronDown
                    className={cn(
                      "-mr-0.5 opacity-60 transition-transform duration-200 motion-reduce:transition-none",
                      proxyPopoverOpen && "rotate-180",
                    )}
                    aria-hidden="true"
                  />
                </button>
              </PopoverTrigger>
              <PopoverContent className="w-84 p-0" align="start" sideOffset={6}>
                {/* Two Tabs: Прокси & VPN */}
                <div className="flex border-b p-1.5 gap-1 bg-muted/20">
                  <button
                    type="button"
                    className={cn(
                      "flex-1 py-1 px-2.5 text-xs font-medium rounded-md transition-all text-center",
                      proxyPopoverTab === "proxies"
                        ? "bg-background text-foreground shadow-xs font-semibold"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                    onClick={() => setProxyPopoverTab("proxies")}
                  >
                    {t("proxies.management.tabProxies", "Прокси")}
                  </button>
                  <button
                    type="button"
                    className={cn(
                      "flex-1 py-1 px-2.5 text-xs font-medium rounded-md transition-all text-center",
                      proxyPopoverTab === "vpn"
                        ? "bg-background text-foreground shadow-xs font-semibold"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                    onClick={() => setProxyPopoverTab("vpn")}
                  >
                    {t("proxies.management.tabVpns", "VPN")}
                  </button>
                </div>

                <Command>
                  <CommandInput placeholder={t("createProfile.proxy.search")} />
                  <CommandList id={proxyListboxId}>
                    <CommandEmpty>
                      {t("createProfile.proxy.notFound")}
                    </CommandEmpty>

                    {proxyPopoverTab === "proxies" ? (
                      <>
                        <CommandGroup>
                          <CommandItem
                            value="__none__"
                            keywords={[
                              t("common.labels.none"),
                              t("createProfile.proxy.noProxy"),
                            ]}
                            onSelect={() => {
                              setSelectedProxyId(undefined);
                              setProxyPopoverOpen(false);
                            }}
                          >
                            <LuCheck
                              className={cn(
                                "size-4 shrink-0",
                                !selectedProxyId ? "opacity-100" : "opacity-0",
                              )}
                              aria-hidden="true"
                            />
                            {t("common.labels.none")}
                          </CommandItem>
                        </CommandGroup>

                        {/* Proxy Chains */}
                        {chains.length > 0 && (
                          <CommandGroup
                            heading={t(
                              "proxyChains.title",
                              "Цепочки прокси (Chains)",
                            )}
                          >
                            {chains.map((chain) => (
                              <CommandItem
                                key={chain.id}
                                value={chain.id}
                                keywords={[
                                  chain.name,
                                  "цепь",
                                  "цепочка",
                                  "chain",
                                  "relay",
                                ]}
                                onSelect={() => {
                                  setSelectedProxyId(chain.id);
                                  setProxyPopoverOpen(false);
                                }}
                                className="flex items-center gap-2"
                              >
                                <LuCheck
                                  className={cn(
                                    "size-4 shrink-0",
                                    selectedProxyId === chain.id
                                      ? "opacity-100"
                                      : "opacity-0",
                                  )}
                                  aria-hidden="true"
                                />
                                <LuLink className="size-3.5 shrink-0 text-primary" />
                                <span className="truncate font-medium">
                                  {chain.name}
                                </span>
                                <Badge
                                  variant="secondary"
                                  className="ml-auto text-[10px] px-1.5 py-0 font-mono"
                                >
                                  Chain
                                </Badge>
                              </CommandItem>
                            ))}
                          </CommandGroup>
                        )}

                        {/* Standalone Proxies */}
                        <CommandGroup
                          heading={t("proxies.management.tabProxies", "Прокси")}
                        >
                          {sortedProxies.map((proxy) => {
                            const res = getCachedProxyResult(proxy.id);
                            const countryCode =
                              res?.country_code ??
                              extractCountryCode(proxy.geo_country, proxy.name);
                            const latency =
                              res?.is_valid &&
                              typeof res.latency_ms === "number"
                                ? res.latency_ms
                                : null;
                            return (
                              <CommandItem
                                key={proxy.id}
                                value={proxy.id}
                                keywords={[proxy.name, countryCode ?? ""]}
                                onSelect={() => {
                                  setSelectedProxyId(proxy.id);
                                  setProxyPopoverOpen(false);
                                }}
                                className="flex items-center gap-2"
                              >
                                <LuCheck
                                  className={cn(
                                    "size-4 shrink-0",
                                    selectedProxyId === proxy.id
                                      ? "opacity-100"
                                      : "opacity-0",
                                  )}
                                  aria-hidden="true"
                                />
                                {countryCode ? (
                                  <FlagIcon
                                    countryCode={countryCode}
                                    className="h-3 shrink-0 rounded-[2px]"
                                  />
                                ) : (
                                  <LuGlobe className="size-3.5 shrink-0 text-muted-foreground/60" />
                                )}
                                <span className="truncate">{proxy.name}</span>
                                {latency !== null && (
                                  <span className="ml-auto shrink-0 font-mono text-[11px] text-muted-foreground">
                                    {latency} ms
                                  </span>
                                )}
                              </CommandItem>
                            );
                          })}
                        </CommandGroup>
                        <CommandSeparator
                          alwaysRender
                          className="my-1 bg-foreground/8"
                        />
                        <CommandGroup forceMount>
                          <CommandItem
                            value="__add_proxy__"
                            forceMount
                            onSelect={() => {
                              setProxyPopoverOpen(false);
                              setShowProxyForm(true);
                            }}
                          >
                            <LuPlus
                              className="size-4 shrink-0"
                              aria-hidden="true"
                            />
                            {t("createProfile.proxy.addProxy")}
                          </CommandItem>
                        </CommandGroup>
                      </>
                    ) : (
                      <>
                        {/* WireGuard VPNs */}
                        {sortedVpns.length > 0 && (
                          <CommandGroup
                            heading={t(
                              "vpnSubscriptions.tabs.wireguard",
                              "WireGuard",
                            )}
                          >
                            {sortedVpns.map((vpn) => {
                              const res = getCachedVpnResult(vpn.id);
                              const countryCode =
                                res?.country_code ??
                                extractCountryCode(undefined, vpn.name);
                              const latency =
                                res?.is_valid &&
                                typeof res.latency_ms === "number"
                                  ? res.latency_ms
                                  : null;
                              return (
                                <CommandItem
                                  key={vpn.id}
                                  value={`vpn-${vpn.id}`}
                                  keywords={[
                                    vpn.name,
                                    "VPN",
                                    "WireGuard",
                                    countryCode ?? "",
                                  ]}
                                  onSelect={() => {
                                    setSelectedProxyId(`vpn-${vpn.id}`);
                                    setProxyPopoverOpen(false);
                                  }}
                                  className="flex items-center gap-2"
                                >
                                  <LuCheck
                                    className={cn(
                                      "size-4 shrink-0",
                                      selectedProxyId === `vpn-${vpn.id}`
                                        ? "opacity-100"
                                        : "opacity-0",
                                    )}
                                    aria-hidden="true"
                                  />
                                  {countryCode ? (
                                    <FlagIcon
                                      countryCode={countryCode}
                                      className="h-3 shrink-0 rounded-[2px]"
                                    />
                                  ) : (
                                    <Badge
                                      variant="soft"
                                      className="px-1 py-0 text-[11px] leading-tight"
                                    >
                                      WG
                                    </Badge>
                                  )}
                                  <span className="truncate">{vpn.name}</span>
                                  {latency !== null && (
                                    <span className="ml-auto shrink-0 font-mono text-[11px] text-muted-foreground">
                                      {latency} ms
                                    </span>
                                  )}
                                </CommandItem>
                              );
                            })}
                          </CommandGroup>
                        )}

                        {/* Subscriptions as collapsible accordion blocks */}
                        {subscriptions.map((sub) => {
                          const subNodes = subscriptionNodes[sub.id] || [];
                          const isExpanded = Boolean(expandedSubIds[sub.id]);

                          const uniqueCountries = Array.from(
                            new Set(
                              subNodes
                                .map(
                                  (n) =>
                                    n.country_code ||
                                    extractCountryCode(undefined, n.name),
                                )
                                .filter(Boolean) as string[],
                            ),
                          );

                          // Always statically sort by ping from lowest to highest
                          const sortedNodes = [...subNodes].sort((a, b) => {
                            const latA =
                              a.is_valid && typeof a.latency_ms === "number"
                                ? a.latency_ms
                                : null;
                            const latB =
                              b.is_valid && typeof b.latency_ms === "number"
                                ? b.latency_ms
                                : null;
                            if (latA !== null && latB !== null) {
                              return latA - latB;
                            }
                            if (latA !== null && latB === null) return -1;
                            if (latA === null && latB !== null) return 1;
                            return a.name.localeCompare(b.name);
                          });

                          return (
                            <div
                              key={sub.id}
                              className="my-1 border-b border-border/40 pb-1 last:border-b-0"
                            >
                              <div
                                role="button"
                                tabIndex={0}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter" || e.key === " ") {
                                    e.preventDefault();
                                    (e.currentTarget as HTMLElement).click();
                                  }
                                }}
                                className="flex items-center justify-between px-2.5 py-1.5 hover:bg-muted/40 cursor-pointer rounded-md mx-1 select-none text-xs font-medium border border-border/40 bg-muted/20"
                                onClick={(e) => {
                                  e.stopPropagation();
                                  setExpandedSubIds((prev) => ({
                                    ...prev,
                                    [sub.id]: !prev[sub.id],
                                  }));
                                }}
                              >
                                <div className="flex items-center gap-1.5 min-w-0 flex-1">
                                  {isExpanded ? (
                                    <LuChevronDown className="size-3.5 text-primary shrink-0" />
                                  ) : (
                                    <LuChevronRight className="size-3.5 text-muted-foreground shrink-0" />
                                  )}
                                  <span className="truncate font-medium">
                                    {sub.name}
                                  </span>
                                  <div className="flex items-center gap-0.5 ml-1 overflow-hidden shrink-0">
                                    {uniqueCountries.slice(0, 4).map((code) => (
                                      <FlagIcon
                                        key={code}
                                        countryCode={code}
                                        className="h-2.5 rounded-[1px]"
                                      />
                                    ))}
                                  </div>
                                </div>
                                <Badge
                                  variant="outline"
                                  className="text-[10px] px-1 py-0 text-muted-foreground shrink-0 ml-1"
                                >
                                  {subNodes.length}
                                </Badge>
                              </div>

                              {isExpanded && (
                                <div className="mt-1 space-y-0.5 pl-2 pr-1">
                                  {sortedNodes.map((node) => {
                                    const countryCode =
                                      node.country_code ??
                                      extractCountryCode(undefined, node.name);
                                    const isPinging = pingingNodeId === node.id;
                                    const isSelected =
                                      selectedProxyId === `subnode-${node.id}`;

                                    return (
                                      <CommandItem
                                        key={node.id}
                                        value={`subnode-${node.id}`}
                                        keywords={[
                                          node.name,
                                          node.protocol,
                                          sub.name,
                                          countryCode ?? "",
                                        ]}
                                        onSelect={() => {
                                          setSelectedProxyId(
                                            `subnode-${node.id}`,
                                          );
                                          setProxyPopoverOpen(false);
                                        }}
                                        className="flex items-center gap-2 pl-3 pr-2 py-1 text-xs"
                                      >
                                        <LuCheck
                                          className={cn(
                                            "size-3.5 shrink-0",
                                            isSelected
                                              ? "opacity-100 text-primary"
                                              : "opacity-0",
                                          )}
                                          aria-hidden="true"
                                        />
                                        {countryCode ? (
                                          <FlagIcon
                                            countryCode={countryCode}
                                            className="h-3 shrink-0 rounded-[2px]"
                                          />
                                        ) : (
                                          <LuGlobe className="size-3.5 shrink-0 text-muted-foreground/60" />
                                        )}
                                        <span className="truncate flex-1">
                                          {node.name}
                                        </span>
                                        <Badge
                                          variant="secondary"
                                          className="text-[9px] uppercase font-mono px-1 py-0 shrink-0"
                                        >
                                          {node.protocol}
                                        </Badge>
                                        {node.latency_ms !== null &&
                                        node.latency_ms !== undefined ? (
                                          <span
                                            className={cn(
                                              "shrink-0 font-mono text-[11px]",
                                              node.is_valid
                                                ? "text-emerald-500 font-medium"
                                                : "text-destructive",
                                            )}
                                          >
                                            {node.latency_ms} ms
                                          </span>
                                        ) : (
                                          <span className="shrink-0 font-mono text-[11px] text-muted-foreground/50">
                                            —
                                          </span>
                                        )}
                                        <button
                                          type="button"
                                          title={t(
                                            "vpnSubscriptions.ping",
                                            "Проверить пинг",
                                          )}
                                          className="p-1 rounded hover:bg-muted text-muted-foreground hover:text-foreground shrink-0 cursor-pointer transition-colors"
                                          onClick={async (e) => {
                                            e.stopPropagation();
                                            e.preventDefault();
                                            try {
                                              setPingingNodeId(node.id);
                                              await pingSingleNode(
                                                node.id,
                                                node.subscription_id,
                                              );
                                            } catch (err) {
                                              console.error("Ping error:", err);
                                            } finally {
                                              setPingingNodeId(null);
                                            }
                                          }}
                                        >
                                          <LuActivity
                                            className={cn(
                                              "size-3.5",
                                              isPinging &&
                                                "animate-spin text-primary",
                                            )}
                                          />
                                        </button>
                                      </CommandItem>
                                    );
                                  })}
                                </div>
                              )}
                            </div>
                          );
                        })}

                        {sortedVpns.length === 0 &&
                          allSubNodes.length === 0 && (
                            <div className="py-6 text-center text-xs text-muted-foreground">
                              {t(
                                "vpnSubscriptions.noNodesInSub",
                                "Нет доступных VPN или серверов подписок",
                              )}
                            </div>
                          )}
                      </>
                    )}
                  </CommandList>
                </Command>
              </PopoverContent>
            </Popover>

            <OptionToggle
              id="enable-password"
              checked={enablePassword}
              onCheckedChange={(checked) => {
                if (checked) {
                  setEnablePassword(true);
                  setEphemeral(false);
                } else clearPassword();
              }}
              disabled={isCreating || many}
              animate={animateEntry}
              icon={enablePassword ? <LuLock /> : <LuLockOpen />}
              label={t("profilePassword.fields.password")}
              hint={
                many
                  ? t("templates.passwordOneOnly")
                  : t("createProfile.passwordProtect.description")
              }
            />
            <OptionToggle
              id="ephemeral"
              checked={ephemeral}
              onCheckedChange={(checked) => {
                setEphemeral(checked);
                if (checked) clearPassword();
              }}
              disabled={isCreating}
              animate={animateEntry}
              icon={<LuFlame className={cn(ephemeral && "fill-current")} />}
              label={t("profiles.ephemeral")}
              hint={t("profiles.ephemeralDescription")}
            />
            <Popover open={morePopoverOpen} onOpenChange={setMorePopoverOpen}>
              <Tooltip delayDuration={600}>
                <TooltipTrigger asChild>
                  <PopoverTrigger asChild>
                    <button
                      id="profile-more-options"
                      type="button"
                      data-slot="profile-create-more"
                      aria-label={t("createProfile.moreOptions")}
                      aria-haspopup="dialog"
                      aria-expanded={morePopoverOpen}
                      aria-describedby={
                        destinationGroup ? groupValueId : undefined
                      }
                      className={cn(
                        optionPill(!!destinationGroup),
                        "w-8 shrink-0 justify-center px-0",
                      )}
                      disabled={isCreating}
                    >
                      <PillIcon on={!!destinationGroup} animate={animateEntry}>
                        <LuEllipsis />
                      </PillIcon>
                      {destinationGroup && (
                        <span id={groupValueId} className="sr-only">
                          {destinationGroup.name}
                        </span>
                      )}
                    </button>
                  </PopoverTrigger>
                </TooltipTrigger>
                <TooltipContent side="top" sideOffset={6} className="max-w-72">
                  <p>{t("createProfile.moreOptions")}</p>
                  {destinationGroup && (
                    <p className="flex items-center gap-1 opacity-70">
                      <LuFolder
                        className="size-3 shrink-0"
                        aria-hidden="true"
                      />
                      <span className="truncate">{destinationGroup.name}</span>
                    </p>
                  )}
                </TooltipContent>
              </Tooltip>
              <PopoverContent className="w-64 p-0" align="start" sideOffset={6}>
                <Command>
                  {groups.length > 6 && (
                    <CommandInput
                      placeholder={t("commandPalette.pages.group.placeholder")}
                    />
                  )}
                  <CommandList id={groupListboxId}>
                    <CommandEmpty>
                      {t("createProfile.group.notFound")}
                    </CommandEmpty>
                    <CommandGroup heading={t("profiles.table.group")}>
                      <CommandItem
                        value="__no_group__"
                        keywords={[t("groups.noGroup")]}
                        onSelect={() => {
                          setDestinationGroupId(null);
                          setMorePopoverOpen(false);
                        }}
                      >
                        <LuCheck
                          className={cn(
                            "size-4 shrink-0",
                            !destinationGroup ? "opacity-100" : "opacity-0",
                          )}
                          aria-hidden="true"
                        />
                        {t("groups.noGroup")}
                      </CommandItem>
                      {groups.map((group) => (
                        <CommandItem
                          key={group.id}
                          value={group.id}
                          keywords={[group.name]}
                          onSelect={() => {
                            setDestinationGroupId(group.id);
                            setMorePopoverOpen(false);
                          }}
                        >
                          <LuCheck
                            className={cn(
                              "size-4 shrink-0",
                              destinationGroup?.id === group.id
                                ? "opacity-100"
                                : "opacity-0",
                            )}
                            aria-hidden="true"
                          />
                          <LuFolder
                            className="size-3.5 shrink-0 text-muted-foreground"
                            aria-hidden="true"
                          />
                          <span className="truncate">{group.name}</span>
                        </CommandItem>
                      ))}
                    </CommandGroup>
                  </CommandList>
                </Command>
              </PopoverContent>
            </Popover>
          </motion.div>

          {(ephemeral || enablePassword) && <EphemeralStorageNotice />}

          {enablePassword && (
            <motion.div className="space-y-1.5" {...reveal}>
              <div className="grid gap-2 sm:grid-cols-2">
                <div>
                  <Label htmlFor="profile-password" className="sr-only">
                    {t("profilePassword.fields.newPassword")}
                  </Label>
                  <Input
                    id="profile-password"
                    variant="soft"
                    type="password"
                    value={password}
                    onChange={(event) => {
                      setPassword(event.target.value);
                      setPasswordError(null);
                    }}
                    placeholder={t("profilePassword.fields.newPassword")}
                    autoComplete="new-password"
                    aria-invalid={!!passwordError}
                    aria-describedby={
                      passwordError ? "profile-password-error" : undefined
                    }
                    disabled={isCreating}
                  />
                </div>
                <div>
                  <Label htmlFor="profile-password-confirm" className="sr-only">
                    {t("profilePassword.fields.confirm")}
                  </Label>
                  <Input
                    id="profile-password-confirm"
                    variant="soft"
                    type="password"
                    value={passwordConfirm}
                    onChange={(event) => {
                      setPasswordConfirm(event.target.value);
                      setPasswordError(null);
                    }}
                    placeholder={t("profilePassword.fields.confirm")}
                    autoComplete="new-password"
                    aria-invalid={!!passwordError}
                    aria-describedby={
                      passwordError ? "profile-password-error" : undefined
                    }
                    disabled={isCreating}
                  />
                </div>
              </div>
              {passwordError && (
                <motion.p
                  key={passwordError}
                  id="profile-password-error"
                  role="alert"
                  className="px-1 text-xs text-destructive-text"
                  initial={animateEntry ? { x: 0 } : false}
                  animate={animateEntry ? { x: [0, -4, 4, -2, 0] } : { x: 0 }}
                  transition={{
                    duration: MOTION_DURATION.slow,
                    ease: MOTION_EASE_OUT,
                  }}
                >
                  {passwordError}
                </motion.p>
              )}
            </motion.div>
          )}

          {crossOs && (
            <motion.p
              role="status"
              className={cn(STATUS_CLASS, "items-start")}
              {...reveal}
            >
              <LuTriangleAlert
                className="mt-0.5 size-3.5 shrink-0"
                aria-hidden="true"
              />
              {t("fingerprint.crossOsWarning")}
            </motion.p>
          )}

          {downloading || browserWait.waiting ? (
            <motion.p role="status" className={STATUS_CLASS} {...reveal}>
              <LuLoaderCircle
                className="size-3.5 shrink-0 animate-spin"
                aria-hidden="true"
              />
              {browserWait.waiting
                ? t("createProfile.version.createWhenReady", {
                    browser: "Wayfern",
                  })
                : t("createProfile.version.downloading", {
                    browser: "Wayfern",
                    version: latestVersion,
                  })}
            </motion.p>
          ) : (
            !creatableVersion &&
            (isLoadingReleaseTypes ? (
              <motion.p role="status" className={STATUS_CLASS} {...reveal}>
                <LuLoaderCircle
                  className="size-3.5 shrink-0 animate-spin"
                  aria-hidden="true"
                />
                {t("createProfile.version.fetching")}
              </motion.p>
            ) : releaseTypesError ? (
              <motion.div
                role="alert"
                className={cn(STATUS_CLASS, "bg-destructive/10 py-1.5 pr-1.5")}
                {...reveal}
              >
                <p className="flex-1 text-destructive-text">
                  {t("createProfile.version.fetchError")}
                </p>
                <Button
                  onClick={() => void loadReleaseTypes()}
                  size="sm"
                  variant="soft"
                  className="h-7 rounded-md text-xs"
                >
                  {t("common.buttons.retry")}
                </Button>
              </motion.div>
            ) : latestVersion ? (
              <motion.div
                className={cn(STATUS_CLASS, "py-1.5 pr-1.5")}
                {...reveal}
              >
                <p className="flex-1">
                  {t("createProfile.version.needsDownload", {
                    browser: "Wayfern",
                    version: latestVersion,
                  })}
                </p>
                <Button
                  onClick={() => void handleDownload()}
                  size="sm"
                  className="h-7 rounded-md text-xs"
                >
                  {t("common.buttons.download")}
                </Button>
              </motion.div>
            ) : (
              <p
                role="status"
                className={cn(STATUS_CLASS, "text-warning-text")}
              >
                {t("createProfile.platformUnavailable", {
                  browser: "Wayfern",
                })}
              </p>
            ))
          )}

          {hourlyCapReached && (
            <motion.p
              role="status"
              data-slot="hourly-limit"
              className={cn(STATUS_CLASS, "items-start")}
              {...reveal}
            >
              <LuClock
                className="mt-0.5 size-3.5 shrink-0"
                aria-hidden="true"
              />
              {t("createProfile.hourlyLimit.reached", {
                limit: allowance.per_hour,
                minutes: Math.max(
                  1,
                  Math.ceil((allowance.retry_after_secs ?? 60) / 60),
                ),
              })}
            </motion.p>
          )}

          {creatingQuickly && (
            <motion.div
              role="status"
              className={cn(
                STATUS_CLASS,
                "items-start bg-warning/10 text-warning-text",
              )}
              {...reveal}
            >
              <LuTriangleAlert
                className="mt-0.5 size-3.5 shrink-0"
                aria-hidden="true"
              />
              <div className="space-y-0.5">
                <p className="font-medium">
                  {t("createProfile.hourlyLimit.warningTitle")}
                </p>
                <p>{t("createProfile.hourlyLimit.warning")}</p>
              </div>
            </motion.div>
          )}

          <div id={advancedOptionsId} hidden={!advancedOpen}>
            <AnimatedDisclosureContent
              open={advancedOpen}
              className="space-y-4 pt-2"
            >
              <SoftFields>
                <div className="space-y-4">
                  <div className="space-y-1.5">
                    <Label
                      htmlFor="launch-hook-url"
                      className={FIELD_LABEL_CLASS}
                    >
                      {t("createProfile.launchHook.label")}
                    </Label>
                    <Input
                      id="launch-hook-url"
                      variant="soft"
                      value={launchHook}
                      onChange={(event) => setLaunchHook(event.target.value)}
                      placeholder={t("createProfile.launchHook.placeholder")}
                      disabled={isCreating}
                    />
                  </div>
                  <div
                    className={cn(
                      "grid gap-4",
                      extensionGroups.length > 0 && "sm:grid-cols-2",
                    )}
                  >
                    <div className="min-w-0 space-y-1.5">
                      <Label
                        htmlFor="profile-dns-blocklist"
                        className={FIELD_LABEL_CLASS}
                      >
                        {t("dnsBlocklist.title")}
                      </Label>
                      <Select
                        value={dnsBlocklist || "none"}
                        onValueChange={(value) =>
                          setDnsBlocklist(value === "none" ? "" : value)
                        }
                        disabled={isCreating}
                      >
                        <SelectTrigger
                          id="profile-dns-blocklist"
                          variant="soft"
                          className="w-full"
                        >
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="none">
                            {t("dnsBlocklist.none")}
                          </SelectItem>
                          {DNS_BLOCKLIST_LEVELS.map((level) => (
                            <SelectItem key={level.value} value={level.value}>
                              {t(level.labelKey)}
                            </SelectItem>
                          ))}
                        </SelectContent>
                      </Select>
                    </div>
                    {extensionGroups.length > 0 && (
                      <div className="min-w-0 space-y-1.5">
                        <Label
                          htmlFor="profile-extension-group"
                          className={FIELD_LABEL_CLASS}
                        >
                          {t("extensions.extensionGroup")}
                        </Label>
                        <Select
                          value={selectedExtensionGroupId ?? "none"}
                          onValueChange={(value) =>
                            setSelectedExtensionGroupId(
                              value === "none" ? undefined : value,
                            )
                          }
                          disabled={isCreating}
                        >
                          <SelectTrigger
                            id="profile-extension-group"
                            variant="soft"
                            className="w-full"
                          >
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem value="none">
                              {t("profileInfo.values.none")}
                            </SelectItem>
                            {extensionGroups.map((group) => (
                              <SelectItem key={group.id} value={group.id}>
                                {group.name} ({group.extension_ids.length})
                              </SelectItem>
                            ))}
                          </SelectContent>
                        </Select>
                      </div>
                    )}
                  </div>
                  <div className="space-y-1.5">
                    <Label
                      htmlFor="profile-dns-server"
                      className={FIELD_LABEL_CLASS}
                    >
                      {t("dnsServer.label")}
                    </Label>
                    <DnsServerField
                      id="profile-dns-server"
                      value={dnsServer}
                      onChange={setDnsServer}
                      onValidityChange={setDnsServerValid}
                      disabled={isCreating}
                    />
                  </div>
                  <div className="space-y-1.5">
                    <Label
                      htmlFor="profile-start-pages"
                      className={FIELD_LABEL_CLASS}
                    >
                      {t("profileInfo.sections.startPages")}
                    </Label>
                    <Textarea
                      id="profile-start-pages"
                      variant="soft"
                      value={startPages}
                      onChange={(event) => setStartPages(event.target.value)}
                      placeholder={t("startPages.placeholder")}
                      rows={3}
                      spellCheck={false}
                      disabled={isCreating}
                      className="font-mono text-xs"
                    />
                    <p className="text-xs text-muted-foreground">
                      {t("startPages.description")}
                    </p>
                  </div>
                  <div className="space-y-4 pt-1">
                    {creatableVersion && (
                      <p className="text-xs text-muted-foreground">
                        {t("createProfile.version.available", {
                          browser: "Wayfern",
                          version: creatableVersion,
                        })}
                      </p>
                    )}
                    {creatableVersion &&
                      latestVersion &&
                      !downloadedVersions.includes(latestVersion) &&
                      !downloading && (
                        <div className={cn(STATUS_CLASS, "py-1.5 pr-1.5")}>
                          <p className="flex-1">
                            {t("createProfile.version.upgradeAvailable", {
                              browser: "Wayfern",
                              version: latestVersion,
                            })}
                          </p>
                          <Button
                            onClick={() => void handleDownload()}
                            size="sm"
                            variant="soft"
                            className="h-7 rounded-md text-xs"
                            disabled={isCreating}
                          >
                            {t("common.buttons.download")}
                          </Button>
                        </div>
                      )}
                    <WayfernConfigForm
                      className="[&_[data-slot=select-trigger]]:w-full"
                      config={wayfernConfig}
                      onConfigChange={updateWayfernConfig}
                      isCreating
                      forceAdvanced
                      readOnly={isCreating}
                      crossOsUnlocked={crossOsUnlocked}
                      limitedMode={!crossOsUnlocked}
                      osPicker={false}
                      profileVersion={creatableVersion}
                      profileBrowser="wayfern"
                    />
                  </div>
                </div>
              </SoftFields>
            </AnimatedDisclosureContent>
          </div>
        </div>

        <DialogFooter className="items-center">
          {/* Every control here keeps its size whatever the count, so the
              row never reflows; the batch progress takes the place of
              Advanced options without moving the buttons. */}
          <div className="relative flex min-w-0 grow items-center">
            <Button
              variant="subtle"
              size="sm"
              className={cn(
                "-ml-2 gap-1 rounded-lg px-2",
                progress && "invisible",
              )}
              aria-expanded={advancedOpen}
              aria-controls={advancedOptionsId}
              onClick={() => setAdvancedOpen((open) => !open)}
              disabled={isCreating}
            >
              <AnimatedDisclosureChevron open={advancedOpen}>
                <LuChevronRight className="size-4" />
              </AnimatedDisclosureChevron>
              {t("createProfile.advancedOptions")}
            </Button>
            <p
              role="status"
              data-slot="profile-create-progress"
              className="pointer-events-none absolute inset-0 flex min-w-0 items-center gap-2 text-xs text-muted-foreground tabular-nums"
            >
              {progress && (
                <>
                  <span
                    aria-hidden="true"
                    className="h-1 w-12 shrink-0 overflow-hidden rounded-full bg-foreground/12"
                  >
                    <motion.span
                      className="block h-full rounded-full bg-foreground"
                      style={{ originX: 0 }}
                      initial={false}
                      animate={{
                        scaleX: progress.total
                          ? progress.done / progress.total
                          : 0,
                      }}
                      transition={{
                        duration: animateEntry ? MOTION_DURATION.slow : 0,
                        ease: MOTION_EASE_OUT,
                      }}
                    />
                  </span>
                  <span className="truncate">
                    {t("templates.progress", progress)}
                  </span>
                </>
              )}
            </p>
          </div>
          <div className="ml-auto flex shrink-0 items-center gap-2">
            <Button
              variant="subtle"
              data-slot={progress ? "profile-create-stop" : undefined}
              onClick={
                progress
                  ? () => void invoke("cancel_bulk_create_profiles")
                  : handleClose
              }
              disabled={isCreating && !progress}
            >
              <SwapLabel
                labels={[t("common.buttons.cancel"), t("templates.stop")]}
                shown={progress ? 1 : 0}
              />
            </Button>
            <CountStepper
              value={count}
              max={maxCount}
              onChange={changeCount}
              disabled={isCreating}
              hint={countHint}
            />
            {closeABrowser ? (
              <Tooltip delayDuration={300}>
                <TooltipTrigger asChild>
                  <span
                    data-slot="profile-create-close-a-browser"
                    // A disabled button takes no pointer events, so the
                    // reason lives on its wrapper.
                    className="inline-flex rounded-md"
                  >
                    {createButton}
                  </span>
                </TooltipTrigger>
                <TooltipContent side="top" sideOffset={6} className="max-w-72">
                  {t("backendErrors.tooManyBrowsersRunning")}
                </TooltipContent>
              </Tooltip>
            ) : (
              createButton
            )}
          </div>
        </DialogFooter>
      </DialogContent>
      <ProxyFormDialog
        isOpen={showProxyForm}
        onClose={() => setShowProxyForm(false)}
      />
    </Dialog>
  );
}

function sameScreen(a: ScreenSize | undefined, b: ScreenSize | undefined) {
  return a?.width === b?.width && a?.height === b?.height;
}

/**
 * The screen a new identity claims, from the sizes the browser lists for the
 * claim, most common first. Not shown when the browser has no such list.
 */
function ScreenPicker({
  version,
  os,
  screen,
  onChange,
  disabled,
  animate,
}: {
  version: string;
  os: WayfernOS;
  screen: ScreenSize | undefined;
  onChange: (screen: ScreenSize | undefined) => void;
  disabled: boolean;
  animate: boolean;
}) {
  const { t } = useTranslation();
  const labelId = useId();
  const valueId = useId();
  const listboxId = useId();
  const [open, setOpen] = useState(false);
  const key = `${version}|${os}`;
  // `null`: the browser lists no sizes, or could not list them.
  const [loaded, setLoaded] = useState<{
    key: string;
    sizes: ScreenSize[] | null;
  } | null>(null);

  useEffect(() => {
    let current = true;
    invoke<ScreenSize[] | null>("get_wayfern_screen_options", { version, os })
      .then((sizes) => {
        if (current) setLoaded({ key: `${version}|${os}`, sizes });
      })
      .catch((error: unknown) => {
        console.error("Failed to read the screen sizes:", error);
        if (current) setLoaded({ key: `${version}|${os}`, sizes: null });
      });
    return () => {
      current = false;
    };
  }, [version, os]);

  // The browser that read the sizes waits for this dialog's creation; once
  // the picker is gone it has nothing left to do.
  useEffect(
    () => () => {
      void invoke("release_wayfern_screen_helper").catch((error: unknown) => {
        console.error("Failed to release the screen size browser:", error);
      });
    },
    [],
  );

  const sizes = loaded?.key === key ? loaded.sizes : undefined;
  const loading = sizes === undefined;
  const offered = !screen || !!sizes?.some((size) => sameScreen(size, screen));

  // A size this browser does not list is no choice at all.
  useEffect(() => {
    if (!loading && !offered) onChange(undefined);
  }, [loading, offered, onChange]);

  if (sizes === null) return null;

  const sizeLabel = (size: ScreenSize) =>
    t("createProfile.screen.size", {
      width: size.width,
      height: size.height,
    });

  return (
    <>
      <span id={labelId} className="sr-only">
        {t("createProfile.screen.label")}
      </span>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            id="profile-screen"
            type="button"
            role="combobox"
            aria-expanded={open}
            aria-controls={listboxId}
            aria-labelledby={`${labelId} ${valueId}`}
            aria-busy={loading}
            className={cn(optionPill(!!screen), "shrink-0")}
            disabled={disabled || loading}
          >
            <PillIcon on={!!screen} animate={animate}>
              {loading ? (
                <LuLoaderCircle className="animate-spin motion-reduce:animate-none" />
              ) : (
                <LuMonitor />
              )}
            </PillIcon>
            <PillLabel
              id={valueId}
              label={
                screen ? sizeLabel(screen) : t("createProfile.screen.anyScreen")
              }
              animate={animate}
            />
            <LuChevronDown
              className={cn(
                "-mr-0.5 opacity-60 transition-transform duration-200 motion-reduce:transition-none",
                open && "rotate-180",
              )}
              aria-hidden="true"
            />
          </button>
        </PopoverTrigger>
        <PopoverContent className="w-52 p-0" align="end" sideOffset={6}>
          <Command>
            {(sizes?.length ?? 0) > SCREEN_SEARCH_FROM && (
              <CommandInput placeholder={t("createProfile.screen.search")} />
            )}
            <CommandList id={listboxId}>
              <CommandEmpty>{t("createProfile.screen.notFound")}</CommandEmpty>
              <CommandGroup heading={t("createProfile.screen.label")}>
                {[undefined, ...(sizes ?? [])].map((size) => (
                  <CommandItem
                    key={size ? `${size.width}x${size.height}` : "any"}
                    value={size ? `${size.width}x${size.height}` : "__any__"}
                    keywords={[
                      size ? sizeLabel(size) : t("createProfile.screen.any"),
                    ]}
                    onSelect={() => {
                      onChange(size);
                      setOpen(false);
                    }}
                  >
                    <LuCheck
                      className={cn(
                        "size-4 shrink-0",
                        sameScreen(size, screen) ? "opacity-100" : "opacity-0",
                      )}
                      aria-hidden="true"
                    />
                    <span className="truncate tabular-nums">
                      {size ? sizeLabel(size) : t("createProfile.screen.any")}
                    </span>
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
    </>
  );
}

/** The group a list filter names, or none for "All". */
function groupFromFilter(filter: string | undefined): string | null {
  return filter && filter !== "__all__" ? filter : null;
}

/** Mirror of `profile_generation_limiter::Allowance`. */
interface ProfileCreationAllowance {
  enforcement: "hard" | "soft";
  per_hour: number;
  used: number;
  retry_after_secs: number | null;
  remaining: number;
  close_a_browser: boolean;
}

const STATUS_CLASS =
  "flex items-center gap-2 rounded-lg bg-foreground/5 px-3 py-2 text-xs text-muted-foreground";

const FIELD_LABEL_CLASS = "text-xs font-medium text-muted-foreground";

function optionPill(on: boolean) {
  return cn(
    "inline-flex h-8 max-w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-full px-3 text-sm font-medium select-none transition-[background-color,color,box-shadow,transform] duration-150 ease-[var(--ease-out)] active:scale-[0.97] disabled:pointer-events-none disabled:opacity-50 motion-reduce:active:scale-100 [&_svg]:size-3.5 [&_svg]:shrink-0",
    FOCUS_RING,
    on
      ? "bg-primary/10 text-foreground [&_svg]:text-primary-text"
      : "bg-foreground/5 text-muted-foreground hover:bg-foreground/8 hover:text-foreground",
  );
}

/**
 * False on the first render, true after it. A pill's parts spring when their
 * option changes, never as the dialog opens: the first frame shows every
 * option at rest, so a stalled frame cannot leave one half drawn.
 */
function useMounted() {
  const mounted = useRef(false);
  useEffect(() => {
    mounted.current = true;
  }, []);
  return mounted.current;
}

/** A pill's icon springs in whenever the option it marks changes state. */
function PillIcon({
  on,
  animate,
  children,
}: {
  on: boolean;
  animate: boolean;
  children: React.ReactNode;
}) {
  const mounted = useMounted();
  return (
    <motion.span
      key={String(on)}
      aria-hidden="true"
      className="inline-flex"
      initial={
        animate && mounted ? { scale: 0.4, rotate: on ? -24 : 24 } : false
      }
      animate={{ scale: 1, rotate: 0 }}
      transition={MOTION_SPRING.snappy}
    >
      {children}
    </motion.span>
  );
}

/** The chosen route's name, which rises into place when it changes. */
function PillLabel({
  id,
  label,
  animate,
}: {
  id: string;
  label: string;
  animate: boolean;
}) {
  const mounted = useMounted();
  return (
    <motion.span
      key={label}
      id={id}
      className="min-w-0 truncate"
      initial={animate && mounted ? { y: 4 } : false}
      animate={{ y: 0 }}
      transition={MOTION_SPRING.page}
    >
      {label}
    </motion.span>
  );
}

/**
 * An on/off option drawn as a pill. It is a real checkbox underneath, so it
 * keeps the checkbox role, state and keyboard behavior; the hint explains the
 * option without adding text to the form.
 */
function OptionToggle({
  id,
  checked,
  onCheckedChange,
  disabled,
  animate,
  icon,
  label,
  hint,
}: {
  id: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  disabled: boolean;
  animate: boolean;
  icon: React.ReactNode;
  label: string;
  hint: string;
}) {
  return (
    <Tooltip delayDuration={600}>
      <TooltipTrigger asChild>
        <CheckboxPrimitive.Root
          id={id}
          checked={checked}
          onCheckedChange={(state) => onCheckedChange(state === true)}
          disabled={disabled}
          className={cn(optionPill(checked), "grow shrink-0 justify-center")}
        >
          <PillIcon on={checked} animate={animate}>
            {icon}
          </PillIcon>
          <span className="truncate">{label}</span>
        </CheckboxPrimitive.Root>
      </TooltipTrigger>
      <TooltipContent side="top" sideOffset={6} className="max-w-72">
        {hint}
      </TooltipContent>
    </Tooltip>
  );
}

/** Picks a template for the form, saves the form as one, or deletes one. */
function TemplatePicker({
  templates,
  applied,
  onApply,
  onSave,
  onDelete,
  onDeleteNow,
  disabled,
  animate,
}: {
  templates: ProfileTemplate[];
  applied: ProfileTemplate | null;
  onApply: (template: ProfileTemplate | null) => void;
  onSave: (name: string) => Promise<boolean>;
  /** Moves it to the trash; calls `confirm` instead when the trash is off. */
  onDelete: (template: ProfileTemplate, confirm: () => void) => Promise<void>;
  /** Deletes it after the inline confirmation. */
  onDeleteNow: (template: ProfileTemplate) => Promise<void>;
  disabled: boolean;
  animate: boolean;
}) {
  const { t } = useTranslation();
  const valueId = useId();
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [saving, setSaving] = useState(false);
  const [confirmingDelete, setConfirmingDelete] = useState<string | null>(null);
  const trimmed = name.trim();
  const replaces = templates.some(
    (template) => template.name.toLowerCase() === trimmed.toLowerCase(),
  );

  useEffect(() => {
    if (!open) return;
    setName(applied?.name ?? "");
    setConfirmingDelete(null);
  }, [open, applied]);

  const save = async () => {
    if (!trimmed || saving) return;
    setSaving(true);
    const saved = await onSave(trimmed);
    setSaving(false);
    if (saved) setOpen(false);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          data-slot="template-picker"
          aria-expanded={open}
          aria-haspopup="dialog"
          aria-describedby={valueId}
          className={cn(optionPill(applied !== null), "h-7 shrink-0 text-xs")}
          disabled={disabled}
        >
          <PillIcon on={applied !== null} animate={animate}>
            <LuLayoutTemplate />
          </PillIcon>
          <PillLabel
            id={valueId}
            label={applied?.name ?? t("templates.label")}
            animate={animate}
          />
          <LuChevronDown
            className={cn(
              "-mr-0.5 opacity-60 transition-transform duration-200 motion-reduce:transition-none",
              open && "rotate-180",
            )}
            aria-hidden="true"
          />
        </button>
      </PopoverTrigger>
      <PopoverContent className="w-64 p-0" align="end" sideOffset={6}>
        {templates.length > 0 ? (
          <Command>
            <CommandList>
              <CommandGroup>
                {applied && (
                  <CommandItem
                    value="__no_template__"
                    onSelect={() => {
                      onApply(null);
                      setOpen(false);
                    }}
                  >
                    <span className="size-4 shrink-0" aria-hidden="true" />
                    <span className="truncate text-muted-foreground">
                      {t("templates.none")}
                    </span>
                  </CommandItem>
                )}
                {templates.map((template) => (
                  <CommandItem
                    key={template.id}
                    value={template.id}
                    data-slot="template-option"
                    className="group/template"
                    onSelect={() => {
                      onApply(template);
                      setOpen(false);
                    }}
                  >
                    <LuCheck
                      className={cn(
                        "size-4 shrink-0",
                        applied?.id === template.id
                          ? "opacity-100"
                          : "opacity-0",
                      )}
                      aria-hidden="true"
                    />
                    <span className="min-w-0 flex-1 truncate">
                      {template.name}
                    </span>
                    {confirmingDelete === template.id ? (
                      <button
                        type="button"
                        data-slot="template-delete-confirm"
                        className="rounded-md px-1.5 py-0.5 text-xs font-medium text-destructive hover:bg-destructive/10"
                        onClick={(event) => {
                          event.stopPropagation();
                          setConfirmingDelete(null);
                          void onDeleteNow(template);
                        }}
                      >
                        {t("common.buttons.delete")}
                      </button>
                    ) : (
                      <button
                        type="button"
                        data-slot="template-delete"
                        aria-label={t("templates.delete", {
                          name: template.name,
                        })}
                        className={cn(
                          "grid size-6 place-items-center rounded-md text-muted-foreground opacity-0 transition-opacity duration-100 group-hover/template:opacity-100 group-data-[selected=true]/template:opacity-100 hover:bg-foreground/8 hover:text-destructive focus-visible:opacity-100 [&_svg]:size-3.5",
                          FOCUS_RING,
                        )}
                        onClick={(event) => {
                          event.stopPropagation();
                          void onDelete(template, () =>
                            setConfirmingDelete(template.id),
                          );
                        }}
                      >
                        <LuTrash2 aria-hidden="true" />
                      </button>
                    )}
                  </CommandItem>
                ))}
              </CommandGroup>
            </CommandList>
          </Command>
        ) : (
          <p className="px-3 pt-3 text-xs text-muted-foreground">
            {t("templates.empty")}
          </p>
        )}
        <div className="space-y-1.5 p-2">
          <p className={cn(FIELD_LABEL_CLASS, "px-1")}>
            {t("templates.saveCurrent")}
          </p>
          <div className="flex items-center gap-1.5">
            <Input
              variant="soft"
              data-slot="template-name"
              value={name}
              maxLength={60}
              onChange={(event) => setName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.nativeEvent.isComposing) {
                  event.preventDefault();
                  void save();
                }
              }}
              placeholder={t("templates.namePlaceholder")}
              aria-label={t("templates.namePlaceholder")}
              className="h-8 text-sm"
            />
            <Button
              size="sm"
              data-slot="template-save"
              className="h-8 shrink-0"
              disabled={!trimmed || saving}
              onClick={() => void save()}
            >
              {replaces ? t("templates.replace") : t("templates.save")}
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
}

/** One line of the count stepper's tooltip. */
interface CountHintLine {
  text: string;
  /** Says what bounds the count; stressed once the count is at the bound. */
  limit?: boolean;
  /** Shown only at the bound. */
  atLimitOnly?: boolean;
}

/** Shows one label in the width of the widest, so swapping them moves nothing. */
function SwapLabel({ labels, shown }: { labels: string[]; shown: number }) {
  return (
    <span className="inline-grid">
      {labels.map((label, index) => (
        <span
          key={label}
          aria-hidden={index !== shown}
          className={cn(
            "col-start-1 row-start-1",
            index !== shown && "invisible",
          )}
        >
          {label}
        </span>
      ))}
    </span>
  );
}

/**
 * How many profiles to create, from 1 to `max`. Its width never changes, and
 * what the count means (the names, the allowance, the bound) shows in a
 * tooltip while the pointer or focus is on it, so nothing around it moves.
 */
function CountStepper({
  value,
  max,
  onChange,
  disabled,
  hint,
}: {
  value: number;
  max: number;
  onChange: (value: number) => void;
  disabled: boolean;
  hint: CountHintLine[];
}) {
  const { t } = useTranslation();
  const hintId = useId();
  const inputRef = useRef<HTMLInputElement>(null);
  const [draft, setDraft] = useState(String(value));
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const [dismissed, setDismissed] = useState(false);
  useEffect(() => {
    setDraft(String(value));
  }, [value]);

  const atLimit = value >= max;
  const lines = hint.filter((line) => atLimit || !line.atLimitOnly);
  const open =
    !disabled && !dismissed && (hovered || focused) && lines.length > 0;

  const set = (next: number) => {
    const clamped = Math.min(max, Math.max(1, next));
    setDraft(String(clamped));
    if (clamped !== value) onChange(clamped);
    return clamped;
  };
  const stepBy = (delta: number, button: HTMLButtonElement) => {
    const next = set(value + delta);
    // The button disables at either end; the focus it held stays in the stepper.
    if ((next >= max || next <= 1) && document.activeElement === button) {
      inputRef.current?.focus();
    }
  };
  const step = `grid h-9 w-7 place-items-center rounded-md text-muted-foreground transition-colors duration-100 hover:text-foreground disabled:pointer-events-none disabled:opacity-40 [&_svg]:size-3.5 ${FOCUS_RING}`;

  return (
    <Tooltip open={open}>
      <TooltipTrigger asChild>
        <div
          role="group"
          aria-label={t("templates.count")}
          data-slot="profile-count-stepper"
          className="inline-flex h-9 shrink-0 items-center rounded-md bg-foreground/5"
          onPointerEnter={() => {
            setHovered(true);
            setDismissed(false);
          }}
          onPointerLeave={() => setHovered(false)}
          onFocus={(event) => {
            // A click focuses a button without showing it; only a focus the
            // person sees keeps the tooltip up once the pointer has left.
            setFocused(event.target.matches(":focus-visible"));
            setDismissed(false);
          }}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget)) {
              setFocused(false);
            }
          }}
        >
          <button
            type="button"
            aria-label={t("templates.fewer")}
            data-slot="profile-count-fewer"
            className={step}
            disabled={disabled || value <= 1}
            onClick={(event) => stepBy(-1, event.currentTarget)}
          >
            <LuMinus aria-hidden="true" />
          </button>
          <input
            ref={inputRef}
            id="profile-count"
            data-slot="profile-count"
            inputMode="numeric"
            aria-label={t("templates.count")}
            aria-describedby={lines.length > 0 ? hintId : undefined}
            value={draft}
            disabled={disabled}
            onChange={(event) => {
              const digits = event.target.value.replace(/\D/g, "");
              if (digits) set(Number.parseInt(digits, 10));
              else setDraft("");
            }}
            onBlur={() => {
              if (!draft) setDraft(String(value));
            }}
            onKeyDown={(event) => {
              if (event.key === "ArrowUp" || event.key === "ArrowDown") {
                event.preventDefault();
                set(value + (event.key === "ArrowUp" ? 1 : -1));
              }
            }}
            className="w-8 bg-transparent text-center text-sm tabular-nums outline-none disabled:opacity-50"
          />
          <button
            type="button"
            aria-label={t("templates.more")}
            data-slot="profile-count-more"
            className={step}
            disabled={disabled || atLimit}
            onClick={(event) => stepBy(1, event.currentTarget)}
          >
            <LuPlus aria-hidden="true" />
          </button>
          <span id={hintId} className="sr-only">
            {lines.map((line) => line.text).join(" ")}
          </span>
        </div>
      </TooltipTrigger>
      <TooltipContent
        side="top"
        sideOffset={6}
        data-slot="profile-count-hint"
        className="max-w-64 space-y-1"
        onEscapeKeyDown={() => setDismissed(true)}
      >
        {lines.map((line) => (
          <p
            key={line.text}
            data-limit={line.limit && atLimit ? "" : undefined}
            className={cn(line.limit && atLimit && "font-medium")}
          >
            {line.text}
          </p>
        ))}
      </TooltipContent>
    </Tooltip>
  );
}
