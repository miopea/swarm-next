import PublicAddressWarning from "./PublicAddressWarning";
import { lazy, Suspense, useEffect, useMemo, useRef, useState, type CSSProperties, type FormEvent } from "react";

import {
  assignTask,
  BROWSER_SESSION_AUTH,
  createBrowserSession,
  createTask,
  fetchDevelopmentRuntime,
  answerDecision,
  resolveDecision,
  claimWorker,
  fetchEmailTasksAwaitingReply,
  fetchStartSurface,
  setStartSurface as setStartSurfaceRequest,
  sendEmailReply,
  reviseEmailReplyDraft,
  prepareEmailReply,
  updateEmailReplyDraft,
  restartSupersededWorkers,
  runQueenAutomation,
  type UnansweredEmailTask,
  type DecisionSurface,
  createWorker,
  fetchHealth,
  fetchTerminalHostStatus,
  fetchJiraTaskLinks,
  retryJiraTaskLink,
  fetchNotificationSettings,
  fetchQueenAutonomyPolicy,
  fetchCoordinatorStatus,
  fetchQueenAutomationStatus,
  fetchPresence,
  fetchProviderCapabilities,
  fetchPresentationPreferences,
  fetchTaskActivity,
  fetchRecentTaskActivity,
  fetchJiraComments,
  addJiraComment,
  removeWorker,
  removeTask,
  restoreTask,
  reorderTasks,
  reorderWorkers,
  reconcileJira,
  recoverTransientRuntime,
  requestDevelopmentReload,
  RuntimeRequestError,
  releaseWorkerEngagement,
  revokeBrowserSession,
  setManualPresence,
  setPresentationPreferences,
  setQueenAutonomyPolicy,
  startWorker,
  stopClaudeSession,
  stopWorker,
  transitionTask,
  updateWorker,
  updateWorkerEngine,
  updateTask,
  type DecisionRequest,
  type Health,
  type HiveIdentity,
  type JiraTaskLink,
  type NotificationPolicy,
  type NotificationSettings,
  type OperatorPresence,
  type PresenceMode,
  type ProviderKind,
  type ProviderCapabilities,
  type PresentationDeviceClass,
  type QueenAutonomyPolicy,
  type HeldDelivery,
  type QueenAutomationStatus,
  type SessionSummary,
  type Task,
  type TaskDraftInput,
  type TaskState,
  type TaskUpdateInput,
  fetchWorkerRepository,
  type RepositoryState,
  type Worker,
  type WorkspaceChoice,
  readTunnel,
  stopTunnel,
  type TunnelStatus,
  recordAttentionSeen,
} from "./api";
import BeeMascot from "./brand/BeeMascot";
import ApiaryAttentionCard from "./apiary/ApiaryAttentionCard";
import QueenAutomationAttentionCard from "./orchestration/QueenAutomationAttentionCard";
import UnansweredEmailAttentionCard from "./tasks/UnansweredEmailAttentionCard";
import HeldDeliveryAttentionCard from "./orchestration/HeldDeliveryAttentionCard";
import { passkeysSupported, signInWithPasskey } from "./settings/passkeys";
import { configureTerminalImageLimit } from "./terminal/TerminalAttachments";
import { queenAutomationNeedsAttention } from "./orchestration/queenAutomationPresentation";
import { foreignEngagement, workerAttention, workerSwitcherDetail } from "./workers/workerAttention";
import DecisionInbox from "./decisions/DecisionInbox";
import DogfoodFeedbackDialog from "./feedback/DogfoodFeedbackDialog";
import CommandPalette, { type CommandChoice } from "./navigation/CommandPalette";
import { applyColorTheme, initialColorTheme, type ColorTheme } from "./brand/theme";
import { ControlRoomLiveFeed, type LiveFeedState } from "./controlRoom/ControlRoomLiveFeed";
import RuntimeUpdateConfirm from "./runtime/RuntimeUpdateConfirm";
import type { RuntimeUpdateSummary } from "./runtime/runtimeUpdates";
import HiveContextIndicator from "./controlRoom/HiveContextIndicator";
import { useControlRoomModel } from "./controlRoom/useControlRoomModel";
import { SETTINGS_SECTIONS, clearSettingsSection, navigateToSettingsSection, readSettingsSection, type SettingsSection } from "./settings/settingsNavigation";
import { isSurface, readSavedSurface, saveSurface, surfaceWasRequested, type Surface } from "./navigation/startSurface";
import { PresenceController, deviceClass, presenceDeviceId, type LockDetectionState } from "./presence/PresenceController";
import { NotificationController, type NotificationCapabilityState } from "./notifications/NotificationController";
import TaskBoardControls, { type TaskBoardFilter, type TaskBoardSort, type TaskBoardSource } from "./tasks/TaskBoardControls";
import TerminalLoadBoundary from "./terminal/TerminalLoadBoundary";
import { initialMobileKeysVisibility, rememberMobileKeysVisibility } from "./terminal/MobileTerminalComposer";
import { terminalWorkspace } from "./terminal/TerminalWorkspace";
import WorkerRosterItem from "./workers/WorkerRosterItem";
import WorkerContextBar from "./workers/WorkerContextBar";
import { workerWork } from "./workers/workerWork";
import { normalizeRosterQuery, orphanSessionMatchesRosterQuery, repositoryName, workerMatchesRosterQuery } from "./workers/workerRoster";
import { useWorkerRailWidth } from "./layout/useWorkerRailWidth";
import { useModalFocus } from "./shared/useModalFocus";
import { isExpectedRuntimeHandoff, requestRuntimeHandoff } from "./runtime/runtimeMaintenance";
import { useRuntimeUpdate } from "./runtime/useRuntimeUpdate";
import {
  detachedSurface,
  focusDetachedSurface,
  openSurfaceWindow,
  surfaceIsDetached,
} from "./navigation/surfaceWindow";
import { measureRoutePaint } from "./runtime/routePaint";
import { workerEngineMatches } from "./runtime/workerEngine";

const loadTerminalView = () => import("./terminal/TerminalView");
const TerminalView = lazy(loadTerminalView);
const TaskBoard = lazy(() => import("./tasks/TaskBoard"));
const SettingsWorkspace = lazy(() => import("./settings/SettingsWorkspace"));
const KeeperControlRoom = lazy(() => import("./apiary/KeeperControlRoom"));
const MemberControlRoom = lazy(() => import("./apiary/MemberControlRoom"));
/**
 * How often held work is re-read.
 *
 * A hold only becomes an item after a two-minute grace period, so it is not
 * urgent to the second — but it must appear without a reload, and disappear
 * without one too.
 */
const HELD_DELIVERY_POLL_MS = 20_000;

const ACTIVE_SESSION_STORAGE_KEY = "swarm-next.active-session.v1";
const WORKER_VISIBILITY_STORAGE_KEY = "swarm-next.worker-visibility.v1";

type LoadState = { kind: "loading" } | { kind: "ready"; health: Health } | { kind: "unavailable" };
type WorkerVisibility = "all" | "awake";

function workerName(sessionId: string): string {
  return `Claude ${sessionId.slice(-4).toUpperCase()}`;
}

