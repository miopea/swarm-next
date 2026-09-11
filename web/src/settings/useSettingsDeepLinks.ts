import { useEffect } from "react";
import { readSettingsSection, type SettingsSection } from "./settingsNavigation";

/** Follow settings links from other surfaces without reloading the Hive. */
export function useSettingsDeepLinks(onNavigate: (section: SettingsSection) => void) {
  useEffect(() => {
    const followLink = () => {
      // Jira callback queries are a startup concern, not a reason to capture
      // unrelated anchors later in this browser session.
      const section = readSettingsSection(window.location.hash, "");
      if (section) onNavigate(section);
    };
    window.addEventListener("hashchange", followLink);
    return () => window.removeEventListener("hashchange", followLink);
  }, [onNavigate]);
}
