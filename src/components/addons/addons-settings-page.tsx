"use client";

import { useEffect, useState } from "react";
import {
  LuCode,
  LuPalette,
  LuPuzzle,
  LuShieldAlert,
  LuTerminal,
} from "react-icons/lu";
import { toast } from "sonner";
import { addonManager } from "@/lib/addons/addon-manager";
import type { ToastOptions } from "@/lib/addons/types";
import { CustomCssTab } from "./custom-css-tab";
import { DevtoolsTab } from "./devtools-tab";
import { PluginsTab } from "./plugins-tab";
import { SecurityTab } from "./security-tab";
import { ThemesTab } from "./themes-tab";

type TabId = "themes" | "css" | "plugins" | "security" | "devtools";

interface TabItem {
  id: TabId;
  label: string;
  icon: typeof LuPalette;
}

const TABS: TabItem[] = [
  { id: "themes", label: "Темы", icon: LuPalette },
  { id: "css", label: "CSS", icon: LuCode },
  { id: "plugins", label: "Плагины", icon: LuPuzzle },
  { id: "security", label: "Безопасность", icon: LuShieldAlert },
  { id: "devtools", label: "Разработчику", icon: LuTerminal },
];

export function AddonsSettingsPage() {
  const [activeTab, setActiveTab] = useState<TabId>("themes");

  // Wire plugin toasts to Sonner
  useEffect(() => {
    return addonManager.registerToastHandler((opts: ToastOptions) => {
      if (opts.variant === "success") {
        toast.success(opts.title, { description: opts.description });
      } else if (opts.variant === "destructive") {
        toast.error(opts.title, { description: opts.description });
      } else if (opts.variant === "warning") {
        toast.warning(opts.title, { description: opts.description });
      } else {
        toast(opts.title, { description: opts.description });
      }
    });
  }, []);

  return (
    <div className="flex flex-col gap-6 w-full">
      {/* Tab Navigation */}
      <div className="flex items-center gap-1.5 border-b border-border pb-2 overflow-x-auto">
        {TABS.map((tab) => {
          const Icon = tab.icon;
          const isActive = activeTab === tab.id;
          return (
            <button
              key={tab.id}
              type="button"
              onClick={() => setActiveTab(tab.id)}
              className={`flex items-center gap-2 rounded-md px-3 py-1.5 text-xs font-medium transition-all cursor-pointer ${
                isActive
                  ? "bg-primary text-primary-foreground shadow-xs"
                  : "text-muted-foreground hover:bg-foreground/5 hover:text-foreground"
              }`}
            >
              <Icon className="size-3.5" />
              <span>{tab.label}</span>
            </button>
          );
        })}
      </div>

      {/* Tab Content */}
      <div className="min-h-[450px]">
        {activeTab === "themes" && <ThemesTab />}
        {activeTab === "css" && <CustomCssTab />}
        {activeTab === "plugins" && <PluginsTab />}
        {activeTab === "security" && <SecurityTab />}
        {activeTab === "devtools" && <DevtoolsTab />}
      </div>
    </div>
  );
}