export function App() {
  const [loadState, setLoadState] = useState<LoadState>({ kind: "loading" });
  const [tokenDraft, setTokenDraft] = useState("");
  const [operatorToken, setOperatorToken] = useState<string>();
  const controlRoomModel = useControlRoomModel();
  const {
    hiveIdentity, sessions, workers, workspaces, tasks, jiraTaskLinks, decisions, stewardAssists, recentEvents,
    setHiveIdentity, setWorkers, setWorkspaces, setTasks,
    setJiraTaskLinks, setDecisions,
  } = controlRoomModel;
  const loadControlRoom = controlRoomModel.load;
  const keeper = hiveIdentity?.apiary_context?.mode === "federated" && hiveIdentity.apiary_context.local_role === "keeper";
  const federated = hiveIdentity?.apiary_context?.mode === "federated";
  const [activeSessionId, setActiveSessionId] = useState<string>();
  const [terminalRevision, setTerminalRevision] = useState(0);
  const [showFeedback, setShowFeedback] = useState(false);
  const [feedbackRevision, setFeedbackRevision] = useState(0);
  const [showCommands, setShowCommands] = useState(false);
  const [showMobileWorkers, setShowMobileWorkers] = useState(false);
  // No search field on the phone, and so no initial focus to give one. Opening
  // the picker used to focus a text input, which raised the keyboard over the
  // list the operator had just asked to see. Focus falls to the first control
  // in the dialog instead, which does not.
  const mobileWorkerDialog = useModalFocus<HTMLElement>(() => setShowMobileWorkers(false), showMobileWorkers);
  const [workerVisibility, setWorkerVisibility] = useState<WorkerVisibility>(readWorkerVisibility);
  const [workerQuery, setWorkerQuery] = useState("");
  const [terminalConnection, setTerminalConnection] = useState<string>();
  const { runtimeUpdates, developmentMode, refreshRuntimeUpdate } = useRuntimeUpdate(operatorToken || undefined);
  const [runtimeConfirm, setRuntimeConfirm] = useState<RuntimeUpdateSummary>();
  const [startSurface, setStartSurface] = useState("tasks");
  /** Applied once, and only while nothing else has claimed the screen. */
  const openedAtLaunch = useRef(true);
  // Settings navigates from the rail like every other surface, so the rail has
  // to know which section is current. The hash stays the source of truth, since
  // a link into a section has to keep working.
  /** What the operator typed into the settings filter. */
  const [settingsQuery, setSettingsQuery] = useState("");
  const [settingsSection, setSettingsSection] = useState<SettingsSection>(
    () => readSettingsSection() ?? "settings-hive",
  );
  // Polled rather than derived from the board, because the board is one surface
  // and a finished task with nobody answered belongs in Needs you regardless of
  // where the operator is standing.
  const [awaitingReply, setAwaitingReply] = useState<UnansweredEmailTask[]>([]);
  useEffect(() => {
    if (!operatorToken) {
      setAwaitingReply([]);
      return;
    }
    let current = true;
    // Only replaced by something that is actually a list. A malformed answer
    // during a rolling update should not take the whole attention queue down
    // with it.
    const load = () => void fetchEmailTasksAwaitingReply(operatorToken)
      .then((awaiting) => { if (current && Array.isArray(awaiting)) setAwaitingReply(awaiting); })
      .catch(() => undefined);
    load();
    const interval = window.setInterval(load, 30_000);
    return () => { current = false; window.clearInterval(interval); };
  }, [operatorToken, feedbackRevision]);
  const [repository, setRepository] = useState<RepositoryState | null>();
  const [popoutBlocked, setPopoutBlocked] = useState(false);
  // A window opened to show one surface shows exactly that: no navigation, no
  // second copy of everything. Duplicating the whole app was what made a
  // pop-out indistinguishable from another window of the same thing.
  const detached = detachedSurface();
  const [surface, setSurface] = useState<Surface>(() => new URLSearchParams(window.location.search).has("jira") || readSettingsSection() ? "settings" : readSavedSurface());
  const [taskFocus, setTaskFocus] = useState<{ id: string; request: number }>();
  const [taskComposeRequest, setTaskComposeRequest] = useState(0);
  const [taskQuery, setTaskQuery] = useState("");
  const [taskFilter, setTaskFilter] = useState<TaskBoardFilter>("all");
  const [taskSource, setTaskSource] = useState<TaskBoardSource>("all");
  const [taskSort, setTaskSort] = useState<TaskBoardSort>("queue");
  const [taskProject, setTaskProject] = useState("all");
  const [taskWorker, setTaskWorkerFilter] = useState("all");
  const workerRail = useWorkerRailWidth();
  const [decisionFocus, setDecisionFocus] = useState<{ id: string; request: number }>();
  const [operationError, setOperationError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [busyLabel, setBusyLabel] = useState<string>();
  const [workerEngineProgress, setWorkerEngineProgress] = useState<string>();
  const [colorTheme, setColorTheme] = useState<ColorTheme>(initialColorTheme);
  const [mobileKeysVisible, setMobileKeysVisible] = useState(initialMobileKeysVisibility);
  const [liveFeedState, setLiveFeedState] = useState<LiveFeedState>("connecting");
  // Whether this Hive is currently reachable from the internet.
  //
  // Read app-wide rather than only inside the settings card that starts it: a
  // quick tunnel publishes the whole Hive, and knowing that was previously
  // confined to the one screen an operator had to already be on. The operator's
  // ruling is that the exposure is acceptable and being unaware of it is not.
  const [publicAddress, setPublicAddress] = useState<TunnelStatus>();
  // Polled, not pushed. The control-room feed carries task and worker change,
  // and a tunnel is neither; a fifteen-second local read is cheaper than a new
  // event kind for something an operator starts by hand.
  useEffect(() => {
    if (!operatorToken || detached) return undefined;
    let cancelled = false;
    const read = () => {
      void readTunnel(operatorToken)
        .then((status) => { if (!cancelled) setPublicAddress(status); })
        .catch(() => { /* a Hive that cannot answer is not publishing */ });
    };
    read();
    const timer = window.setInterval(read, 15_000);
    return () => { cancelled = true; window.clearInterval(timer); };
  }, [operatorToken, detached]);

  const [presence, setPresence] = useState<OperatorPresence>();
  const [lockDetectionState, setLockDetectionState] = useState<LockDetectionState>("unsupported");
  const [notificationSettings, setNotificationSettings] = useState<NotificationSettings>();
  const [queenPolicy, setQueenPolicy] = useState<QueenAutonomyPolicy>();
  const [queenAutomation, setQueenAutomation] = useState<QueenAutomationStatus>();
  /** What the coordinator is holding behind an unanswered terminal prompt. */
  const [heldDeliveries, setHeldDeliveries] = useState<HeldDelivery[]>([]);
  /** Bumped to re-read held work immediately rather than waiting for the tick. */
  const [heldDeliveryRefresh, setHeldDeliveryRefresh] = useState(0);
  const [providers, setProviders] = useState<ProviderCapabilities>({ claude_code: true, codex: false });
  const [providerCapabilitiesUnavailable, setProviderCapabilitiesUnavailable] = useState(false);
  const [notificationState, setNotificationState] = useState<NotificationCapabilityState>("unsupported");
  const presenceController = useMemo(() => new PresenceController(), []);
  const notificationController = useMemo(() => new NotificationController(), []);
  const presentationDevice = useMemo<PresentationDeviceClass>(() => deviceClass(), []);

  useEffect(() => applyColorTheme(colorTheme), [colorTheme]);
  useEffect(() => {
    saveSurface(surface);
    if (surface !== "settings") clearSettingsSection();
  }, [surface]);
  useEffect(() => {
    if (surface === "apiary" && hiveIdentity && !federated) setSurface("tasks");
  }, [federated, hiveIdentity, surface]);
  useEffect(() => {
    if (activeSessionId) saveActiveSessionId(activeSessionId);
  }, [activeSessionId]);

  useEffect(() => { void loadTerminalView().catch(() => undefined); }, []);
  useEffect(() => {
    if (!operatorToken) {
      presenceController.stop();
      setPresence(undefined);
      return;
    }
    // The operator's chosen opening screen, applied when nothing more specific
    // asked for one — a link, a settings section, or a surface already chosen
    // in this tab all still win, because they are a request rather than a
    // default.
    // Wrapped so a synchronous throw from fetch cannot abort this effect before
    // presence starts. A rejected promise was handled; a thrown one was not.
    void (async () => fetchStartSurface(operatorToken))()
      .then((chosen) => {
        setStartSurface(chosen);
        if (!openedAtLaunch.current) return;
        openedAtLaunch.current = false;
        if (surfaceWasRequested()) return;
        if (isSurface(chosen)) setSurface(chosen);
      })
      .catch(() => undefined);
    presenceController.start(operatorToken, setPresence, setLockDetectionState);
    return () => presenceController.stop();
  }, [operatorToken, presenceController]);

  // A tapped notification says where it wants to go. The service worker also
  // tries to navigate the window, but in an installed PWA that is usually a
  // no-op, and focus() alone brings the app up on whatever it was already
  // showing — which is why opening a "needs you" notification landed on the
  // opening screen instead of the thing that needed you.
  useEffect(() => {
    if (detached || !("serviceWorker" in navigator)) return undefined;
    const onMessage = (event: MessageEvent) => {
      const requested = (event.data as { type?: string; surface?: string } | null)?.type === "swarm-show-surface"
        ? (event.data as { surface?: string }).surface
        : undefined;
      if (isSurface(requested)) setSurface(requested);
    };
    navigator.serviceWorker.addEventListener("message", onMessage);
    return () => navigator.serviceWorker.removeEventListener("message", onMessage);
  }, [detached]);

  useEffect(() => {
    if (!operatorToken) {
      notificationController.stop();
      setNotificationSettings(undefined);
      setNotificationState("unsupported");
      return;
    }
    void notificationController.start(operatorToken, setNotificationSettings, setNotificationState);
    return () => notificationController.stop();
  }, [notificationController, operatorToken]);

  useEffect(() => {
    if (!operatorToken) {
      setQueenPolicy(undefined);
      setQueenAutomation(undefined);
      return;
    }
    void fetchQueenAutonomyPolicy(operatorToken)
      .then(setQueenPolicy)
      .catch((error: unknown) => setOperationError(error instanceof Error ? error.message : "Queen policy could not be loaded"));
    void fetchQueenAutomationStatus(operatorToken)
      .then(setQueenAutomation)
      .catch(() => setQueenAutomation(undefined));
  }, [operatorToken]);

  // Held work is polled, not read once at sign-in.
  //
  // The comment here used to say "polled with the rest of the control room"
  // while the effect depended on the token alone, so it ran once and never
  // again. That broke it in both directions: a hold that began after sign-in
  // never appeared, and a hold that cleared stayed on the "Needs you" page
  // until the page was reloaded. The operator was shown a card an hour after
  // the thing it described had resolved, with a button that did nothing.
  useEffect(() => {
    if (!operatorToken) {
      setHeldDeliveries([]);
      return undefined;
    }
    let cancelled = false;
    const load = () => {
      void fetchCoordinatorStatus(operatorToken)
        // Defaulted rather than assumed: during an update the page can briefly
        // be newer than the API answering it, and a missing field should not
        // take the control room down.
        .then((status) => { if (!cancelled) setHeldDeliveries(status.held ?? []); })
        .catch(() => { if (!cancelled) setHeldDeliveries([]); });
    };
    load();
    const interval = window.setInterval(load, HELD_DELIVERY_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(interval);
    };
  }, [operatorToken, heldDeliveryRefresh]);

  useEffect(() => {
    if (!operatorToken) return;
    void fetchProviderCapabilities(operatorToken)
      .then((nextProviders) => {
        setProviders(nextProviders);
        setProviderCapabilitiesUnavailable(false);
      })
      .catch(() => setProviderCapabilitiesUnavailable(true));
  }, [operatorToken]);

  useEffect(() => {
    if (!operatorToken) return;
    let cancelled = false;
    void fetchPresentationPreferences(operatorToken, presentationDevice)
      .then(async (preferences) => preferences.configured
        ? preferences
        : setPresentationPreferences(operatorToken, {
            device_class: presentationDevice,
            color_theme: initialColorTheme(),
            terminal_keys_visible: initialMobileKeysVisibility(),
          }))
      .then((preferences) => {
        if (cancelled) return;
        setColorTheme(preferences.color_theme);
        setMobileKeysVisible(preferences.terminal_keys_visible);
        rememberMobileKeysVisibility(preferences.terminal_keys_visible);
      })
      .catch((error: unknown) => {
        if (!cancelled) setOperationError(error instanceof Error ? error.message : "Presentation preferences could not be loaded");
      });
    return () => { cancelled = true; };
  }, [operatorToken, presentationDevice]);

  useEffect(() => {
    let cancelled = false;
    void recoverTransientRuntime(fetchHealth)
      .then((health) => {
        if (cancelled) return;
        // The Hive's own number, so an oversized image is refused with the
        // limit that is actually enforced rather than a copy that can drift.
        if (health.attachment_max_bytes) configureTerminalImageLimit(health.attachment_max_bytes);
        setLoadState({ kind: "ready", health });
      })
      .catch((error: unknown) => {
        if (!cancelled) setLoadState({ kind: "unavailable" });
      });
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    const controller = new AbortController();
    void recoverTransientRuntime(async () => {
      const next = await controlRoomModel.restoreBrowserSession(controller.signal);
      if (!next) throw new DOMException("Aborted", "AbortError");
      return next;
    })
      .then((nextControlRoom) => {
        if (controller.signal.aborted) return;
        terminalWorkspace.authenticate(BROWSER_SESSION_AUTH);
        setOperatorToken(BROWSER_SESSION_AUTH);
        setActiveSessionId(restoredSessionId(nextControlRoom.workers, nextControlRoom.sessions));
      })
      .catch((error: unknown) => {
        if (controller.signal.aborted) return;
        terminalWorkspace.logout();
        if (!(error instanceof Error && error.message.includes("401"))) {
          setOperationError(error instanceof Error ? error.message : "Saved authentication could not be restored");
        }
      })
    return () => controller.abort();
  }, []);

  useEffect(() => {
    if (!operatorToken) {
      setLiveFeedState("connecting");
      return;
    }
    const controller = new AbortController();
    const feed = new ControlRoomLiveFeed();
    const connect = () => feed.start(
      operatorToken,
      async (page) => {
        const runtimeChanged = page.events.some((event) => event.kind === "runtime_changed");
        const refreshQueenAutomation = page.events.some((event) => event.kind === "workers_changed");
        const [controlRoom, refreshedPresence, refreshedNotifications, refreshedQueenPolicy, refreshedQueenAutomation, refreshedPresentation, refreshedProviders] = await Promise.all([
          controlRoomModel.refreshFromEvents(operatorToken, page, controller.signal),
          page.events.some((event) => event.kind === "presence_changed")
            ? fetchPresence(operatorToken)
            : Promise.resolve(undefined),
          page.events.some((event) => event.kind === "notifications_changed")
            ? fetchNotificationSettings(operatorToken)
            : Promise.resolve(undefined),
          runtimeChanged
            ? fetchQueenAutonomyPolicy(operatorToken)
            : Promise.resolve(undefined),
          refreshQueenAutomation
            ? fetchQueenAutomationStatus(operatorToken).catch(() => undefined)
            : Promise.resolve(undefined),
          runtimeChanged
            ? fetchPresentationPreferences(operatorToken, presentationDevice)
            : Promise.resolve(undefined),
          runtimeChanged
            ? fetchProviderCapabilities(operatorToken).catch(() => {
                setProviderCapabilitiesUnavailable(true);
                return undefined;
              })
            : Promise.resolve(undefined),
        ]);
        if (controller.signal.aborted || !controlRoom) return;
        if (refreshedPresence) setPresence(refreshedPresence);
        if (refreshedNotifications) setNotificationSettings(refreshedNotifications);
        if (refreshedQueenPolicy) setQueenPolicy(refreshedQueenPolicy);
        if (refreshedQueenAutomation) setQueenAutomation(refreshedQueenAutomation);
        if (refreshedProviders) {
          setProviders(refreshedProviders);
          setProviderCapabilitiesUnavailable(false);
        }
        if (refreshedPresentation?.configured) {
          setColorTheme(refreshedPresentation.color_theme);
          setMobileKeysVisible(refreshedPresentation.terminal_keys_visible);
          rememberMobileKeysVisibility(refreshedPresentation.terminal_keys_visible);
        }
        setActiveSessionId((current) =>
          current && controlRoom.sessions.some((session) => session.session_id === current)
            ? current
            : preferredSessionId(controlRoom.workers, controlRoom.sessions),
        );
      },
      setLiveFeedState,
    );
    connect();
    // A phone suspends a backgrounded tab, and the poll it left in flight may
    // never settle — it neither answers nor fails. The feed abandons such a
    // poll at its own ceiling, but reconnecting the moment the page comes back
    // makes the roster current immediately instead of after that wait, which is
    // the difference between returning to live work and returning to a screen
    // frozen where it stood.
    const reconnectOnReturn = () => {
      if (document.visibilityState === "visible") connect();
    };
    document.addEventListener("visibilitychange", reconnectOnReturn);
    return () => {
      document.removeEventListener("visibilitychange", reconnectOnReturn);
      controller.abort();
      feed.stop();
    };
  }, [operatorToken, presentationDevice]);

  async function authenticate(event: FormEvent) {
    event.preventDefault();
    if (!tokenDraft) return;
    await perform(async () => {
      await createBrowserSession(tokenDraft);
      const controlRoom = await loadControlRoom(BROWSER_SESSION_AUTH);
      terminalWorkspace.authenticate(BROWSER_SESSION_AUTH);
      setOperatorToken(BROWSER_SESSION_AUTH);
      controlRoomModel.replace(controlRoom);
      setActiveSessionId((current) => current ?? preferredSessionId(controlRoom.workers, controlRoom.sessions));
      setTokenDraft("");
    });
  }

  function changeColorTheme(theme: ColorTheme) {
    setColorTheme(theme);
    if (!operatorToken) return;
    void perform(async () => {
      await setPresentationPreferences(operatorToken, {
        device_class: presentationDevice,
        color_theme: theme,
        terminal_keys_visible: mobileKeysVisible,
      });
    });
  }

  function changeWorkerVisibility(visibility: WorkerVisibility) {
    setWorkerVisibility(visibility);
    rememberWorkerVisibility(visibility);
  }

  function changeMobileKeysVisibility(visible: boolean) {
    setMobileKeysVisible(visible);
    rememberMobileKeysVisibility(visible);
    if (!operatorToken) return;
    void perform(async () => {
      await setPresentationPreferences(operatorToken, {
        device_class: presentationDevice,
        color_theme: colorTheme,
        terminal_keys_visible: visible,
      });
    });
  }

  async function changePresenceMode(mode: PresenceMode | null) {
    if (!operatorToken) return;
    await perform(async () => setPresence(await setManualPresence(operatorToken, mode)));
  }

  async function enableLockDetection() {
    await perform(async () => { await presenceController.enableLockDetection(); });
  }
  async function changeNotificationPolicy(policy: NotificationPolicy) {
    await perform(() => notificationController.changePolicy(policy));
  }

  async function enableNotifications() {
    await perform(async () => { await notificationController.enable(); });
  }

  async function changeQueenPolicy(policy: QueenAutonomyPolicy) {
    if (!operatorToken) return;
    await perform(async () => setQueenPolicy(await setQueenAutonomyPolicy(operatorToken, policy)));
  }

  async function disableNotifications() {
    await perform(() => notificationController.disable());
  }

  async function testNotification() {
    await perform(() => notificationController.test());
  }
  async function refreshControlRoom(recoverTerminal = false) {
    if (!operatorToken) return;
    await perform(async () => {
      const [controlRoom, automation] = await Promise.all([
        loadControlRoom(operatorToken),
        fetchQueenAutomationStatus(operatorToken).catch(() => undefined),
      ]);
      controlRoomModel.replace(controlRoom);
      if (automation) setQueenAutomation(automation);
      // The indicator keeps itself current on a timer; this is so an operator
      // who presses refresh does not wait out the next tick.
      void refreshRuntimeUpdate();
      setHeldDeliveryRefresh((current) => current + 1);
      if (recoverTerminal && activeSessionId) {
        terminalWorkspace.resetSessionRenderer(activeSessionId);
        setTerminalRevision((current) => current + 1);
      }
      setActiveSessionId((current) =>
        current && controlRoom.sessions.some((session) => session.session_id === current)
          ? current
          : preferredSessionId(controlRoom.workers, controlRoom.sessions),
      );
    });
  }

  async function configureWorker(name: string, workspace: string, provider: ProviderKind, allowOutsideRoots: boolean) {
    if (!operatorToken) return;
    await perform(async () => {
      await createWorker(operatorToken, { name, workspace, provider, allow_outside_roots: allowOutsideRoots });
      const controlRoom = await loadControlRoom(operatorToken);
      setWorkers(controlRoom.workers);
      setWorkspaces(controlRoom.workspaces);
    });
  }

  async function reorderWorkerProfiles(workerIds: string[]) {
    if (!operatorToken) return;
    await perform(async () => {
      await reorderWorkers(operatorToken, workerIds);
      const order = new Map(workerIds.map((workerId, index) => [workerId, index]));
      setWorkers((current) => [...current].sort((left, right) => {
        if (left.role === "queen") return -1;
        if (right.role === "queen") return 1;
        return (order.get(left.id) ?? Number.MAX_SAFE_INTEGER) - (order.get(right.id) ?? Number.MAX_SAFE_INTEGER);
      }));
    });
  }

  async function maintainWorkerProfile(workerId: string, name: string, description: string, provider: ProviderKind, autostart: boolean, workspace?: string, allowOutsideRoots?: boolean) {
    if (!operatorToken) return;
    await perform(async () => {
      const updated = await updateWorker(operatorToken, workerId, { name, description, provider, autostart, workspace, allow_outside_roots: allowOutsideRoots });
      setWorkers((current) => current.map((worker) => worker.id === updated.id ? updated : worker));
    });
  }

  async function removeWorkerProfile(workerId: string) {
    if (!operatorToken) return;
    await perform(async () => {
      await removeWorker(operatorToken, workerId);
      const controlRoom = await loadControlRoom(operatorToken);
      setWorkers(controlRoom.workers);
      setWorkspaces(controlRoom.workspaces);
    });
  }

  async function startWorkerForTask(task: Task) {
    if (!operatorToken) return;
    let startedSessionId: string | undefined;
    await perform(async () => {
      const profile = workers.find((worker) => worker.workspace === task.workspace && worker.role !== "queen");
      if (!profile) throw new Error("Choose a configured worker for this task before starting it.");
      const runningWorker = profile.running ? profile : await startWorker(operatorToken, profile.id);
      const sessionId = requireActiveSession(runningWorker);
      await assignTask(operatorToken, task.id, profile.id);
      await transitionTask(operatorToken, task.id, "active");
      const controlRoom = await loadControlRoom(operatorToken);
      controlRoomModel.replace(controlRoom);
      releaseEngagementWhenSwitching(activeSessionId, sessionId);
      setActiveSessionId(sessionId);
      setSurface("workers");
      startedSessionId = sessionId;
    });
    if (startedSessionId) focusTerminalAfterRender(startedSessionId);
  }

  async function stopSession(sessionId: string) {
    if (!operatorToken) return;
    await perform(async () => {
      const profile = workers.find((worker) => worker.active_session_id === sessionId);
      if (profile) await stopWorker(operatorToken, profile.id);
      else await stopClaudeSession(operatorToken, sessionId);
      terminalWorkspace.closeSession(sessionId);
      const controlRoom = await loadControlRoom(operatorToken);
      controlRoomModel.replace(controlRoom);
      setActiveSessionId((current) => current === sessionId ? preferredSessionId(controlRoom.workers, controlRoom.sessions) : current);
    });
  }

  async function startExistingWorker(profile: Worker) {
    if (!operatorToken) return;
    let startedSessionId: string | undefined;
    await perform(async () => {
      const runningWorker = await startWorker(operatorToken, profile.id);
      const sessionId = requireActiveSession(runningWorker);
      const controlRoom = await loadControlRoom(operatorToken);
      controlRoomModel.replace(controlRoom);
      releaseEngagementWhenSwitching(activeSessionId, sessionId);
      setActiveSessionId(sessionId);
      setSurface("workers");
      startedSessionId = sessionId;
    });
    if (startedSessionId) focusTerminalAfterRender(startedSessionId);
  }

  async function addTask(input: TaskDraftInput) {
    if (!operatorToken) return;
    await perform(async () => {
      const worker = workers.find((candidate) => candidate.id === input.worker_id && candidate.role !== "queen");
      if (!worker) throw new Error("Choose a configured worker for this task.");
      let task = await createTask(operatorToken, {
        title: input.title,
        description: input.description,
        priority: input.priority,
        workspace: worker.workspace,
      });
      task = await assignTask(operatorToken, task.id, worker.id);
      setTasks((current) => [task, ...current]);
    });
  }

  async function editTask(task: Task, input: TaskUpdateInput) {
    if (!operatorToken) return;
    setBusy(true);
    setOperationError(undefined);
    try {
      replaceTask(await updateTask(operatorToken, task.id, input));
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "The task could not be updated");
      throw error;
    } finally {
      setBusy(false);
    }
  }

  async function moveTask(task: Task, state: TaskState, note = "") {
    if (!operatorToken) return;
    await perform(async () => {
      const updated = await transitionTask(operatorToken, task.id, state, note);
      replaceTask(updated);
    });
  }

  async function removeTaskFromHive(task: Task) {
    if (!operatorToken) return;
    setBusy(true);
    setOperationError(undefined);
    try {
      await removeTask(operatorToken, task.id);
      setTasks((current) => current.filter((candidate) => candidate.id !== task.id));
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "The task could not be removed");
      throw error;
    } finally {
      setBusy(false);
    }
  }

  async function restoreTaskToHive(task: Task) {
    if (!operatorToken) return;
    setBusy(true);
    setOperationError(undefined);
    try {
      const restored = await restoreTask(operatorToken, task.id);
      setTasks((current) => [restored, ...current.filter((candidate) => candidate.id !== restored.id)]);
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "The task could not be restored");
      throw error;
    } finally {
      setBusy(false);
    }
  }

  async function setTaskWorker(task: Task, workerId: string) {
    if (!operatorToken) return;
    await perform(async () => {
      if (!workerId) {
        replaceTask(await assignTask(operatorToken, task.id, null));
        return;
      }
      const worker = workers.find((candidate) => candidate.id === workerId && candidate.role !== "queen");
      if (!worker) throw new Error("That worker is no longer configured.");
      let updated = await updateTask(operatorToken, task.id, { workspace: worker.workspace });
      updated = await assignTask(operatorToken, task.id, worker.id);
      replaceTask(updated);
    });
  }

  async function reorderOpenTasks(taskIds: string[]) {
    if (!operatorToken) return;
    await perform(async () => setTasks(await reorderTasks(operatorToken, taskIds)));
  }

  /**
   * Shows a surface, unless it is already open in a window of its own — then
   * that window comes forward. Two copies of one surface is what the pop-out
   * was creating, and the rail is where the operator would otherwise make the
   * second one.
   */
  function showSurface(next: Surface) {
    if (next !== "settings" && focusDetachedSurface(next)) return;
    setSurface(next);
  }

  async function resumeQueenReview() {
    if (!operatorToken) return;
    setQueenAutomation(await runQueenAutomation(operatorToken));
  }

  async function answerInboxDecision(decision: DecisionRequest, answers: Record<string, string[]>, note: string) {
    if (!operatorToken) return;
    await perform(async () => {
      const updated = await answerDecision(operatorToken, decision.id, answers, note);
      setDecisions((current) => current.map((item) => item.id === updated.id ? updated : item));
    }, "Sending your answers…");
  }

  async function resolveInboxDecision(decision: DecisionRequest, action: string, note: string, surface: DecisionSurface) {
    if (!operatorToken) return;
    await perform(async () => {
      const updated = await resolveDecision(operatorToken, decision.id, action, note, surface);
      setDecisions((current) => current.map((item) => item.id === updated.id ? updated : item));
    });
  }
  function replaceTask(updated: Task) {
    setTasks((current) => current.map((task) => task.id === updated.id ? updated : task));
  }

  async function perform(action: () => Promise<void>, progress = "Saving…") {
    setBusy(true);
    setBusyLabel(progress);
    setOperationError(undefined);
    try {
      await action();
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "The operation could not be completed");
    } finally {
      setBusy(false);
      setBusyLabel(undefined);
    }
  }

  function handleShortcut(event: KeyboardEvent) {
    if (!operatorToken || !event.altKey || event.ctrlKey || event.metaKey || isTypingTarget(event.target)) return;
    if (event.key.toLocaleLowerCase() === "k") {
      event.preventDefault();
      setShowCommands(true);
      return;
    }
    if (["1", "2", "3", "4"].includes(event.key)) {
      event.preventDefault();
      setSurface(event.key === "1" ? "decisions" : event.key === "2" ? "tasks" : event.key === "3" ? "workers" : "settings");
      return;
    }
    if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
    const running = workers.filter((worker) => worker.running && worker.active_session_id);
    if (running.length === 0) return;
    event.preventDefault();
    const currentIndex = running.findIndex((worker) => worker.active_session_id === activeSessionId);
    const direction = event.key === "ArrowDown" ? 1 : -1;
    const nextIndex = currentIndex < 0 ? 0 : (currentIndex + direction + running.length) % running.length;
    const nextSessionId = running[nextIndex]?.active_session_id;
    if (nextSessionId) openWorker(nextSessionId);
  }

  function openWorker(sessionId: string) {
    releaseEngagementWhenSwitching(activeSessionId, sessionId);
    setActiveSessionId(sessionId);
    setSurface("workers");
    focusTerminalAfterRender(sessionId);
  }

  async function retryTaskJira(task: Task) {
    if (!operatorToken) return;
    await perform(async () => {
      await retryJiraTaskLink(operatorToken, task.id);
      setJiraTaskLinks(await fetchJiraTaskLinks(operatorToken));
    });
  }

  async function syncJiraBoard() {
    if (!operatorToken) return;
    await perform(async () => {
      await reconcileJira(operatorToken);
      await refreshControlRoom();
    });
  }

  /**
   * Runs a runtime update the operator started from the control room.
   *
   * Routes to the same actions the settings page uses rather than duplicating
   * them, so there is one implementation of each and no second path to keep
   * true. The dialog has already asked; this only does it.
   */
  async function runRuntimeUpdate(update: RuntimeUpdateSummary) {
    setRuntimeConfirm(undefined);
    if (update.action === "build") await reloadDevelopmentBuild();
    else if (update.action === "apply_worker_engine") await maintainWorkerEngine();
    else if (update.action === "restart_providers") await restartProviders();
    await refreshRuntimeUpdate();
  }

  /**
   * Sends a reply the operator has just read, from where they read it.
   *
   * Reviewing the words is the operator's part of an email task. Whether the
   * work is running belongs to the worker that did it, and is recorded as
   * deployment evidence rather than asked of the operator here.
   */
  async function sendAwaitingReply(replyId: string) {
    if (!operatorToken) return;
    await perform(async () => {
      await sendEmailReply(operatorToken, replyId);
      setAwaitingReply(await fetchEmailTasksAwaitingReply(operatorToken));
    }, "Sending the reply…");
  }

  /**
   * Saves an edited reply without leaving this screen.
   *
   * Editing used to mean being thrown to the task board to find the task the
   * reply belonged to — the operator: "This kicks me to the task page to edit,
   * this should stay on the task page." Reading the words is the only part of
   * an email task that is theirs, and it was the one part that made them go
   * somewhere else to do it.
   */
  /**
   * Asks Claude to revise a draft, and hands the text back UNSAVED.
   *
   * Nothing is written here. The editor swaps the revision in and keeps what it
   * replaced, so a prompt that overshoots costs one Undo rather than a draft
   * the operator liked — which is the failure they are most exposed to, having
   * already said of one draft "I like how it's written".
   */
  async function reviseAwaitingReply(taskId: string, instruction: string): Promise<string | null> {
    if (!operatorToken) return null;
    let revised: string | null = null;
    await perform(async () => {
      revised = await reviseEmailReplyDraft(operatorToken, taskId, instruction);
    }, "Claude is revising the reply…");
    return revised;
  }

  async function saveAwaitingReply(taskId: string, body: string) {
    if (!operatorToken) return;
    await perform(async () => {
      // Prepare when there is no reply yet, update when there is. Every task on
      // this queue is Completed, and completing requires settled evidence — a
      // deployment or an approved exemption — so a reply can always be written
      // from here. The operator never needs the task page's deployment form,
      // which was asking them to record something that is the worker's to
      // verify and, for exemption-closed work, does not exist at all.
      const existing = awaitingReply.find((item) => item.task_id === taskId);
      if (existing?.draft_id) await updateEmailReplyDraft(operatorToken, taskId, body);
      else await prepareEmailReply(operatorToken, taskId, body);
      setAwaitingReply(await fetchEmailTasksAwaitingReply(operatorToken));
    }, "Saving the reply…");
  }

  /**
   * Takes a worker back for this screen, without sending it anything.
   *
   * ADR 0049. The claim is granted rather than negotiated — one operator moving
   * between screens is not two parties contending — on a shorter lease than
   * typing earns, and it does not move terminal geometry.
   */
  async function claimActiveWorker(workerId: string) {
    if (!operatorToken) return;
    await perform(async () => {
      await claimWorker(operatorToken, workerId, presenceDeviceId());
      await refreshControlRoom(false);
      // Taking the worker moves the geometry claim here, but the terminal keeps
      // whatever size the device you took it from set until something re-fits.
      // Without this, "Work here" appeared to do nothing on a phone.
      const session = workers.find((worker) => worker.id === workerId)?.active_session_id;
      if (session) terminalWorkspace.redrawSession(session);
    }, "Taking this worker…");
  }

  /** Chooses the screen Swarm opens on, for every device. */
  async function chooseStartSurface(next: string) {
    if (!operatorToken) return;
    await perform(async () => setStartSurface(await setStartSurfaceRequest(operatorToken, next)));
  }

  async function restartProviders() {
    if (!operatorToken) return;
    await perform(async () => {
      await restartSupersededWorkers(operatorToken);
      await refreshControlRoom(true);
    }, "Restarting workers on the installed provider release…");
  }

  async function maintainWorkerEngine() {
    if (!operatorToken) return;
    const previousSessionIds = sessions.map((session) => session.session_id);
    setWorkerEngineProgress("Stopping active workers and preserving their conversations…");
    try {
      await perform(async () => {
        await requestRuntimeHandoff(() => updateWorkerEngine(operatorToken).then(() => undefined));
        setBusyLabel("Checking the updated worker engine…");
        setWorkerEngineProgress("The engine is restarting. Swarm is checking its version and reconnecting your crew…");
        let ready = false;
        for (let attempt = 0; attempt < 300; attempt += 1) {
          await new Promise((resolve) => window.setTimeout(resolve, 1_000));
          try {
            const [health, host] = await Promise.all([
              fetchHealth(),
              fetchTerminalHostStatus(operatorToken),
            ]);
            if (workerEngineMatches(health, host) && !host.draining) {
              ready = true;
              break;
            }
          } catch (error) {
            if (!isExpectedRuntimeHandoff(error)) throw error;
          }
        }
        if (!ready) throw new Error("The worker engine update is still not visible after five minutes. Workers remain recoverable; Runtime will keep showing the installed and available versions so it is safe to check again.");
        setWorkerEngineProgress("Worker engine updated. Reconnecting Queen and your always-active workers…");
        previousSessionIds.forEach((sessionId) => terminalWorkspace.closeSession(sessionId));
        const [controlRoom, nextProviders] = await Promise.all([
          loadControlRoom(operatorToken),
          fetchProviderCapabilities(operatorToken),
        ]);
        controlRoomModel.replace(controlRoom);
        setProviders(nextProviders);
        setProviderCapabilitiesUnavailable(false);
        setActiveSessionId(preferredSessionId(controlRoom.workers, controlRoom.sessions));
      }, "Restarting worker engine…");
    } finally {
      setWorkerEngineProgress(undefined);
    }
  }

  async function reloadDevelopmentBuild() {
    if (!operatorToken || loadState.kind !== "ready") return;
    const previousVersion = loadState.health.version;
    // Only asking for the build is a blocking operation.
    //
    // Waiting for it used to be one too, and a build takes minutes: the whole
    // control room sat disabled the entire time, every worker greyed out, with
    // the reason in small text in a corner. Nothing about a rebuild requires
    // that. The API keeps serving the old binary until the new one is ready, so
    // opening a terminal, waking a worker and stopping one all work normally
    // throughout. The only unavailable moment is the restart at the end, which
    // is seconds long and already recovers on its own.
    await perform(
      async () => requestRuntimeHandoff(() => requestDevelopmentReload(operatorToken)),
      "Starting development build…",
    );
    void watchDevelopmentBuild(previousVersion);
  }

  /**
   * Follows a running build to its end without holding the control room still.
   *
   * Progress is already reported by the runtime indicator, which polls on its
   * own; this exists to reload the page the moment the new version answers, and
   * to surface a compile failure rather than leaving the indicator spinning.
   */
  async function watchDevelopmentBuild(previousVersion: string) {
    if (!operatorToken) return;
    for (let attempt = 0; attempt < 600; attempt += 1) {
      await new Promise((resolve) => window.setTimeout(resolve, 2_000));
      let next;
      let development;
      try {
        [next, development] = await Promise.all([
          fetchHealth(),
          fetchDevelopmentRuntime(operatorToken),
        ]);
      } catch (error) {
        if (error instanceof RuntimeRequestError && ![502, 503, 504].includes(error.status)) {
          setOperationError(error instanceof Error ? error.message : "The development build could not be followed");
          return;
        }
        // The API is expected to disappear briefly only after the build succeeds.
        continue;
      }
      if (next.version !== previousVersion) {
        window.location.reload();
        return;
      }
      if (development.state === "failed") {
        setOperationError("The development working copy did not compile. The current release is still running; check the development reload service log for the build error.");
        return;
      }
    }
    setOperationError("The development build did not become healthy within 20 minutes");
  }

  function releaseEngagementWhenSwitching(
    currentSessionId: string | undefined,
    nextSessionId: string,
  ) {
    if (!operatorToken || !currentSessionId || currentSessionId === nextSessionId) return;
    void releaseWorkerEngagement(operatorToken, currentSessionId, presenceDeviceId()).catch(
      (error: unknown) => setOperationError(
        error instanceof Error ? error.message : "The previous worker engagement could not be released",
      ),
    );
  }

  function focusTerminalAfterRender(sessionId: string) {
    const input = shouldFocusTerminalInput();
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => terminalWorkspace.focusSession(sessionId, input));
    });
  }

  async function logout() {
    await perform(async () => {
      await revokeBrowserSession();
      lockInterface();
    });
  }

  function lockInterface() {
    terminalWorkspace.logout();
    setOperatorToken(undefined);
    controlRoomModel.clear();
    setNotificationSettings(undefined);
    setNotificationState("unsupported");
    setActiveSessionId(undefined);
    setOperationError(undefined);
  }

  const activeSession = sessions.find((session) => session.session_id === activeSessionId);
  const activeWorker = workers.find((worker) => worker.active_session_id === activeSessionId);
  const openTaskCount = tasks.filter((task) => task.state !== "completed").length;
  const pendingDecisionCount = decisions.filter((decision) => decision.state === "pending").length;
  const pendingAssistCount = stewardAssists?.incoming?.filter((request) => request.state === "pending").length ?? 0;
  const queenWorkerId = workers.find((worker) => worker.role === "queen")?.id;
  const pendingQueenDecisionCount = decisions.filter((decision) => decision.state === "pending" && decision.requesting_worker_id === queenWorkerId).length;
  // Counted only when it is not already counted as the specific request it
  // refers to, so one thing needing the operator is one item in the queue.
  const queenAutomationAttentionCount =
    queenAutomationNeedsAttention(queenAutomation, pendingQueenDecisionCount > 0)
      && pendingQueenDecisionCount === 0
      ? 1
      : 0;
  // Held work is one card however many deliveries are behind it, the same way
  // unanswered email is.
  //
  // It was in the queue and in neither count, so "Needs you" read 0 with a card
  // plainly on the page. A badge that disagrees with the page teaches the
  // operator to stop believing the badge, which is the one thing it has to do.
  const heldDeliveryAttentionCount = heldDeliveries.length > 0 ? 1 : 0;
  const attentionCount = pendingDecisionCount + pendingAssistCount + queenAutomationAttentionCount
    + heldDeliveryAttentionCount + awaitingReply.length;
  // WHEN THEY ACTUALLY LOOKED. The watermark this advances is the only thing
  // keeping push quiet now that every Needs-you source is eligible, so it is
  // recorded from the surface being open and visible — never from a poll.
  //
  // document.visibilityState matters as much as the surface: a Needs-you tab
  // left open behind a browser window is not someone reading it, and treating
  // it as a look would silence the queue for as long as the tab existed.
  useEffect(() => {
    if (!operatorToken || surface !== "decisions") return undefined;
    const mark = () => {
      if (document.visibilityState === "visible") void recordAttentionSeen(operatorToken);
    };
    mark();
    document.addEventListener("visibilitychange", mark);
    return () => document.removeEventListener("visibilitychange", mark);
    // attentionCount is a dependency on purpose: reading the queue, then having
    // a new item arrive while still looking at it, is still having seen it.
  }, [operatorToken, surface, attentionCount]);
  const orphanSessions = useMemo(
    () => sessions.filter((session) => session.running && !workers.some((worker) => worker.active_session_id === session.session_id)),
    [sessions, workers],
  );
  const visibleWorkers = useMemo(
    () => workerVisibility === "awake" ? workers.filter((worker) => worker.running) : workers,
    [workerVisibility, workers],
  );
  const normalizedWorkerQuery = normalizeRosterQuery(workerQuery);
  const railVisibleWorkers = useMemo(
    () => visibleWorkers.filter((worker) => workerMatchesRosterQuery(worker, normalizedWorkerQuery)),
    [normalizedWorkerQuery, visibleWorkers],
  );
  const railVisibleOrphanSessions = useMemo(
    () => orphanSessions.filter((session) => orphanSessionMatchesRosterQuery(workerName(session.session_id), normalizedWorkerQuery)),
    [normalizedWorkerQuery, orphanSessions],
  );
  const sleepingWorkerMatchesRailQuery = useMemo(
    () => workerVisibility === "awake" && normalizedWorkerQuery
      ? workers.some((worker) => !worker.running && workerMatchesRosterQuery(worker, normalizedWorkerQuery))
      : false,
    [normalizedWorkerQuery, workerVisibility, workers],
  );
  const mobileVisibleWorkers = visibleWorkers;
  const mobileVisibleOrphanSessions = orphanSessions;
  const liveWorkerCount = workers.filter((worker) => worker.running).length
    + orphanSessions.filter((session) => session.running).length;
  const rosterWorkerCount = workers.length + orphanSessions.length;
  const tasksBySession = useMemo(
    () => new Map(
      tasks
        .filter((task) => task.assigned_session_id && task.state !== "completed")
        .map((task) => [task.assigned_session_id, task]),
    ),
    [tasks],
  );
  /// Shows the board as this worker's queue: filtered to that worker and
  /// ordered by state, which is the order the roster and the context chip
  /// already imply. A focused task is pointless behind a filter that hides it,
  /// so both entry points set the filter rather than only the chip's target.
  function openWorkerQueue(workerId: string, focusTaskId?: string) {
    setTaskWorkerFilter(workerId);
    setTaskSort("status");
    if (focusTaskId) {
      setTaskFocus((current) => ({ id: focusTaskId, request: (current?.request ?? 0) + 1 }));
    }
    setSurface("tasks");
  }
  const workByWorker = useMemo(() => {
    const grouped = new Map<string, Task[]>();
    tasks.forEach((task) => {
      if (!task.assigned_worker_id || task.state === "completed") return;
      const assigned = grouped.get(task.assigned_worker_id) ?? [];
      assigned.push(task);
      grouped.set(task.assigned_worker_id, assigned);
    });
    return new Map([...grouped].map(([workerId, assigned]) => [workerId, workerWork(assigned)]));
  }, [tasks]);
  const activeWorkerWork = activeWorker ? workByWorker.get(activeWorker.id) : undefined;
  const activeWorkerEngagement = activeWorker ? foreignEngagement(activeWorker, presenceDeviceId()) : undefined;
  const taskProjects = useMemo(() => [...new Map(jiraTaskLinks.map((link) => [link.project_key, {
    key: link.project_key,
    name: link.project_name,
    url: jiraProjectUrl(link),
  }])).values()].sort((left, right) => left.name.localeCompare(right.name)), [jiraTaskLinks]);
  const openSettings = (section: SettingsSection = "settings-hive") => {
    navigateToSettingsSection(section);
    setSettingsSection(section);
    setSurface("settings");
  };
  const openApiarySettings = () => openSettings("settings-connections");
  const openQueenForAttention = () => {
    const queen = workers.find((worker) => worker.role === "queen");
    if (!queen) return openSettings("settings-workers");
    if (queen.active_session_id) return openWorker(queen.active_session_id);
    void startExistingWorker(queen);
  };
  const commandChoices = useMemo<CommandChoice[]>(() => [
    { id: "decisions", label: "Needs you", detail: `${attentionCount} pending`, group: "Go to", run: () => setSurface("decisions") },
    { id: "tasks", label: "Tasks", detail: `${openTaskCount} open`, group: "Go to", run: () => setSurface("tasks") },
    { id: "new-task", label: "Create task", detail: "Plan work for a worker", group: "Go to", run: () => { setTaskComposeRequest((current) => current + 1); setSurface("tasks"); } },
    { id: "workers", label: "Workers", detail: `${workers.filter((worker) => worker.running).length} running`, group: "Go to", run: () => setSurface("workers") },
    ...(federated ? [{ id: "apiary", label: "Apiary", detail: pendingAssistCount ? `${pendingAssistCount} help offer${pendingAssistCount === 1 ? "" : "s"}` : keeper ? "Keeper overview" : "Membership overview", group: "Go to" as const, run: () => setSurface("apiary") }] : []),
    { id: "add-worker", label: "Add worker", detail: "Configure a repository worker", group: "Go to", run: () => openSettings("settings-workers") },
    { id: "settings", label: "Settings", detail: "Preferences and diagnostics", group: "Go to", run: () => openSettings() },
    // Reachable rather than resident. Locking Swarm while leaving the machine
    // unlocked is a rare thing to want, and it was taking a permanent place in
    // every header on every surface to offer it.
    { id: "lock", label: "Lock Swarm", detail: "Require the operator token again", group: "Go to", run: () => void logout() },
    ...workers.map((worker) => ({
      id: `worker-${worker.id}`,
      label: worker.name,
      detail: worker.running && worker.active_session_id ? "Open worker terminal" : "Wake sleeping worker",
      group: "Workers" as const,
      run: () => {
        if (worker.active_session_id) openWorker(worker.active_session_id);
        else void startExistingWorker(worker);
      },
    })),
    ...tasks.map((task) => ({
      id: `task-${task.id}`,
      label: task.title,
      detail: `${taskStateLabel(task)} · ${workers.find((worker) => worker.id === task.assigned_worker_id)?.name ?? "Unassigned"}`,
      group: "Work" as const,
      run: () => {
        setTaskFocus((current) => ({ id: task.id, request: (current?.request ?? 0) + 1 }));
        setSurface("tasks");
      },
    })),
    ...decisions.map((decision) => ({
      id: `decision-${decision.id}`,
      label: decision.title,
      detail: decision.reason,
      group: "Attention" as const,
      run: () => {
        setDecisionFocus((current) => ({ id: decision.id, request: (current?.request ?? 0) + 1 }));
        setSurface("decisions");
      },
    })),
  ], [openTaskCount, attentionCount, pendingAssistCount, workers, tasks, decisions, activeSessionId, operatorToken, federated, keeper]);

  useEffect(() => {
    window.addEventListener("keydown", handleShortcut);
    return () => window.removeEventListener("keydown", handleShortcut);
  });

  useEffect(() => setPopoutBlocked(false), [surface]);

  useEffect(
    () => measureRoutePaint(
      surface,
      (callback) => requestAnimationFrame(callback),
      (handle) => cancelAnimationFrame(handle),
    ),
    [surface],
  );

  useEffect(() => {
    const workerId = activeWorker?.id;
    if (!operatorToken || !workerId) {
      setRepository(undefined);
      return;
    }
    let current = true;
    setRepository(undefined);
    void fetchWorkerRepository(operatorToken, workerId)
      .then((state) => { if (current) setRepository(state); })
      .catch(() => { if (current) setRepository(null); });
    return () => { current = false; };
  }, [operatorToken, activeWorker?.id]);

  /**
   * The count in the browser tab, which is the only part of this that reaches
   * an operator not looking at the page.
   *
   * A real browser notification cannot serve the case they reported: push is
   * deliberately suppressed while they are AT the Hive
   * (enqueue_decision_notifications returns early on PresenceMode::AtHive), on
   * the reasonable assumption that someone here can see the queue. Their
   * complaint is that they are here and cannot — "I sometimes don't realize I
   * have something pending which is slowing the whole system down". A tab title
   * survives the tab being backgrounded, needs no permission, and cannot be
   * revoked, which makes it the right instrument for exactly this gap.
   */
  useEffect(() => {
    const base = "Swarm";
    document.title = attentionCount > 0 ? `(${attentionCount}) ${base}` : base;
  }, [attentionCount]);

  useEffect(() => {
    if (surface !== "workers" || !activeSessionId) return;
    const frame = requestAnimationFrame(() => {
      terminalWorkspace.focusSession(activeSessionId, shouldFocusTerminalInput());
    });
    return () => cancelAnimationFrame(frame);
  }, [surface, activeSessionId]);

  return (
    <main className={`app-shell${workerRail.resizing ? " resizing-rail" : ""}${detached ? " detached-surface" : ""}`} style={{ "--rail-width": `${workerRail.width}px` } as CSSProperties}>
      {/* A detached window keeps the controls that belong to the surface it is
          showing — the worker picker, the filters, the settings sections — and
          drops the navigation between surfaces, which is what it was detached
          from. Without this a popped-out worker panel could only ever show the
          one worker it opened with. */}
      <aside className={`control-rail surface-${surface}${detached ? " detached-rail" : ""}`} aria-label={detached ? `${surfaceLabel(surface)} controls` : "Swarm navigation"}>
        {detached ? null : <div className="brand-lockup">
          <div className="brand-mark"><BeeMascot expression="available" /></div>
          <div className="brand-copy"><p className="eyebrow">Swarm</p><h1>Control room</h1><HiveContextIndicator identity={hiveIdentity} /></div>
          {operatorToken ? (
            <button
              type="button"
              className="icon-button brand-diagnostics"
              aria-label="Open diagnostics"
              title="Browser, API, database, terminal, provider and integration health"
              onClick={() => openSettings("settings-maintenance")}
            ><DiagnosticsIcon /></button>
          ) : null}
        </div>}

        {operatorToken ? (
          <>
            {detached ? null : <nav className={`surface-nav${federated ? " with-apiary" : ""}`} aria-label="Primary">
              <span className="surface-nav-item"><button className={surface === "decisions" ? "selected" : ""} aria-current={surface === "decisions" ? "page" : undefined} data-detached={surfaceIsDetached("decisions") || undefined} onClick={() => showSurface("decisions")}>
                {/* data-waiting is what makes a queue holding work look
                    different from an empty one. The operator: "I sometimes
                    don't realize I have something pending which is slowing the
                    whole system down." Every item here is by construction
                    something only they can clear, so a queue that does not
                    announce itself stalls the fleet at its one irreplaceable
                    participant. */}
                <span><DecisionIcon /> Needs you</span>
                <small data-waiting={attentionCount > 0 || undefined}>
                  {attentionCount}
                  {/* Not colour alone — WCAG 2.1 AA. The count is already a
                      number; this gives the state a second, non-colour channel
                      for anyone who cannot rely on the first. */}
                  {attentionCount > 0 ? <span className="visually-hidden"> waiting for you</span> : null}
                </small>
              </button>{operatorToken && !detached ? <button type="button" className="surface-nav-popout" aria-label={`Open ${surfaceLabel("decisions")} in a new window`} title={`Open ${surfaceLabel("decisions")} in a new window. Workers keep running; a window only views them.`} onClick={() => setPopoutBlocked(!openSurfaceWindow("decisions", (url, name, features) => window.open(url, name, features)))}><PopoutIcon /></button> : null}</span>
              <span className="surface-nav-item"><button className={surface === "tasks" ? "selected" : ""} aria-current={surface === "tasks" ? "page" : undefined} data-detached={surfaceIsDetached("tasks") || undefined} onClick={() => showSurface("tasks")}>
                <span><TaskIcon /> Tasks</span><small>{openTaskCount}</small>
              </button>{operatorToken && !detached ? <button type="button" className="surface-nav-popout" aria-label={`Open ${surfaceLabel("tasks")} in a new window`} title={`Open ${surfaceLabel("tasks")} in a new window. Workers keep running; a window only views them.`} onClick={() => setPopoutBlocked(!openSurfaceWindow("tasks", (url, name, features) => window.open(url, name, features)))}><PopoutIcon /></button> : null}</span>
              <span className="surface-nav-item"><button className={surface === "workers" ? "selected" : ""} aria-current={surface === "workers" ? "page" : undefined} data-detached={surfaceIsDetached("workers") || undefined} aria-label={`Workers, ${liveWorkerCount} active of ${rosterWorkerCount}`} onClick={() => showSurface("workers")}>
                <span><TerminalIcon /> Workers</span><small>{liveWorkerCount}/{rosterWorkerCount}</small>
              </button>{operatorToken && !detached ? <button type="button" className="surface-nav-popout" aria-label={`Open ${surfaceLabel("workers")} in a new window`} title={`Open ${surfaceLabel("workers")} in a new window. Workers keep running; a window only views them.`} onClick={() => setPopoutBlocked(!openSurfaceWindow("workers", (url, name, features) => window.open(url, name, features)))}><PopoutIcon /></button> : null}</span>
              {federated ? <button className={`apiary-nav-button${surface === "apiary" ? " selected" : ""}`} aria-current={surface === "apiary" ? "page" : undefined} data-detached={surfaceIsDetached("apiary") || undefined} onClick={() => showSurface("apiary")}><span><ApiaryIcon /> Apiary</span>{pendingAssistCount ? <small aria-label={`${pendingAssistCount} pending help offer${pendingAssistCount === 1 ? "" : "s"}`}>{pendingAssistCount}</small> : null}</button> : null}
              <button className={surface === "settings" ? "selected" : ""} aria-current={surface === "settings" ? "page" : undefined} onClick={() => openSettings()}>
                <span><SettingsIcon /> Settings</span>
              </button>

              <PublicAddressWarning
                status={publicAddress}
                onOpen={() => openSettings("settings-access")}
                onStop={async () => {
                  if (!operatorToken) return;
                  setPublicAddress(await stopTunnel(operatorToken));
                }}
              />
            </nav>}

            {/* Settings navigates from the rail like every other surface. It
                was the only one carrying its own bar of sections above the
                content, so it read as a different kind of screen. */}
            {surface === "settings" && (
              <div className="rail-context">
                <div className="rail-settings-search">
                  <label className="sr-only" htmlFor="settings-search">Find a setting</label>
                  <input
                    id="settings-search"
                    type="search"
                    placeholder="Find a setting…"
                    autoComplete="off"
                    value={settingsQuery}
                    onChange={(event) => setSettingsQuery(event.target.value)}
                    onKeyDown={(event) => { if (event.key === "Escape" && settingsQuery) { event.stopPropagation(); setSettingsQuery(""); } }}
                  />
                  {settingsQuery ? (
                    <button type="button" className="worker-search-clear" aria-label="Clear settings search" onClick={() => setSettingsQuery("")}>×</button>
                  ) : null}
                </div>
                <nav className="rail-settings-sections" aria-label="Settings sections">
                  {SETTINGS_SECTIONS.map(([id, label]) => (
                    <button
                      key={id}
                      type="button"
                      className={settingsSection === id ? "selected" : ""}
                      aria-current={settingsSection === id ? "location" : undefined}
                      onClick={() => { setSettingsQuery(""); openSettings(id); }}
                    >{label}</button>
                  ))}
                </nav>
              </div>
            )}
            {(surface === "tasks" || surface === "workers") && <div className="rail-context">
              <div className="rail-controls">
                <div className="rail-heading">
                  <span>{surface === "tasks" ? "Board view" : "Workers"}</span>
                  {surface === "workers" ? (
                    <div className="worker-visibility-toggle" role="group" aria-label="Workers shown">
                      <button type="button" aria-pressed={workerVisibility === "all"} onClick={() => changeWorkerVisibility("all")}>All</button>
                      <button type="button" aria-pressed={workerVisibility === "awake"} onClick={() => changeWorkerVisibility("awake")}>Awake</button>
                    </div>
                  ) : null}
                </div>
                {surface === "workers" && workers.length > 0 ? (
                  <div className="worker-search">
                    <label className="sr-only" htmlFor="worker-search">Find a worker by name, repository, or path</label>
                    <input
                      id="worker-search"
                      type="search"
                      placeholder="Find a worker…"
                      autoComplete="off"
                      value={workerQuery}
                      onChange={(event) => setWorkerQuery(event.target.value)}
                      onKeyDown={(event) => { if (event.key === "Escape" && workerQuery) { event.stopPropagation(); setWorkerQuery(""); } }}
                    />
                    {workerQuery ? (
                      <button type="button" className="worker-search-clear" aria-label="Clear worker search" onClick={() => setWorkerQuery("")}>×</button>
                    ) : null}
                  </div>
                ) : null}
                {surface === "tasks" ? (
                  <TaskBoardControls query={taskQuery} filter={taskFilter} source={taskSource} sort={taskSort} project={taskProject} worker={taskWorker} workers={workers} projects={taskProjects} openCount={openTaskCount} busy={busy} onQueryChange={setTaskQuery} onFilterChange={setTaskFilter} onSourceChange={(value) => { setTaskSource(value); if (value === "email" || value === "local") setTaskProject("all"); }} onSortChange={setTaskSort} onProjectChange={setTaskProject} onWorkerChange={setTaskWorkerFilter} onSync={() => void syncJiraBoard()} />
                ) : null}
              </div>
              {surface === "tasks" ? null : workers.length === 0 && orphanSessions.length === 0 ? (
                <p className="empty-rail">No workers configured.</p>
              ) : visibleWorkers.length === 0 && orphanSessions.length === 0 ? (
                <p className="empty-rail">All {workers.length} workers are sleeping. Choose All to wake one.</p>
              ) : railVisibleWorkers.length === 0 && railVisibleOrphanSessions.length === 0 ? (
                <p className="empty-rail">
                  <strong>No worker matches “{workerQuery.trim()}”</strong>
                  <span>Try a worker name, repository name, or path.</span>
                  {sleepingWorkerMatchesRailQuery ? (
                    <button type="button" className="secondary-button" onClick={() => changeWorkerVisibility("all")}>Show sleeping workers</button>
                  ) : null}
                </p>
              ) : (
                <div className="worker-list">
                  {railVisibleWorkers.map((worker) => {
                    const sessionId = worker.active_session_id;
                    const work = workByWorker.get(worker.id);
                    const task = work?.current ?? (sessionId ? tasksBySession.get(sessionId) : undefined);
                    return (
                      <WorkerRosterItem
                        key={worker.id}
                        worker={worker}
                        selected={sessionId === activeSessionId}
                        detail={worker.runtime_error ?? task?.title ?? (worker.role === "queen" ? "Always-active command terminal" : worker.running ? `${repositoryName(worker.workspace)} · Ready for work` : `${repositoryName(worker.workspace)} · Sleeping`)}
                        workSummary={work?.summary}
                        busy={busy}
                        busyReason={busyLabel}
                        onOpen={() => sessionId && openWorker(sessionId)}
                        onStart={() => void startExistingWorker(worker)}
                        onStop={() => sessionId && void stopSession(sessionId)}
                      />
                    );
                  })}
                  {railVisibleOrphanSessions.map((session) => {
                    const task = tasksBySession.get(session.session_id);
                    return (
                      <button className="worker-button" aria-current={session.session_id === activeSessionId ? "page" : undefined} key={session.session_id} onClick={() => openWorker(session.session_id)}>
                        <span className="worker-avatar"><BeeMascot expression={session.running ? "focused" : "sleeping"} /></span>
                        <span className="worker-copy">
                          <strong>{workerName(session.session_id)}</strong>
                          <small>{task?.title ?? "Pre-roster session"}</small>
                        </span>
                        <span className={`presence ${session.running ? "online" : "offline"}`} title={session.running ? "Running" : "Exited"} />
                      </button>
                    );
                  })}
                </div>
              )}
            </div>}

            {surface === "workers" && (
              <div className="start-worker-disclosure">
                <button type="button" onClick={() => openSettings("settings-workers")}>Manage workers</button>
              </div>
            )}
          </>
        ) : <p className="empty-rail">Unlock this runtime to access tasks and workers.</p>}

        {/* Beside the runtime line rather than in the lockup: this is what the
            version it sits next to is about, and the lockup is a row the
            operator reaches for often enough that a rarely-used control there
            is mostly a misclick risk. */}
        {detached ? null : <div className="rail-footer">
          <RuntimeStatus state={loadState} developmentMode={developmentMode} />
          {/* One per subsystem rather than only the most severe: they are
              independent and can all be true at once, and ranking them into a
              single pill hid the others until the first was dealt with.
              Each states what it is and what it will do, and offers to do it —
              a title attribute said none of that on a phone, and sending the
              operator to Settings to press a button they can see from here is
              a trip with nothing in it. */}
          {operatorToken ? runtimeUpdates.map((update) => (
            <div key={update.kind} className={`runtime-update-card runtime-update-${update.kind}${update.consequence ? " costly" : ""}`}>
              <p className="runtime-update-label">
                {update.busy ? <span className="runtime-update-spinner" aria-hidden="true" /> : null}
                {update.label}
              </p>
              <p className="runtime-update-detail">{update.detail}</p>
              {update.action && !update.busy ? (
                <button
                  type="button"
                  className="runtime-update-run"
                  onClick={() => setRuntimeConfirm(update)}
                  disabled={busy}
                >{update.actionLabel}</button>
              ) : null}
              <button type="button" className="runtime-update-settings" onClick={() => openSettings("settings-updates")}>
                Details
              </button>
            </div>
          )) : null}
        </div>}
        <div className="rail-resize-handle" role="separator" aria-label="Resize worker area" aria-orientation="vertical" aria-valuemin={220} aria-valuemax={480} aria-valuenow={workerRail.width} tabIndex={0} onPointerDown={workerRail.start} onPointerMove={workerRail.move} onPointerUp={workerRail.finish} onPointerCancel={workerRail.finish} onKeyDown={workerRail.resizeWithKeyboard} />
      </aside>

      {/* Replacing the paint boundary on a workspace change prevents Chromium
          from retaining xterm's detached accelerated layer over the next view. */}
      <section className="workspace" key={surface}>
        <header className="workspace-header">
          <div>
            <p className="eyebrow">{surface === "decisions" ? "Attention without interruption" : surface === "tasks" ? "Plan and dispatch" : surface === "apiary" ? "Organization without noise" : surface === "settings" ? "Preferences and diagnostics" : "Persistent terminal"}</p>
            {/* Detaching lives in the rail beside each surface's own entry, on
                the operator's ruling. The heading names one surface; the rail
                lists them all, which is where a per-surface action belongs. */}
            <div className="workspace-heading">
              <h2>{surface === "decisions" ? "Needs you" : surface === "tasks" ? "Task board" : surface === "apiary" ? keeper ? "Keeper" : "Member Hive" : surface === "settings" ? "Settings" : activeSession ? activeWorker?.name ?? workerName(activeSession.session_id) : "Worker terminal"}</h2>

            </div>
            <HiveContextIndicator identity={hiveIdentity} compact />
          </div>
          {surface === "workers" && operatorToken ? (
            <button className="mobile-worker-switcher-trigger" type="button" aria-haspopup="dialog" aria-label={`Switch worker, current ${activeWorker?.name ?? (activeSession ? workerName(activeSession.session_id) : "none")}${activeWorkerWork?.current ? `, carrying ${activeWorkerWork.current.title}` : ""}`} onClick={() => setShowMobileWorkers(true)}>
              <span className="worker-avatar"><BeeMascot expression={activeWorker ? workerExpression(activeWorker) : "sleeping"} /></span>
              {/* On a phone this trigger replaces the header's worker name and
                  Hive line entirely, and the context bar's task chip is hidden
                  because a row of chips would return the vertical space the
                  phone layout reclaimed. So the task takes the small line the
                  Hive indicator was using rather than adding one: on the worker
                  surface, what the worker is carrying is the thing the phone
                  could not see at all, and the Hive line is on every other
                  surface's header. */}
              <span>
                {activeWorkerWork?.current
                  ? <span className="mobile-worker-task">{activeWorkerWork.current.title}</span>
                  : <HiveContextIndicator identity={hiveIdentity} compact />}
                <strong>{activeWorker?.name ?? (activeSession ? workerName(activeSession.session_id) : "Choose worker")}</strong>
              </span>
              <span aria-hidden="true">⌄</span>
            </button>
          ) : null}
          {surface === "workers" && activeWorker && activeWorkerWork?.current ? (
            <WorkerContextBar
              worker={activeWorker}
              currentTask={activeWorkerWork.current}
              openCount={activeWorkerWork.openCount}
              workSummary={activeWorkerWork.summary}
              repository={repository}
              engagement={activeWorkerEngagement}
              unconfirmedDelivery={activeWorker.unconfirmed_delivery}
              onClaim={() => void claimActiveWorker(activeWorker.id)}
              taskStateLabel={taskStateLabel}
              onOpenQueue={openWorkerQueue}
            />
          ) : null}
          <div className="header-actions">
            {busy && <span className="saving-state">{busyLabel ?? "Saving…"}</span>}
            {surface === "workers" && activeSession && terminalConnection ? (
              <span
                className={`terminal-connection-dot connection-${terminalConnection}`}
                role="status"
                aria-label={`Terminal ${terminalConnection.replace("_", " ")}`}
                title={`Terminal ${terminalConnection.replace("_", " ")}`}
              />
            ) : null}
            {/* The word is dropped on a phone to give the selector its room back, so the
                state has to be carried by the label rather than by the text. */}
            {operatorToken && presence && <span className={`operator-presence-chip ${presence.mode}`} role="status" aria-label={`Operator presence: ${presenceModeLabel(presence.mode)}`} title={`Operator presence: ${presenceModeLabel(presence.mode)}`}><span className="state-dot" /><span>{presenceModeLabel(presence.mode)}</span></span>}
            {popoutBlocked && <span className="saving-state" role="alert">Your browser blocked the new window</span>}
            {operatorToken && <button className="icon-button feedback-button" aria-label="Report a problem" onClick={() => setShowFeedback(true)}><FeedbackIcon /></button>}
            {operatorToken && <button className="icon-button command-button" aria-label="Open quick navigation" onClick={() => setShowCommands(true)}><CommandIcon /></button>}
            <button className="icon-button theme-button" aria-label={`Switch to ${colorTheme === "light" ? "dark" : "light"} theme`} onClick={() => changeColorTheme(colorTheme === "light" ? "dark" : "light")}><ThemeIcon theme={colorTheme} /></button>
            {operatorToken && <button className="icon-button refresh-button" aria-label="Refresh control room" title="Refresh data and rebuild the visible terminal" onClick={() => void refreshControlRoom(true)} disabled={busy}><RefreshIcon /></button>}

          </div>
        </header>
        {operationError && <div className="operation-error" role="alert">{operationError}</div>}
        {showMobileWorkers ? (
          <div className="mobile-worker-backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget) setShowMobileWorkers(false); }}>
            <section ref={mobileWorkerDialog} tabIndex={-1} className="mobile-worker-dialog" role="dialog" aria-modal="true" aria-labelledby="mobile-worker-heading">
              <div className="mobile-worker-dialog-heading">
                <div><p className="eyebrow">Worker switcher</p><h3 id="mobile-worker-heading">Where do you want to work?</h3></div>
                <button type="button" onClick={() => setShowMobileWorkers(false)}>Close</button>
              </div>
              <div className="mobile-worker-dialog-toolbar">
                <p className="mobile-worker-dialog-summary">{liveWorkerCount} awake · {rosterWorkerCount - liveWorkerCount} sleeping</p>
                <div className="worker-visibility-toggle" role="group" aria-label="Workers shown">
                  <button type="button" aria-pressed={workerVisibility === "all"} onClick={() => changeWorkerVisibility("all")}>All</button>
                  <button type="button" aria-pressed={workerVisibility === "awake"} onClick={() => changeWorkerVisibility("awake")}>Awake</button>
                </div>
              </div>
              <div className="mobile-worker-dialog-list">
                {mobileVisibleWorkers.length === 0 && mobileVisibleOrphanSessions.length === 0 ? (
                  workers.length === 0 && orphanSessions.length === 0 ? (
                    <div className="mobile-worker-empty"><strong>No workers configured yet</strong><span>Add a repository worker in Settings, then return here to wake her.</span><button type="button" onClick={() => { setShowMobileWorkers(false); openSettings("settings-workers"); }}>Manage workers</button></div>
                  ) : (
                    <div className="mobile-worker-empty"><strong>All {workers.length} workers are sleeping</strong><span>Show the full roster to wake one.</span><button type="button" onClick={() => changeWorkerVisibility("all")}>Show all workers</button></div>
                  )
                ) : null}
                {mobileVisibleWorkers.map((worker) => {
                  const sessionId = worker.active_session_id;
                  const work = workByWorker.get(worker.id);
                  const assignedTask = work?.current ?? (sessionId ? tasksBySession.get(sessionId) : undefined);
                  return (
                    <button
                      type="button"
                      className="mobile-worker-choice"
                      aria-current={sessionId === activeSessionId ? "page" : undefined}
                      key={worker.id}
                      disabled={busy}
                      onClick={() => {
                        if (sessionId) openWorker(sessionId);
                        else void startExistingWorker(worker);
                        setShowMobileWorkers(false);
                      }}
                    >
                      <span className="worker-avatar"><BeeMascot expression={workerExpression(worker)} /></span>
                      <span className="mobile-worker-choice-copy">
                        <span><strong>{worker.name}</strong><small>{worker.role === "queen" ? "Queen" : repositoryName(worker.workspace)}</small></span>
                        <small>{worker.runtime_error ?? workerSwitcherDetail(worker, assignedTask?.title)}</small>
                        {work?.summary ? <span className="worker-work-summary" title={`${worker.name}'s open work: ${work.summary}`}>Open work · {work.summary}</span> : null}
                      </span>
                      <span className={`presence ${worker.running ? "online" : "offline"}`} aria-label={worker.running ? workerAttentionLabel(worker) : "Sleeping"} />
                    </button>
                  );
                })}
                {mobileVisibleOrphanSessions.map((session) => (
                  <button type="button" className="mobile-worker-choice" aria-current={session.session_id === activeSessionId ? "page" : undefined} key={session.session_id} onClick={() => { openWorker(session.session_id); setShowMobileWorkers(false); }}>
                    <span className="worker-avatar"><BeeMascot expression={session.running ? "focused" : "sleeping"} /></span>
                    <span className="mobile-worker-choice-copy"><span><strong>{workerName(session.session_id)}</strong><small>Unconfigured session</small></span><small>{tasksBySession.get(session.session_id)?.title ?? "Pre-roster session"}</small></span>
                    <span className={`presence ${session.running ? "online" : "offline"}`} aria-label={session.running ? "Running" : "Exited"} />
                  </button>
                ))}
              </div>
              <button className="secondary-button mobile-manage-workers" type="button" onClick={() => { setShowMobileWorkers(false); openSettings("settings-workers"); }}>Manage workers</button>
            </section>
          </div>
        ) : null}
        {operatorToken && runtimeConfirm ? (
          <RuntimeUpdateConfirm
            update={runtimeConfirm}
            busy={busy}
            onConfirm={() => void runRuntimeUpdate(runtimeConfirm)}
            onCancel={() => setRuntimeConfirm(undefined)}
          />
        ) : null}
        {operatorToken && showCommands ? <CommandPalette choices={commandChoices} onClose={() => setShowCommands(false)} /> : null}
        {operatorToken && showFeedback ? (
          <DogfoodFeedbackDialog
            activeSessionId={activeSessionId}
            health={loadState.kind === "ready" ? loadState.health : undefined}
            hiveIdentity={hiveIdentity}
            liveFeedState={liveFeedState}
            onClose={() => setShowFeedback(false)}
            onSaved={() => setFeedbackRevision((current) => current + 1)}
            operatorToken={operatorToken}
            recentEvents={recentEvents}
            sessions={sessions}
            surface={surface}
            workers={workers}
          />
        ) : null}
        {!operatorToken ? (
          <form className="unlock-panel" onSubmit={(event) => void authenticate(event)}>
            <div className="unlock-symbol"><BeeMascot expression="available" /></div>
            <p className="eyebrow">Private local runtime</p>
            <h3>Welcome back</h3>
            <p>Unlock this trusted device once. Swarm keeps the credential out of browser storage and terminal access uses one-time grants.</p>
            <label htmlFor="operator-token">Operator token</label>
            <input id="operator-token" type="password" autoComplete="off" value={tokenDraft} onChange={(event) => setTokenDraft(event.target.value)} />
            <button disabled={busy || !tokenDraft}>Unlock Swarm</button>
            {passkeysSupported() && (
              <button
                type="button"
                className="secondary-button unlock-passkey"
                disabled={busy}
                onClick={() => void perform(async () => {
                  await signInWithPasskey();
                  const controlRoom = await loadControlRoom(BROWSER_SESSION_AUTH);
                  terminalWorkspace.authenticate(BROWSER_SESSION_AUTH);
                  setOperatorToken(BROWSER_SESSION_AUTH);
                  controlRoomModel.replace(controlRoom);
                  setActiveSessionId((current) => current ?? preferredSessionId(controlRoom.workers, controlRoom.sessions));
                })}
              >
                Use a passkey
              </button>
            )}
          </form>
        ) : surface === "decisions" ? (
          <div className="attention-workspace">
            <DecisionInbox
              decisions={decisions}
              tasks={tasks}
              workers={workers}
              busy={busy}
              focusDecisionId={decisionFocus?.id}
              focusRequest={decisionFocus?.request}
              additionalPendingCount={pendingAssistCount + queenAutomationAttentionCount + heldDeliveryAttentionCount + awaitingReply.length}
              attentionCards={<>
                <UnansweredEmailAttentionCard awaiting={awaitingReply} busy={busy} onSendReply={sendAwaitingReply} onSaveReply={saveAwaitingReply} onReviseReply={reviseAwaitingReply} onOpenTask={(taskId) => { setTaskFocus((current) => ({ id: taskId, request: (current?.request ?? 0) + 1 })); setSurface("tasks"); }} />
                <QueenAutomationAttentionCard status={queenAutomation} queenRequestPending={pendingQueenDecisionCount > 0} coveredBySpecificDecision={pendingQueenDecisionCount > 0} onOpenQueen={openQueenForAttention} onReviewSettings={() => openSettings("settings-workers")} onRetry={resumeQueenReview} />
                <ApiaryAttentionCard pendingAssistance={pendingAssistCount} onReview={() => setSurface("apiary")} />
                <HeldDeliveryAttentionCard held={heldDeliveries} onOpenWorker={(name) => { const worker = workers.find((candidate) => candidate.name === name); if (worker) openWorker(worker.id); }} />
              </>}
              onOpenTask={(taskId) => { setTaskFocus((current) => ({ id: taskId, request: (current?.request ?? 0) + 1 })); setSurface("tasks"); }}
              onFetchActivity={() => fetchRecentTaskActivity(operatorToken)}
              onResolve={resolveInboxDecision}
              onAnswer={answerInboxDecision}
            />
          </div>
        ) : surface === "tasks" ? (
          <Suspense fallback={<WorkspaceLoading label="task board" />}>
            <TaskBoard tasks={tasks} jiraTaskLinks={jiraTaskLinks} operatorToken={operatorToken} hiveIdentity={hiveIdentity} focusTaskId={taskFocus?.id} focusRequest={taskFocus?.request} composeRequest={taskComposeRequest} sessions={sessions} workers={workers} busy={busy} query={taskQuery} filter={taskFilter} source={taskSource} sort={taskSort} project={taskProject} worker={taskWorker} projects={taskProjects} onQueryChange={setTaskQuery} onFilterChange={setTaskFilter} onSourceChange={(value) => { setTaskSource(value); if (value === "email" || value === "local") setTaskProject("all"); }} onSortChange={setTaskSort} onProjectChange={setTaskProject} onWorkerChange={setTaskWorkerFilter} onJiraSync={() => void syncJiraBoard()} onCreate={addTask} onUpdate={editTask} onRemove={removeTaskFromHive} onRestore={restoreTaskToHive} onTransition={moveTask} onAssign={setTaskWorker} onStartWorker={startWorkerForTask} onOpenWorker={openWorker} onOpenTask={(taskId) => { setTaskFocus((current) => ({ id: taskId, request: (current?.request ?? 0) + 1 })); void refreshControlRoom(); }} onFetchActivity={(taskId) => fetchTaskActivity(operatorToken, taskId)} onFetchJiraComments={(taskId) => fetchJiraComments(operatorToken, taskId)} onAddJiraComment={(taskId, body) => addJiraComment(operatorToken, taskId, body)} onRetryJira={retryTaskJira} onJiraImported={refreshControlRoom} onEmailImported={refreshControlRoom} onReorder={reorderOpenTasks} />
          </Suspense>
        ) : surface === "apiary" && keeper && hiveIdentity ? (
          <Suspense fallback={<WorkspaceLoading label="Apiary" />}>
            <KeeperControlRoom identity={hiveIdentity} operatorToken={operatorToken} onManage={openApiarySettings} onOpenTasks={() => { setTaskComposeRequest((current) => current + 1); setSurface("tasks"); }} />
          </Suspense>
        ) : surface === "apiary" && federated && hiveIdentity ? (
          <Suspense fallback={<WorkspaceLoading label="Apiary" />}>
            <MemberControlRoom
              identity={hiveIdentity}
              operatorToken={operatorToken}
              onManage={openApiarySettings}
              onOpenTasks={() => setSurface("tasks")}
            />
          </Suspense>
        ) : surface === "settings" ? (
          <Suspense fallback={<WorkspaceLoading label="Settings" />}>
            <SettingsWorkspace
              section={settingsSection}
              query={settingsQuery}
              busy={busy}
              workerEngineProgress={workerEngineProgress}
              colorTheme={colorTheme}
              feedbackRevision={feedbackRevision}
              hiveIdentity={hiveIdentity}
              liveFeedState={liveFeedState}
              health={loadState.kind === "ready" ? loadState.health : undefined}
              operatorToken={operatorToken}
              publicAddress={publicAddress}
              onPublicAddressChange={setPublicAddress}
              recentEvents={recentEvents}
              presence={presence}
              lockDetectionState={lockDetectionState}
              notificationSettings={notificationSettings}
              queenPolicy={queenPolicy}
              pendingQueenDecisionCount={pendingQueenDecisionCount}
              providers={providers}
              providerCapabilitiesUnavailable={providerCapabilitiesUnavailable}
              notificationState={notificationState}
              sessions={sessions}
              workers={workers}
              workspaces={workspaces}
              onThemeChange={changeColorTheme}
              onPresenceChange={changePresenceMode}
              startSurface={startSurface}
              onStartSurfaceChange={(next) => void chooseStartSurface(next)}
              onLock={() => void logout()}
              onEnableLockDetection={enableLockDetection}
              onNotificationPolicyChange={changeNotificationPolicy}
              onQueenPolicyChange={changeQueenPolicy}
              onOpenQueenDecisions={() => setSurface("decisions")}
              onOpenTasks={() => setSurface("tasks")}
              onEnableNotifications={enableNotifications}
              onDisableNotifications={disableNotifications}
              onTestNotification={testNotification}
              onCreateWorker={configureWorker}
              onUpdateWorker={maintainWorkerProfile}
              onRemoveWorker={removeWorkerProfile}
              onReorderWorkers={reorderWorkerProfiles}
              onRestartProviders={restartProviders}
              onUpdateWorkerEngine={maintainWorkerEngine}
              onReloadDevelopment={reloadDevelopmentBuild}
              onHiveIdentityChange={setHiveIdentity}
            />
          </Suspense>
        ) : activeSession ? (
          <TerminalLoadBoundary key={`${operatorToken}:${activeSession.session_id}:${terminalRevision}`}>
            <Suspense fallback={<div className="terminal-empty">Preparing terminal…</div>}>
              <TerminalView operatorToken={operatorToken} session={activeSession} busy={busy} canStop={activeWorker?.role !== "queen"} mobileKeysVisible={mobileKeysVisible} onMobileKeysVisibleChange={changeMobileKeysVisibility} onRedraw={() => void refreshControlRoom(true)} queenAutomation={activeWorker?.role === "queen" ? queenAutomation : undefined} queenAutonomy={activeWorker?.role === "queen" ? queenPolicy?.[presence?.mode ?? "at_hive"] : undefined} onOpenQueenSettings={activeWorker?.role === "queen" ? () => openSettings("settings-workers") : undefined} onConnectionStateChange={setTerminalConnection} />
            </Suspense>
          </TerminalLoadBoundary>
        ) : (
          <div className="terminal-empty"><BeeMascot className="empty-bee" expression="sleeping" /><p className="eyebrow">No active session</p><h3>Start with a task or workspace</h3><p>Launch Claude from a ready task to preserve its assignment, or start an unassigned worker from the sidebar.</p></div>
        )}
      </section>
    </main>
  );
}

