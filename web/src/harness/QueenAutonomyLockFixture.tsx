import SettingsWorkspace from "../settings/SettingsWorkspace";

const noop = () => undefined;
const resolved = () => Promise.resolve();

/**
 * A member whose Apiary sets Queen's Night Watch level (ADR 0113). Fictional
 * Hive and operator; changes go nowhere.
 */
export default function QueenAutonomyLockFixture() {
  return (
    <SettingsWorkspace
      section="settings-workers"
      busy={false}
      colorTheme="light"
      feedbackRevision={0}
      liveFeedState="connected"
      operatorToken="fixture-only"
      presence={{ mode: "at_hive", manual_mode: null, source: "active_device" }}
      lockDetectionState="unsupported"
      notificationSettings={{ policy: "important_only", subscription_count: 0, vapid_public_key: "fixture" }}
      queenPolicy={{
        at_hive: "coordinate",
        away: "coordinate",
        night_watch: "advisory",
        set_by_apiary: { at_hive: null, away: null, night_watch: "advisory" },
      }}
      notificationState="available"
      recentEvents={[]}
      sessions={[]}
      workers={[]}
      workspaces={[]}
      providers={{ claude_code: true, codex: false }}
      health={{ status: "ok", version: "1.17.0" }}
      hiveIdentity={{ operator: { id: "operator-1", display_name: "Bea" }, hive: { id: "hive-1", name: "Clover Hive", operator_id: "operator-1", apiary_id: "apiary-1" } }}
      startSurface="tasks"
      onStartSurfaceChange={noop}
      onLock={noop}
      onThemeChange={noop}
      onPresenceChange={resolved}
      onEnableLockDetection={resolved}
      onNotificationPolicyChange={resolved}
      onQueenPolicyChange={resolved}
      onEnableNotifications={resolved}
      onDisableNotifications={resolved}
      onTestNotification={resolved}
      onCreateWorker={resolved}
      onUpdateWorker={resolved}
      onChooseWorkerMark={resolved}
      onRemoveWorker={resolved}
      onReorderWorkers={resolved}
      onRestartProviders={resolved}
      onRestartForSettings={resolved}
      onUpdateWorkerEngine={resolved}
      onForceWorkerReload={resolved}
      onReloadDevelopment={resolved}
      onHiveIdentityChange={noop}
    />
  );
}