function WorkspaceLoading({ label }: { label: string }) {
  return <div className="workspace-loading" role="status"><BeeMascot expression="available" /><span>Opening {label}…</span></div>;
}

function preferredSessionId(workers: Worker[], sessions: SessionSummary[]): string | undefined {
  return workers.find((worker) => worker.role === "queen" && worker.running)?.active_session_id
    ?? workers.find((worker) => worker.running)?.active_session_id
    ?? sessions.find((session) => session.running)?.session_id;
}

function restoredSessionId(workers: Worker[], sessions: SessionSummary[]): string | undefined {
  try {
    const saved = window.localStorage.getItem(ACTIVE_SESSION_STORAGE_KEY);
    if (saved && sessions.some((session) => session.running && session.session_id === saved)) {
      return saved;
    }
  } catch {
    // Selection persistence is a non-critical convenience.
  }
  return preferredSessionId(workers, sessions);
}

function saveActiveSessionId(sessionId: string) {
  try {
    window.localStorage.setItem(ACTIVE_SESSION_STORAGE_KEY, sessionId);
  } catch {
    // Selection persistence is a non-critical convenience.
  }
}

function readWorkerVisibility(): WorkerVisibility {
  try {
    return window.localStorage.getItem(WORKER_VISIBILITY_STORAGE_KEY) === "awake" ? "awake" : "all";
  } catch {
    return "all";
  }
}

function rememberWorkerVisibility(visibility: WorkerVisibility) {
  try {
    window.localStorage.setItem(WORKER_VISIBILITY_STORAGE_KEY, visibility);
  } catch {
    // Worker filtering is a non-critical presentation preference.
  }
}

function DecisionIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a7 7 0 0 0-7 7v3l-2 3h18l-2-3v-3a7 7 0 0 0-7-7ZM9 20h6"/></svg>; }
function FeedbackIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5h16v11H9l-5 4V5Z"/><path d="M12 8v4M12 14h.01"/></svg>; }
function CommandIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6"/><path d="m16 16 4 4M8 11h6M11 8v6"/></svg>; }
function requireActiveSession(worker: Worker): string {
  if (!worker.active_session_id) throw new Error(`${worker.name} did not receive a terminal session`);
  return worker.active_session_id;
}

function jiraProjectUrl(link: JiraTaskLink): string | undefined {
  if (!link.issue_url) return undefined;
  try {
    const url = new URL(link.issue_url);
    url.pathname = "/issues/";
    url.search = "";
    url.searchParams.set("jql", `project = "${link.project_key}"`);
    return url.toString();
  } catch {
    return undefined;
  }
}

function taskStateLabel(task: Task): string {
  if (task.state === "ready" && task.assigned_worker_id) return "Assigned";
  if (task.state === "active") return "In progress";
  return task.state[0].toUpperCase() + task.state.slice(1);
}

function workerAttentionLabel(worker: Worker): string {
  return workerAttention(worker).label;
}

function workerExpression(worker: Worker) {
  return workerAttention(worker).expression;
}

function presenceModeLabel(mode: PresenceMode) {
  if (mode === "at_hive") return "At Hive";
  if (mode === "night_watch") return "Night Watch";
  return "Away";
}

function RuntimeStatus({ state, developmentMode }: { state: LoadState; developmentMode?: boolean }) {
  // Dev mode changes what every line beneath this means — updates come from a
  // working copy rather than a release — so it is said here rather than found
  // in Settings.
  const mode = developmentMode
    ? <span className="runtime-mode development" title="This Hive builds and releases from its working copy">Dev</span>
    : null;
  if (state.kind === "ready") return <span className="runtime-status"><span className="presence online" /> Runtime {state.health.version}{mode}</span>;
  if (state.kind === "unavailable") return <span className="runtime-status error"><span className="presence offline" /> Runtime unavailable</span>;
  return <span className="runtime-status"><span className="presence" /> Connecting…</span>;
}

function TaskIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01" /></svg>; }
function TerminalIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m5 7 4 4-4 4M11 17h8" /></svg>; }
/**
 * Whether something asked for a particular screen, as opposed to simply opening
 * the app. A deep link, a settings section, or a surface remembered from this
 * tab are all requests; the operator's chosen opening screen is the default
 * they fall back to.
 */
function surfaceLabel(surface: Surface): string {
  return surface === "decisions" ? "Needs you"
    : surface === "tasks" ? "Tasks"
    : surface === "workers" ? "Workers"
    : surface === "apiary" ? "Apiary"
    : "Settings";
}
function PopoutIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M14 4h6v6"/><path d="M20 4 11 13"/><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5"/></svg>; }
function DiagnosticsIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 12h4l2.5-6 4 12L16 12h5"/></svg>; }
function RefreshIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 6v5h-5M4 18v-5h5M6.1 9a7 7 0 0 1 11.4-2.4L20 9M4 15l2.5 2.4A7 7 0 0 0 17.9 15" /></svg>; }
function SettingsIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.6v-.2h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"/></svg>; }
function ApiaryIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m12 3 4 2.3v4.6L12 12.2 8 9.9V5.3L12 3Zm-4 6.9-4 2.3v4.6L8 19l4-2.2v-4.6L8 9.9Zm8 0-4 2.3v4.6l4 2.2 4-2.2v-4.6l-4-2.3Z"/></svg>; }
function ThemeIcon({ theme }: { theme: ColorTheme }) { return theme === "light" ? <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v2m0 14v2M3 12h2m14 0h2M5.6 5.6 7 7m10 10 1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4"/><circle cx="12" cy="12" r="4"/></svg> : <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 15.5A8 8 0 0 1 8.5 4 8.5 8.5 0 1 0 20 15.5Z"/></svg>; }

function isTypingTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) || Boolean(target.closest("[role='menu'], .xterm")));
}

function shouldFocusTerminalInput(): boolean {
  return !window.matchMedia?.("(pointer: coarse)").matches;
}
