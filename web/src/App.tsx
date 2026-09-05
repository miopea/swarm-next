import { isClosedTaskState, isOpenTaskState } from "./api/tasks";
import BroadcastToWorkers from "./workers/BroadcastToWorkers";
import ConversationDriftCard, { type WorkerConversation } from "./workers/ConversationDriftCard";
import PublicAddressWarning from "./PublicAddressWarning";
import StaleBundleNotice from "./StaleBundleNotice";
import { useDogfoodCollection } from "./runtime/useDogfoodCollection";
import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type FormEvent } from "react";

import {
  assignTask,
  BROWSER_SESSION_AUTH,
  createBrowserSession,
  createTask,
  fetchReleaseNotes,
  fetchDevelopmentRuntime,
  type DevelopmentRuntime,
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
  restartAllWorkers,
  runQueenAutomation,
  type UnansweredEmailTask,
  type DecisionSurface,
  createWorker,
  fetchHealth,
  fetchRuntimeResources,
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
  fetchSettledTasks,
  broadcastToWorkers,
  fetchWorkerConversations,
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
  adoptWorker,
  openWorkerShell,
  spawnTemporaryWorker,
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
  type HeldBriefing,
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
import type { BlockedEscalation, ReleaseVersionNotes, UnsettledReview } from "./api";
import { bundleIsStale } from "./staleBundle";
import BeeMascot from "./brand/BeeMascot";
import ApiaryAttentionCard from "./apiary/ApiaryAttentionCard";
import QueenAutomationAttentionCard from "./orchestration/QueenAutomationAttentionCard";
import UnansweredEmailAttentionCard from "./tasks/UnansweredEmailAttentionCard";
import HeldDeliveryAttentionCard from "./orchestration/HeldDeliveryAttentionCard";
import { isQueuedDeliveryObservation } from "./orchestration/deliveryAttention";
import { passkeysSupported, signInWithPasskey } from "./settings/passkeys";
import { configureTerminalImageLimit } from "./terminal/TerminalAttachments";
import { queenAutomationNeedsAttention } from "./orchestration/queenAutomationPresentation";
import { foreignEngagement, workerAttention, workerSwitcherDetail } from "./workers/workerAttention";
import DecisionInbox from "./decisions/DecisionInbox";
import DogfoodFeedbackDialog from "./feedback/DogfoodFeedbackDialog";
import ShellModal from "./terminal/ShellModal";
import CommandPalette, { type CommandChoice } from "./navigation/CommandPalette";
import { applyColorTheme, initialColorTheme, type ColorTheme } from "./brand/theme";
import { ControlRoomLiveFeed, type LiveFeedState } from "./controlRoom/ControlRoomLiveFeed";
import UnsettledReviewCard from "./decisions/UnsettledReviewCard";
import MachinePressureBadge from "./runtime/MachinePressureBadge";
import DailyBackupNotice from "./runtime/DailyBackupNotice";
import { machinePressureNotice, type MachineResourceState } from "./runtime/machinePressure";
import RuntimeUpdateConfirm from "./runtime/RuntimeUpdateConfirm";
import HeldBriefingList from "./orchestration/HeldBriefingList";
import WhatsNewModal from "./runtime/WhatsNewModal";
import { readSeenVersion, storeSeenVersion, whatsNewFor } from "./runtime/whatsNew";
import type { RuntimeUpdateSummary } from "./runtime/runtimeUpdates";
import HiveContextIndicator from "./controlRoom/HiveContextIndicator";
import { useControlRoomModel } from "./controlRoom/useControlRoomModel";
import { visibleSettingsSections, clearSettingsSection, navigateToSettingsSection, readSettingsSection, type SettingsSection } from "./settings/settingsNavigation";
import { isSurface, readSavedSurface, saveSurface, surfaceWasRequested, type Surface } from "./navigation/startSurface";
import { PresenceController, deviceClass, presenceDeviceId, type LockDetectionState } from "./presence/PresenceController";
import { NotificationController, type NotificationCapabilityState } from "./notifications/NotificationController";
import TaskBoardControls, { type TaskBoardFilter, type TaskBoardSort, type TaskBoardSource } from "./tasks/TaskBoardControls";
import TerminalLoadBoundary from "./terminal/TerminalLoadBoundary";
import { initialMobileKeysVisibility, rememberMobileKeysVisibility } from "./terminal/MobileTerminalComposer";
import { terminalWorkspace } from "./terminal/TerminalWorkspace";
import WorkerRosterItem from "./workers/WorkerRosterItem";
import WorkerAvatar from "./workers/WorkerAvatar";
import WorkerContextBar from "./workers/WorkerContextBar";
import { workerWork } from "./workers/workerWork";
import { normalizeRosterQuery, orphanSessionMatchesRosterQuery, repositoryName, workerMatchesRosterQuery } from "./workers/workerRoster";
import { useWorkerRailWidth } from "./layout/useWorkerRailWidth";
import { useModalFocus } from "./shared/useModalFocus";
import { isExpectedRuntimeHandoff, requestRuntimeHandoff } from "./runtime/runtimeMaintenance";
import { useRuntimeUpdate } from "./runtime/useRuntimeUpdate";
import { useVisiblePolling } from "./runtime/useVisiblePolling";
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
const QueuesView = lazy(() => import("./queues/QueuesView"));
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

/**
 * What actually went wrong with a development reload, in the operator's terms.
 *
 * The reload records `reason` (which step) and `detail` (that step's own last
 * line). Both are repeated rather than interpreted: a message we compose from a
 * cause we did not observe is how "did not compile" came to be shown for a
 * build that compiled.
 */
function developmentFailureMessage(runtime?: DevelopmentRuntime): string {
  const detail = runtime?.failure_detail?.trim();
  const said = detail ? ` It said: ${detail}` : "";
  switch (runtime?.failure_reason) {
    case "build":
      return `The development working copy did not compile. The current release is still running.${said}`;
    case "install":
      return `The development build compiled, but could not be installed. The current release is still running.${said}`;
    case "protocol-change":
      return `This checkout changes the terminal-host protocol, which a reload cannot install — it stops every worker, so run the protocol migration when they are idle.${said}`;
    case "source-moved":
      return `The checkout changed while the build was running, so nothing was installed and the current release is still running. Reload again once the files have stopped moving.${said}`;
    default:
      // "DID NOT RECORD WHY" MUST NOT BE SAID WHEN IT DID. This branch used to
      // claim the reason was missing and then print it in the next sentence —
      // hit the moment source-moved was added and this switch was not told,
      // which is the same defect as every refusal that names the wrong field.
      // A step nobody has taught this function about is still a step that
      // recorded its cause, so the default now leans on the detail rather than
      // denying it exists.
      return detail
        ? `The development reload failed. The current release is still running.${said}`
        : "The development reload failed and did not record why. The current release is still running; the development reload service log has the detail.";
  }
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
  useEffect(() => {
    if (operatorToken) terminalWorkspace.reconcileSessions(
      sessions.filter((session) => session.running).map((session) => session.session_id),
    );
  }, [operatorToken, sessions]);
  /**
   * SETTLED WORK, HELD SEPARATELY FROM THE BOARD SNAPSHOT.
   *
   * The control room reloads its entire snapshot on every task event, and
   * settled work was the large majority of it — 462 of 561 tasks, 1,411 KB of
   * 1,711 KB on the operator's Hive — reloaded constantly to render a collapsed
   * panel. It now loads once per session, and again when that panel is opened,
   * so an event refresh carries the 99 rows somebody is actually looking at.
   *
   * The trade: this list can be stale between those two moments. That is
   * acceptable because settled work is, by definition, finished — nothing is
   * coming for it — and opening the panel refreshes it.
   */
  const [settledTasks, setSettledTasks] = useState<Task[]>([]);
  const loadSettledTasks = useCallback((token: string) => {
    void fetchSettledTasks(token)
      // A settled list that fails to load must not take the board down with it:
      // the board's own 99 rows are the ones that matter.
      .then((settled: Task[]) => { if (Array.isArray(settled)) setSettledTasks(settled); })
      .catch(() => undefined);
  }, []);
  useEffect(() => {
    if (!operatorToken) { setSettledTasks([]); return; }
    loadSettledTasks(operatorToken);
  }, [operatorToken, loadSettledTasks]);
  const boardTasks = useMemo(() => [...tasks, ...settledTasks], [tasks, settledTasks]);
  const keeper = hiveIdentity?.apiary_context?.mode === "federated" && hiveIdentity.apiary_context.local_role === "keeper";
  const federated = hiveIdentity?.apiary_context?.mode === "federated";
  const [activeSessionId, setActiveSessionId] = useState<string>();
  /**
   * Scratch shells, held here rather than read from the control room.
   *
   * A shell is deliberately not a worker session: nothing binds it server-side,
   * so it never appears in controlRoom.sessions and the roster never shows it as
   * a worker. That is the feature, not an omission — a bash prompt cannot answer
   * "is this worker working", and a bound shell would make a SLEEPING worker
   * read as awake.
   *
   * Held as a MODAL over the worker it was opened from rather than as another
   * entry in the rail. As a rail entry it appeared at the bottom of the roster,
   * did not jump there when opened, and read as a peer of the workers it is not
   * one of. Closing it now returns the operator to the worker they started on,
   * because that is where they were going anyway.
   *
   * The browser is the only thing that knows a shell is open, so a refresh loses
   * the reference while the shell keeps running on the host.
   */
  const [shell, setShell] = useState<{ session_id: string; worker_name: string }>();
  const [terminalRevision, setTerminalRevision] = useState(0);
  const [showFeedback, setShowFeedback] = useState(false);
  const [feedbackRevision, setFeedbackRevision] = useState(0);
  const [showCommands, setShowCommands] = useState(false);
  const [showMobileWorkers, setShowMobileWorkers] = useState(false);
  const [showMobileRuntime, setShowMobileRuntime] = useState(false);
  // No search field on the phone, and so no initial focus to give one. Opening
  // the picker used to focus a text input, which raised the keyboard over the
  // list the operator had just asked to see. Focus falls to the first control
  // in the dialog instead, which does not.
  const mobileWorkerDialog = useModalFocus<HTMLElement>(() => setShowMobileWorkers(false), showMobileWorkers);
  const [workerVisibility, setWorkerVisibility] = useState<WorkerVisibility>(readWorkerVisibility);
  const [workerQuery, setWorkerQuery] = useState("");
  const [terminalConnection, setTerminalConnection] = useState<string>();
  const { runtimeUpdates, developmentMode, refreshRuntimeUpdate } = useRuntimeUpdate(operatorToken || undefined);
  const dogfoodCollection = useDogfoodCollection(operatorToken, developmentMode, import.meta.env.VITE_SWARM_BUILD_VERSION);
  const [whatsNew, setWhatsNew] = useState<ReleaseVersionNotes[]>([]);
  const [whatsNewTruncated, setWhatsNewTruncated] = useState(false);
  const [whatsNewEarlier, setWhatsNewEarlier] = useState<ReleaseVersionNotes[]>([]);
  // Asked for ONCE per session rather than polled: the notes only change when
  // the build under the operator changes, and that already reloads the page.
  useEffect(() => {
    if (!operatorToken) return;
    let cancelled = false;
    void (async () => {
      try {
        const notes = await fetchReleaseNotes(operatorToken);
        if (cancelled) return;
        const { show, recordAs, truncated, earlier } = whatsNewFor(
          notes.releases,
          notes.running_version,
          readSeenVersion(),
          notes.previous_version,
        );
        if (recordAs) storeSeenVersion(recordAs);
        setWhatsNew(show);
        setWhatsNewTruncated(truncated);
        setWhatsNewEarlier(earlier);
      } catch {
        // A change list nobody can fetch is not worth telling the operator
        // about; the Hive works either way and a development build has none.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [operatorToken]);
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
  useEffect(() => { if (!operatorToken) setAwaitingReply([]); }, [operatorToken]);
  const refreshAwaitingReply = useCallback(async (signal: AbortSignal) => {
    if (!operatorToken) return;
    try {
      const awaiting = await fetchEmailTasksAwaitingReply(operatorToken, signal);
      if (!signal.aborted && Array.isArray(awaiting)) setAwaitingReply(awaiting);
    } catch { /* Keep the last known queue during an unavailable read. */ }
  }, [operatorToken, feedbackRevision]);
  useVisiblePolling(refreshAwaitingReply, Boolean(operatorToken), 30_000);
  // WHETHER THIS PAGE IS STILL THE PAGE THE HIVE SERVES. Health is read once
  // at mount, which is fine for starting up and useless for a tab left open
  // across an upgrade — and a tab left open across an upgrade is exactly the
  // situation that had a developer reporting a bug fixed days earlier.
  //
  // Rides the same 30s cadence rather than adding a poll. The cost is one
  // unauthenticated health call, which this page already makes.
  const [serverVersion, setServerVersion] = useState<string | null>(null);
  const [dismissedVersion, setDismissedVersion] = useState<string | null>(null);
  const refreshServerVersion = useCallback(async (signal: AbortSignal) => {
    try {
      const health = await fetchHealth(signal);
      if (!signal.aborted) setServerVersion(health.version);
    } catch { /* An unavailable version read does not invalidate the loaded UI. */ }
  }, []);
  // Startup already reads health; returning to the tab must still refresh immediately.
  useVisiblePolling(refreshServerVersion, true, 30_000, 8_000, { initialRefresh: false });
  // WHETHER THE MACHINE UNDERNEATH CAN TAKE ANY MORE. Swarm starts processes,
  // and a Hive that quietly exhausts a box takes the operator's machine with
  // it. The server has computed this for some time and ADR 0040 already
  // refuses automatic starts against it; it was only ever RENDERED in
  // Settings -> Diagnostics, which is not a screen anyone sits on while their
  // machine is dying.
  //
  // A FAILED READ IS NOT SWALLOWED. The polls above end in
  // `.catch(() => undefined)`, which is right for them — a malformed answer
  // during a rolling update should not empty the attention queue. It is wrong
  // here: silence is indistinguishable from a healthy machine, so a failure
  // becomes `failed` and the header says so.
  const [machineResources, setMachineResources] = useState<MachineResourceState>({ kind: "loading" });
  const [diagnosticsActive, setDiagnosticsActive] = useState(false);
  useEffect(() => { if (!operatorToken) setMachineResources({ kind: "loading" }); }, [operatorToken]);
  const refreshMachineResources = useCallback(async (signal: AbortSignal) => {
    if (!operatorToken) return;
    try {
      const resources = await fetchRuntimeResources(operatorToken, signal);
      if (!signal.aborted) setMachineResources({ kind: "ready", resources });
    } catch {
      if (!signal.aborted || (signal.reason instanceof DOMException && signal.reason.name === "TimeoutError")) {
        setMachineResources({ kind: "failed" });
      }
    }
  }, [operatorToken]);
  const refreshSharedMachineResources = useVisiblePolling(refreshMachineResources, Boolean(operatorToken), diagnosticsActive ? 10_000 : 30_000);
  const sharedMachineResources = useMemo(() => ({ state: machineResources, refresh: refreshSharedMachineResources, setDiagnosticsActive }), [machineResources, refreshSharedMachineResources]);
  const machinePressure = machinePressureNotice(machineResources);
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
  const [showBroadcast, setShowBroadcast] = useState(false);
  /**
   * Whether each worker's pinned conversation is still the newest one.
   *
   * This legacy filesystem-recency diagnostic is separate from authenticated
   * provider startup/selection evidence. A failed scan leaves last-known data
   * in place and marks its freshness as unconfirmed in runtime status.
   */
  const [workerConversations, setWorkerConversations] = useState<WorkerConversation[]>([]);
  const [conversationChecksUnavailable, setConversationChecksUnavailable] = useState(false);
  useEffect(() => {
    if (!operatorToken) {
      setWorkerConversations([]);
      setConversationChecksUnavailable(false);
    }
  }, [operatorToken]);
  const refreshConversations = useCallback(async (signal: AbortSignal) => {
    if (!operatorToken) return;
    const timedOut = () => {
      if (signal.reason?.name === "TimeoutError") setConversationChecksUnavailable(true);
    };
    signal.addEventListener("abort", timedOut, { once: true });
    try {
      const page = await fetchWorkerConversations(operatorToken, signal);
      if (!signal.aborted) {
        if (!Array.isArray(page.workers)) throw new Error("Invalid conversation check response");
        setWorkerConversations(page.workers as WorkerConversation[]);
        setConversationChecksUnavailable(false);
      }
    } catch {
      if (!signal.aborted) setConversationChecksUnavailable(true);
    } finally {
      signal.removeEventListener("abort", timedOut);
    }
  }, [operatorToken, workers.length]);
  // Filesystem-backed transcript checks do not run for hidden windows or overlap.
  const retryConversationChecks = useVisiblePolling(refreshConversations, Boolean(operatorToken), 120_000);
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
  const refreshPublicAddress = useCallback(async (signal: AbortSignal) => {
    if (!operatorToken) return;
    const status = await readTunnel(operatorToken, signal);
    if (!signal.aborted) setPublicAddress(status);
  }, [operatorToken]);
  useVisiblePolling(refreshPublicAddress, Boolean(operatorToken) && !detached, 15_000);

  const [presence, setPresence] = useState<OperatorPresence>();
  const readRecentActivity = useCallback((signal: AbortSignal) => {
    if (!operatorToken) return Promise.reject(new Error("Operator session is unavailable"));
    return fetchRecentTaskActivity(operatorToken, 100, signal);
  }, [operatorToken]);
  const [lockDetectionState, setLockDetectionState] = useState<LockDetectionState>("unsupported");
  const [notificationSettings, setNotificationSettings] = useState<NotificationSettings>();
  const [queenPolicy, setQueenPolicy] = useState<QueenAutonomyPolicy>();
  const [queenAutomation, setQueenAutomation] = useState<QueenAutomationStatus>();
  /** What the coordinator is holding behind an unanswered terminal prompt. */
  const [heldDeliveries, setHeldDeliveries] = useState<HeldDelivery[]>([]);
  const [coordinatorUnavailable, setCoordinatorUnavailable] = useState(false);
  const [blockedEscalations, setBlockedEscalations] = useState<BlockedEscalation[]>([]);
  const [unsettledReview, setUnsettledReview] = useState<UnsettledReview[]>([]);
  // NOT fed into attentionCount, on purpose. See HeldBriefingList for why a
  // self-resolving state must not badge.
  const [heldBriefings, setHeldBriefings] = useState<HeldBriefing[]>([]);
  /** Bumped to re-read held work immediately rather than waiting for the tick. */
  const [heldDeliveryRefresh, setHeldDeliveryRefresh] = useState(0);
  const [providers, setProviders] = useState<ProviderCapabilities>({ claude_code: true, codex: false });
  const [providerCapabilitiesUnavailable, setProviderCapabilitiesUnavailable] = useState(false);
  const [notificationState, setNotificationState] = useState<NotificationCapabilityState>("unsupported");
  const presenceController = useMemo(() => new PresenceController(), []);
  useEffect(() => presenceController.setPresenceMode(presence?.mode), [presenceController, presence?.mode]);
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
      setCoordinatorUnavailable(false);
      setBlockedEscalations([]);
      setUnsettledReview([]);
      setHeldBriefings([]);
    }
  }, [operatorToken]);
  const refreshHeldDeliveries = useCallback(async (signal: AbortSignal) => {
    if (!operatorToken) return;
    try {
      const status = await fetchCoordinatorStatus(operatorToken, signal);
      if (signal.aborted) return;
      setHeldDeliveries(status.held ?? []);
      setBlockedEscalations(status.blocked_escalations ?? []);
      setUnsettledReview(status.unsettled_review ?? []);
      setHeldBriefings(status.held_briefings ?? []);
      setCoordinatorUnavailable(false);
    } catch {
      if (!signal.aborted || (signal.reason instanceof DOMException && signal.reason.name === "TimeoutError")) {
        // A failed observation is not a resolution. Keep the last known holds.
        setCoordinatorUnavailable(true);
      }
    }
  }, [operatorToken, heldDeliveryRefresh]);
  useVisiblePolling(refreshHeldDeliveries, Boolean(operatorToken), HELD_DELIVERY_POLL_MS);

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
        // The baseline the staleness check compares against, so a page loaded
        // AFTER an upgrade starts out correct rather than warning for 30s.
        setServerVersion(health.version);
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
    const feed = new ControlRoomLiveFeed();
    const connect = () => feed.start(
      operatorToken,
      async (page, signal) => {
        const runtimeChanged = page.reset_required || page.events.some((event) => event.kind === "runtime_changed");
        const refreshQueenAutomation = page.reset_required || page.events.some((event) => event.kind === "workers_changed");
        const [controlRoom, refreshedPresence, refreshedNotifications, refreshedQueenPolicy, refreshedQueenAutomation, refreshedPresentation, refreshedProviders] = await Promise.all([
          controlRoomModel.refreshFromEvents(operatorToken, page, signal),
          page.reset_required || page.events.some((event) => event.kind === "presence_changed")
            ? fetchPresence(operatorToken, signal)
            : Promise.resolve(undefined),
          page.reset_required || page.events.some((event) => event.kind === "notifications_changed")
            ? fetchNotificationSettings(operatorToken, signal)
            : Promise.resolve(undefined),
          runtimeChanged
            ? fetchQueenAutonomyPolicy(operatorToken, signal)
            : Promise.resolve(undefined),
          refreshQueenAutomation
            ? fetchQueenAutomationStatus(operatorToken, signal).catch(() => undefined)
            : Promise.resolve(undefined),
          runtimeChanged
            ? fetchPresentationPreferences(operatorToken, presentationDevice, signal)
            : Promise.resolve(undefined),
          runtimeChanged
            ? fetchProviderCapabilities(operatorToken, signal).catch(() => {
                if (!signal.aborted) setProviderCapabilitiesUnavailable(true);
                return undefined;
              })
            : Promise.resolve(undefined),
        ]);
        if (signal.aborted) return;
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
        if (controlRoom) setActiveSessionId((current) =>
          current && controlRoom.sessions.some((session) => session.session_id === current)
            ? current
            : preferredSessionId(controlRoom.workers, controlRoom.sessions),
        );
      },
      setLiveFeedState,
    );
    // This is presentation traffic, not worker execution or push delivery.
    // Hidden documents retain their last snapshot but own no event poll or
    // invalidation fetch. Returning starts a fresh cursor handshake immediately.
    const syncFeedVisibility = () => {
      if (document.visibilityState === "visible") {
        connect();
      } else {
        feed.stop();
        setLiveFeedState("connecting");
      }
    };
    syncFeedVisibility();
    document.addEventListener("visibilitychange", syncFeedVisibility);
    return () => {
      document.removeEventListener("visibilitychange", syncFeedVisibility);
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
  function reloadTerminalView() {
    if (!activeSessionId) return;
    terminalWorkspace.resetSessionRenderer(activeSessionId);
    setTerminalRevision((current) => current + 1);
  }

  async function refreshControlRoom(recoverTerminal = false) {
    if (!operatorToken) return;
    // A local renderer failure must not wait behind unrelated API reads. Bind
    // the reset to the view selected now, not whichever view is open later.
    if (recoverTerminal) reloadTerminalView();
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

  /**
   * Applies a chosen bee on its own.
   *
   * Its own call rather than a seventh argument to maintainWorkerProfile: a
   * mark is applied the moment it is clicked, while everything else in that
   * form is a draft the operator reviews before saving. Sending both together
   * would either save a half-typed name or make choosing a bee wait for one.
   */
  async function chooseWorkerMark(workerId: string, mark: string) {
    if (!operatorToken) return;
    await perform(async () => {
      const updated = await updateWorker(operatorToken, workerId, { mark });
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

  function openWorkerProfile(workerId: string) {
    const worker = workers.find((candidate) => candidate.id === workerId);
    if (!worker) return;
    if (worker.active_session_id) openWorker(worker.active_session_id);
    else void startExistingWorker(worker);
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

  /**
   * Ends and reconnects every live worker session.
   *
   * The operator's lever for staleness nothing announces: a worker caches its
   * MCP tool list at connect, so a changed agent tool surface reaches nobody
   * until the session reconnects — while the engine card correctly reports the
   * engine as current. restartProviders cannot do this; it only touches
   * sessions whose provider release has moved on.
   */
  async function forceWorkerReload() {
    if (!operatorToken) return;
    await perform(async () => {
      await restartAllWorkers(operatorToken);
      await refreshControlRoom(true);
    }, "Reconnecting every worker…");
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
        // NEVER ASSERT A CAUSE WE DID NOT OBSERVE. This said "did not compile"
        // for every failure, so an install refused for a good reason read as a
        // compiler error and sent the operator to the wrong file. The reload
        // now records which step failed and what it said; this repeats that
        // rather than guessing, and only falls back to a generic sentence when
        // there is genuinely nothing recorded.
        setOperationError(developmentFailureMessage(development));
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

  /**
   * Spawns a throwaway sibling on another provider, in the same workspace.
   *
   * Not a second session on the worker underneath: two providers under one
   * worker would break the one-session-per-worker assumption that sleep/wake
   * and briefing delivery rely on.
   */
  async function spawnTemporary(worker: Worker, provider: string) {
    if (!operatorToken) return;
    setOperationError(undefined);
    try {
      const created = await spawnTemporaryWorker(operatorToken, worker.id, provider);
      const controlRoom = await loadControlRoom(operatorToken);
      setWorkers(controlRoom.workers);
      setOperationError(`${created.name} is temporary — adopt it to keep it, or release it when you are done.`);
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "the temporary worker could not be started");
    }
  }

  /** Keeps a temporary worker, under a permanent name. */
  async function adoptTemporary(worker: Worker) {
    if (!operatorToken) return;
    const name = window.prompt("Name this worker", worker.name.replace(/ · .*$/, ""));
    if (!name?.trim()) return;
    setOperationError(undefined);
    try {
      await adoptWorker(operatorToken, worker.id, name.trim());
      const controlRoom = await loadControlRoom(operatorToken);
      setWorkers(controlRoom.workers);
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "the worker could not be adopted");
    }
  }

  async function openShellForWorker(worker: Worker) {
    if (!operatorToken) return;
    setOperationError(undefined);
    try {
      const sessionId = await openWorkerShell(operatorToken, worker.id);
      // Deliberately does NOT change the active session. The operator stays on
      // the worker underneath; the shell is a modal over it.
      setShell({ session_id: sessionId, worker_name: worker.name });
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "the shell could not be opened");
    }
  }

  /**
   * Closes a shell and forgets it.
   *
   * Nothing else can: a shell is bound to no worker, so the roster's sleep
   * action does not reach it and reconcile never sweeps it. Without this the
   * only way to stop one is to restart the terminal host, which would take
   * every worker down with it.
   */
  async function closeShell(sessionId: string) {
    setShell(undefined);
    // The worker terminal underneath repaints from its own buffer the moment the
    // modal unmounts, and it does so with the metrics it measured while it was
    // covered — which renders as garbled glyphs for one frame before the next
    // full redraw corrects it. The operator saw exactly that. Resetting the
    // renderer makes the first paint after the modal a correct one; this is the
    // same remedy a recovered terminal already uses.
    if (activeSessionId) {
      terminalWorkspace.resetSessionRenderer(activeSessionId);
      setTerminalRevision((current) => current + 1);
    }
    if (!operatorToken) return;
    try {
      await stopClaudeSession(operatorToken, sessionId);
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "the shell could not be closed");
    }
  }

  const activeSession = sessions.find((session) => session.session_id === activeSessionId);
  const activeWorker = workers.find((worker) => worker.active_session_id === activeSessionId);
  const openTaskCount = tasks.filter((task) => isOpenTaskState(task.state)).length;
  // Open work somebody or something owes a move on. Deliberately excludes a
  // task whose owner the server did not state: an older API must not be able
  // to manufacture a queue, and the tab must agree with what the view shows.
  // ⚠️ THIS EQUALS openTaskCount ALWAYS, and the operator noticed: the Queues
  // and Tasks badges are the same number by construction. Every open state
  // derives a real owner and only completed and abandoned derive "nobody", so
  // this filter excludes nothing.
  //
  // NOT "fixed" by excluding blocked here. That would read 6 against 43 rows on
  // the page, which is the badge-disagrees-with-the-page defect this repo
  // already has a test for on Needs You — and trading one badge defect for
  // another is not a fix. What the badge should say is a product question and
  // it is reported rather than guessed at.
  const queuedTaskCount = tasks.filter(
    (task) => isOpenTaskState(task.state) && task.next_move_owner !== undefined && task.next_move_owner !== "nobody",
  ).length;
  const pendingDecisionCount = decisions.filter((decision) => decision.state === "pending").length;
  const pendingAssistCount = stewardAssists?.incoming?.filter((request) => request.state === "pending").length ?? 0;
  const queenWorkerId = workers.find((worker) => worker.role === "queen")?.id;
  const pendingQueenDecisionCount = decisions.filter((decision) => decision.state === "pending" && decision.requesting_worker_id === queenWorkerId).length;
  // Counted only when it is not already counted as the specific request it
  // refers to, so one thing needing the operator is one item in the queue.
  const queenAutomationAttentionCount =
    queenAutomationNeedsAttention(queenAutomation, pendingQueenDecisionCount > 0, pendingQueenDecisionCount > 0)
      ? 1
      : 0;
  // Held work is one card however many deliveries are behind it, the same way
  // unanswered email is.
  //
  // It was in the queue and in neither count, so "Needs you" read 0 with a card
  // plainly on the page. A badge that disagrees with the page teaches the
  // operator to stop believing the badge, which is the one thing it has to do.
  const actionableHeldDeliveries = heldDeliveries.filter((held) => !isQueuedDeliveryObservation(held));
  const queuedDeliveryObservations = heldDeliveries.filter(isQueuedDeliveryObservation);
  const heldDeliveryAttentionCount = actionableHeldDeliveries.length > 0 ? 1 : 0;
  // ONE CARD, ONE COUNT, like held deliveries and blocked escalations above.
  // The card carries the number itself, so the operator sees how much without
  // opening anything; the badge counts things to deal with, not rows.
  const unsettledReviewAttentionCount = unsettledReview.length > 0 ? 1 : 0;
  // A reviewable default is distinct from a scan that cannot establish history.
  // Actual filesystem faults also have an operator action; benign never-run,
  // empty-history and bounded-scan outcomes remain runtime diagnostics.
  const uncheckedConversations = workerConversations.filter((worker) => worker.freshness.state === "unknown");
  const actionableConversationChecks = workerConversations.filter(
    (worker) => worker.freshness.state === "stale"
      || (worker.freshness.state === "unknown" && worker.freshness.cause?.fault === true),
  );
  // One card, one badge count. The card reads the same actionable collection;
  // do not count unknowns that only appear in runtime diagnostics.
  const conversationDriftAttentionCount =
    actionableConversationChecks.length > 0 ? 1 : 0;
  // ⚠️ unsettledReviewAttentionCount IS DELIBERATELY ABSENT, and so is its card.
  // QueuesView's own docstring names it as the anti-pattern: "a card there
  // reading 'N pieces of finished work are waiting on Queen' is Queen's backlog
  // rendered in the operator's attention area, and it trains them to ignore the
  // screen that matters." That surface exists and groups by who owes the next
  // move, so the work is not hidden — it is filed under Waiting on Queen, which
  // is what it is. The operator: "some of this is queue items not needs you
  // items."
  //
  // The maturity ruling supersedes the earlier twelve-hour escalation: age is
  // queue evidence, not a human action. Queen escalates through a real decision.
  const attentionCount = pendingDecisionCount + pendingAssistCount + queenAutomationAttentionCount
    + heldDeliveryAttentionCount
    + conversationDriftAttentionCount + awaitingReply.length;
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
    // A shell is unbound by design, which is exactly what makes a session an
    // "orphan" here — so without this filter every shell is listed twice, once
    // as itself and once as a pre-roster session with a generated worker name.
    () => orphanSessions.filter((session) =>
      session.session_id !== shell?.session_id
      && orphanSessionMatchesRosterQuery(workerName(session.session_id), normalizedWorkerQuery)),
    [normalizedWorkerQuery, orphanSessions, shell],
  );
  const sleepingWorkerMatchesRailQuery = useMemo(
    () => workerVisibility === "awake" && normalizedWorkerQuery
      ? workers.some((worker) => !worker.running && workerMatchesRosterQuery(worker, normalizedWorkerQuery))
      : false,
    [normalizedWorkerQuery, workerVisibility, workers],
  );
  // UNFILTERED ON PURPOSE, and this is the record that it is a decision.
  //
  // The rail narrows its list with the search box beside it. The phone picker
  // has no search field — it was removed because focusing it raised the
  // keyboard over the roster it was meant to help you read — so there is no
  // query to apply and filtering here would hide workers against a box the
  // operator cannot see or clear.
  const mobileVisibleWorkers = visibleWorkers;
  const mobileVisibleOrphanSessions = orphanSessions;
  const liveWorkerCount = workers.filter((worker) => worker.running).length
    + orphanSessions.filter((session) => session.running).length;
  const rosterWorkerCount = workers.length + orphanSessions.length;
  const tasksBySession = useMemo(
    () => new Map(
      tasks
        .filter((task) => task.assigned_session_id && isOpenTaskState(task.state))
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
      if (!task.assigned_worker_id || isClosedTaskState(task.state)) return;
      const assigned = grouped.get(task.assigned_worker_id) ?? [];
      assigned.push(task);
      grouped.set(task.assigned_worker_id, assigned);
    });
    return new Map([...grouped].map(([workerId, assigned]) => [workerId, workerWork(assigned)]));
  }, [tasks]);
  const activeWorkerWork = activeWorker ? workByWorker.get(activeWorker.id) : undefined;
  /**
   * The one line under the worker's name in the collapsed picker.
   *
   * The task it is carrying, or the repository it works in. Never blank and
   * never a different KIND of fact for different workers — that inconsistency
   * is exactly what the operator reported.
   */
  const activeWorkerSecondLine = activeWorkerWork?.current?.title
    ?? (activeWorker
      ? (activeWorker.role === "queen" ? "Queen" : repositoryName(activeWorker.workspace))
      : "No worker selected");
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
          {/* The Hive's own mark is the QUEEN. It defaulted to role="worker", so the
              control room was headed by one of the workers rather than by the Hive
              — and every worker avatar below it is the same drawing, which is what
              made the header read as just another bee. */}
          <div className="brand-mark"><BeeMascot role="queen" expression="available" /></div>
          <div className="brand-copy"><p className="eyebrow">Swarm</p><h1>Control room</h1><HiveContextIndicator identity={hiveIdentity} /></div>
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
              {federated ? <button className={`apiary-nav-button${surface === "apiary" ? " selected" : ""}`} aria-current={surface === "apiary" ? "page" : undefined} data-detached={surfaceIsDetached("apiary") || undefined} onClick={() => showSurface("apiary")}><span><ApiaryIcon /> Apiary</span>{pendingAssistCount ? <small aria-label={`${pendingAssistCount} pending help offer${pendingAssistCount === 1 ? "" : "s"}`}>{pendingAssistCount}</small> : null}</button> : null}
              <span className="surface-nav-item"><button className={surface === "workers" ? "selected" : ""} aria-current={surface === "workers" ? "page" : undefined} data-detached={surfaceIsDetached("workers") || undefined} aria-label={`Workers, ${liveWorkerCount} active of ${rosterWorkerCount}`} onClick={() => showSurface("workers")}>
                <span><TerminalIcon /> Workers</span><small>{liveWorkerCount}/{rosterWorkerCount}</small>
              </button>{operatorToken && !detached ? <button type="button" className="surface-nav-popout" aria-label={`Open ${surfaceLabel("workers")} in a new window`} title={`Open ${surfaceLabel("workers")} in a new window. Workers keep running; a window only views them.`} onClick={() => setPopoutBlocked(!openSurfaceWindow("workers", (url, name, features) => window.open(url, name, features)))}><PopoutIcon /></button> : null}</span>
              <span className="surface-nav-item"><button className={surface === "queues" ? "selected" : ""} aria-current={surface === "queues" ? "page" : undefined} data-detached={surfaceIsDetached("queues") || undefined} onClick={() => showSurface("queues")}>
                <span><QueuesIcon /> Queues</span><small>{queuedTaskCount}</small>
              </button>{operatorToken && !detached ? <button type="button" className="surface-nav-popout" aria-label={`Open ${surfaceLabel("queues")} in a new window`} title={`Open ${surfaceLabel("queues")} in a new window. Workers keep running; a window only views them.`} onClick={() => setPopoutBlocked(!openSurfaceWindow("queues", (url, name, features) => window.open(url, name, features)))}><PopoutIcon /></button> : null}</span>
              <span className="surface-nav-item"><button className={surface === "tasks" ? "selected" : ""} aria-current={surface === "tasks" ? "page" : undefined} data-detached={surfaceIsDetached("tasks") || undefined} onClick={() => showSurface("tasks")}>
                <span><TaskIcon /> Tasks</span><small>{openTaskCount}</small>
              </button>{operatorToken && !detached ? <button type="button" className="surface-nav-popout" aria-label={`Open ${surfaceLabel("tasks")} in a new window`} title={`Open ${surfaceLabel("tasks")} in a new window. Workers keep running; a window only views them.`} onClick={() => setPopoutBlocked(!openSurfaceWindow("tasks", (url, name, features) => window.open(url, name, features)))}><PopoutIcon /></button> : null}</span>
              <button className={surface === "settings" ? "selected" : ""} aria-current={surface === "settings" ? "page" : undefined} onClick={() => openSettings()}>
                <span><SettingsIcon /> Settings</span>
              </button>
              <button type="button" className="mobile-runtime-toggle" aria-expanded={showMobileRuntime} aria-controls="runtime-system-status" onClick={() => setShowMobileRuntime((open) => !open)}>
                <span><DiagnosticsIcon /> System</span>
              </button>

              <PublicAddressWarning
                status={publicAddress}
                onOpen={() => openSettings("settings-access")}
                onStop={async () => {
                  if (!operatorToken) return;
                  setPublicAddress(await stopTunnel(operatorToken));
                }}
              />
              {/* In the rail rather than over the page, for the same reason the
                  address warning is: it is a state this Hive is in, not an
                  interruption, and a terminal is a bad place to put a banner. */}
              <StaleBundleNotice
                stale={bundleIsStale(serverVersion)}
                serverVersion={serverVersion}
                dismissed={dismissedVersion}
                onDismiss={setDismissedVersion}
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
                  {visibleSettingsSections(developmentMode).map(([id, label]) => (
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
                        onOpenShell={() => void openShellForWorker(worker)}
                        onSpawnTemporary={(provider) => void spawnTemporary(worker, provider)}
                        onAdopt={() => void adoptTemporary(worker)}
                        onRelease={() => void removeWorkerProfile(worker.id)}
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
        {detached ? null : <div id="runtime-system-status" className={`rail-footer${showMobileRuntime ? " mobile-open" : ""}`} role="region" aria-label="Runtime and system status">
          <RuntimeStatus state={loadState} developmentMode={developmentMode} />
          {operatorToken ? (
            <button
              type="button"
              className="runtime-diagnostics"
              aria-label="Open diagnostics"
              title="Browser and server evidence"
              onClick={() => openSettings("settings-maintenance")}
            ><DiagnosticsIcon /><span>Diagnostics</span></button>
          ) : null}
          {/* Beside the runtime line because it is about the machine that line
              is running on. Silent when the machine is fine — a header that
              shows something most of the day stops being read, which is the
              failure this exists to prevent. */}
          <MachinePressureBadge notice={machinePressure} />
          {operatorToken && machineResources.kind === "ready" ? <DailyBackupNotice status={machineResources.resources.daily_backup} onDetails={() => openSettings("settings-maintenance")} /> : null}
          {operatorToken && conversationChecksUnavailable ? (
            <div className="runtime-update-card" role="status">
              <p className="runtime-update-label">Conversation checks unavailable</p>
              <p className="runtime-update-detail">Conversation status is unconfirmed. Existing results may be out of date.</p>
              <button type="button" className="runtime-update-run" onClick={() => void retryConversationChecks()}>Retry conversation checks</button>
            </div>
          ) : null}
          {operatorToken && uncheckedConversations.length > 0 ? (
            <details className="runtime-update-card">
              <summary>Conversation history unconfirmed · {uncheckedConversations.length}</summary>
              <p className="runtime-update-detail">Swarm could not verify these histories. This does not prove context was lost or require a conversation change.</p>
              <ul>
                {uncheckedConversations.map((worker) => <li key={worker.worker_id}>
                  <strong>{worker.name}</strong>: {worker.freshness.state === "unknown" ? worker.freshness.reason : ""}
                </li>)}
              </ul>
              <button type="button" className="runtime-update-run" onClick={() => void retryConversationChecks()}>Retry conversation checks</button>
            </details>
          ) : null}
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
              {activeWorker
                    ? <WorkerAvatar worker={activeWorker} />
                    : <span className="worker-avatar"><BeeMascot expression="sleeping" /></span>}
              {/* On a phone this trigger replaces the header's worker name and
                  Hive line entirely, and the context bar's task chip is hidden
                  because a row of chips would return the vertical space the
                  phone layout reclaimed. So the task takes the small line the
                  Hive indicator was using rather than adding one: on the worker
                  surface, what the worker is carrying is the thing the phone
                  could not see at all, and the Hive line is on every other
                  surface's header. */}
              <span>
                {/* ONE SECOND LINE, ALWAYS, on the operator's ruling: "name plus
                    one consistent second line, either the repo name or the
                    current issue being worked on."

                    It used to be the task title OR the apiary indicator, which
                    is how the line came to differ between workers — and a
                    specificity loss then hid one branch and not the other, so
                    it read blank on a worker with a task and "Grand Garden
                    KEEPER" on one without. The apiary is deliberately NOT the
                    fallback now: it says which Hive you are on, which is on
                    every other surface's header and is not what somebody
                    scanning a worker wants to know.

                    One element rather than two branches, so there is no second
                    thing that can be styled differently from the first. */}
                <span className="mobile-worker-task">{activeWorkerSecondLine}</span>
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
            {operatorToken && <button className="icon-button broadcast-button" aria-label="Tell every worker" title="Say one thing to every running worker" onClick={() => setShowBroadcast(true)}><BroadcastIcon /></button>}
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
                {/* SAID, NOT SILENTLY ABSENT. An operator who goes looking for
                    a control and cannot find it learns nothing about whether
                    it exists; naming where it lives costs one line. */}
                <p className="mobile-worker-dialog-note">Opening a shell, temporary workers and adopting are on the desktop roster.</p>
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
                  // TWO ACTIONS, NOT THE RAIL'S SIX, and that is a decision
                  // rather than an omission. Waking is already the row's own
                  // tap. Sleeping earns a target because the mobile case IS
                  // noticing from away that something is running which should
                  // not be — but it gets its own control, since a mis-tap on a
                  // phone is easy and this one ends a session.
                  //
                  // The other four stay on the desk and the picker says so
                  // below: a shell is barely usable on a phone and outlives
                  // the screen that started it, spawning a temporary worker
                  // needs a provider submenu, adopting prompts for a typed
                  // name, and releasing is destructive and rare.
                  //
                  // Queen is never offered sleep, the same as on the rail.
                  const canSleep = Boolean(sessionId) && worker.role !== "queen";
                  return (
                    <div className="mobile-worker-row" key={worker.id}>
                    <button
                      type="button"
                      className="mobile-worker-choice"
                      aria-current={sessionId === activeSessionId ? "page" : undefined}
                      disabled={busy}
                      onClick={() => {
                        if (sessionId) openWorker(sessionId);
                        else void startExistingWorker(worker);
                        setShowMobileWorkers(false);
                      }}
                    >
                      <WorkerAvatar worker={worker} />
                      <span className="mobile-worker-choice-copy">
                        <span>
                          <strong>{worker.name}</strong>
                          <small>{worker.role === "queen" ? "Queen" : repositoryName(worker.workspace)}</small>
                          {/* SAID, NOT IMPLIED BY A BACKGROUND COLOUR. The row
                              already carried aria-current="page", so a screen
                              reader was told and a sighted operator got a faint
                              green tint. Looking straight at the row for the
                              worker they were on, they asked "Shouldn't it be
                              showing that I'm on that worker currently?" — which
                              is what a tint that has to be learned gets you.

                              Deliberately NOT the "With you" state: that one
                              means the engagement claim from Work here. Viewing
                              a worker from a phone is not engaging it, and
                              conflating the two would say something false about
                              who is driving. */}
                          {sessionId === activeSessionId ? <em className="mobile-worker-here">You&rsquo;re here</em> : null}
                        </span>
                        <small>{worker.runtime_error ?? workerSwitcherDetail(worker, assignedTask?.title, assignedTask?.state === "active")}</small>
                        {work?.summary ? <span className="worker-work-summary" title={`${worker.name}'s open work: ${work.summary}`}>Open work · {work.summary}</span> : null}
                      </span>
                      <span className={`presence ${worker.running ? "online" : "offline"}`} aria-label={worker.running ? workerAttentionLabel(worker) : "Sleeping"} />
                    </button>
                    {canSleep ? (
                      <button
                        type="button"
                        className="mobile-worker-sleep"
                        disabled={busy}
                        aria-label={`Put ${worker.name} to sleep`}
                        onClick={() => { if (sessionId) void stopSession(sessionId); }}
                      >Sleep</button>
                    ) : null}
                    </div>
                  );
                })}
                {mobileVisibleOrphanSessions.map((session) => (
                  <button type="button" className="mobile-worker-choice" aria-current={session.session_id === activeSessionId ? "page" : undefined} key={session.session_id} onClick={() => { openWorker(session.session_id); setShowMobileWorkers(false); }}>
                    <span className="worker-avatar"><BeeMascot expression={session.running ? "focused" : "sleeping"} /></span>
                    <span className="mobile-worker-choice-copy"><span><strong>{workerName(session.session_id)}</strong><small>Unconfigured session</small>{session.session_id === activeSessionId ? <em className="mobile-worker-here">You&rsquo;re here</em> : null}</span><small>{tasksBySession.get(session.session_id)?.title ?? "Pre-roster session"}</small></span>
                    <span className={`presence ${session.running ? "online" : "offline"}`} aria-label={session.running ? "Running" : "Exited"} />
                  </button>
                ))}
              </div>
              <button className="secondary-button mobile-manage-workers" type="button" onClick={() => { setShowMobileWorkers(false); openSettings("settings-workers"); }}>Manage workers</button>
            </section>
          </div>
        ) : null}
        {operatorToken && whatsNew.length > 0 ? (
          <WhatsNewModal
            releases={whatsNew}
            truncated={whatsNewTruncated}
            earlier={whatsNewEarlier}
            onDismiss={() => setWhatsNew([])}
          />
        ) : null}
        {operatorToken && runtimeConfirm ? (
          <RuntimeUpdateConfirm
            update={runtimeConfirm}
            busy={busy}
            onConfirm={() => void runRuntimeUpdate(runtimeConfirm)}
            onCancel={() => setRuntimeConfirm(undefined)}
          />
        ) : null}
        {operatorToken && shell ? (
          <ShellModal
            title="Shell"
            subtitle={`${shell.worker_name}'s workspace`}
            onClose={() => void closeShell(shell.session_id)}
          >
            <TerminalLoadBoundary key={`${operatorToken}:${shell.session_id}`}>
              <Suspense fallback={<div className="terminal-empty">Preparing shell…</div>}>
                <TerminalView
                  operatorToken={operatorToken}
                  session={{ session_id: shell.session_id, running: true }}
                  busy={false}
                  canStop={false}
                />
              </Suspense>
            </TerminalLoadBoundary>
          </ShellModal>
        ) : null}
        {operatorToken && showCommands ? <CommandPalette choices={commandChoices} onClose={() => setShowCommands(false)} /> : null}
        {operatorToken ? (
          <BroadcastToWorkers
            open={showBroadcast}
            onClose={() => setShowBroadcast(false)}
            onBroadcast={(body) => broadcastToWorkers(operatorToken, body)}
          />
        ) : null}
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
              coordinatorUnavailable={coordinatorUnavailable}
              decisions={decisions}
              tasks={tasks}
              workers={workers}
              busy={busy}
              focusDecisionId={decisionFocus?.id}
              focusRequest={decisionFocus?.request}
              additionalPendingCount={pendingAssistCount + queenAutomationAttentionCount + heldDeliveryAttentionCount + conversationDriftAttentionCount + awaitingReply.length}
              attentionCards={<>
                <UnansweredEmailAttentionCard awaiting={awaitingReply} busy={busy} onSendReply={sendAwaitingReply} onSaveReply={saveAwaitingReply} onReviseReply={reviseAwaitingReply} onOpenTask={(taskId) => { setTaskFocus((current) => ({ id: taskId, request: (current?.request ?? 0) + 1 })); setSurface("tasks"); }} />
                <QueenAutomationAttentionCard status={queenAutomation} queenRequestPending={pendingQueenDecisionCount > 0} coveredBySpecificDecision={pendingQueenDecisionCount > 0} onOpenQueen={openQueenForAttention} onReviewSettings={() => openSettings("settings-workers")} onRetry={resumeQueenReview} />
                <ConversationDriftCard workers={actionableConversationChecks} onOpenWorker={openWorkerProfile} />
                <ApiaryAttentionCard pendingAssistance={pendingAssistCount} onReview={() => setSurface("apiary")} />
                <HeldDeliveryAttentionCard
                  held={actionableHeldDeliveries}
                  workerIsAwake={(name) => Boolean(workers.find((candidate) => candidate.name === name)?.active_session_id)}
                  /* TWO BUGS LIVED ON ONE LINE HERE, and the card they broke is
                     the one that exists for a worker that never started.

                     It called openWorker(worker.id). openWorker takes a SESSION
                     id — every other call site passes active_session_id — so
                     this set the active session to an id no session has. And a
                     SLEEPING worker has no session at all, so even the right id
                     would have been undefined.

                     The card's own words are "Wake it yourself and it picks up
                     from there". The button could not wake anything. The
                     operator pressed it on two tasks routed to a sleeping Voice
                     Bridge worker and reported "there was an option in needs you
                     to open it and that did nothing either". It did nothing. */
                  onOpenWorker={(name) => {
                    const worker = workers.find((candidate) => candidate.name === name);
                    if (!worker) return;
                    if (worker.active_session_id) openWorker(worker.active_session_id);
                    else void startExistingWorker(worker);
                  }}
                />
              </>}
              /* BELOW THE REQUESTS, not above them. Queued briefings say of
                 themselves that nothing is wrong; they were rendering ahead of
                 questions the operator has to answer before work continues. */
              onOpenTask={(taskId) => { setTaskFocus((current) => ({ id: taskId, request: (current?.request ?? 0) + 1 })); setSurface("tasks"); }}
              onFetchActivity={readRecentActivity}
              onResolve={resolveInboxDecision}
              onAnswer={answerInboxDecision}
            />
          </div>
        ) : surface === "queues" ? (
          <Suspense fallback={<WorkspaceLoading label="queues" />}>
            <QueuesView
              coordinatorUnavailable={coordinatorUnavailable}
              tasks={tasks}
              workers={workers}
              heldBriefings={heldBriefings}
              blockedWaits={blockedEscalations}
              heldDeliveries={queuedDeliveryObservations}
              onOpenTask={(taskId) => { setTaskFocus((current) => ({ id: taskId, request: (current?.request ?? 0) + 1 })); setSurface("tasks"); }}
            />
          </Suspense>
        ) : surface === "tasks" ? (
          <Suspense fallback={<WorkspaceLoading label="task board" />}>
            <TaskBoard tasks={boardTasks} onCompletedPanelOpen={() => { if (operatorToken) loadSettledTasks(operatorToken); }} jiraTaskLinks={jiraTaskLinks} operatorToken={operatorToken} hiveIdentity={hiveIdentity} focusTaskId={taskFocus?.id} focusRequest={taskFocus?.request} composeRequest={taskComposeRequest} sessions={sessions} workers={workers} busy={busy} query={taskQuery} filter={taskFilter} source={taskSource} sort={taskSort} project={taskProject} worker={taskWorker} projects={taskProjects} onQueryChange={setTaskQuery} onFilterChange={setTaskFilter} onSourceChange={(value) => { setTaskSource(value); if (value === "email" || value === "local") setTaskProject("all"); }} onSortChange={setTaskSort} onProjectChange={setTaskProject} onWorkerChange={setTaskWorkerFilter} onJiraSync={() => void syncJiraBoard()} onCreate={addTask} onUpdate={editTask} onRemove={removeTaskFromHive} onRestore={restoreTaskToHive} onTransition={moveTask} onAssign={setTaskWorker} onStartWorker={startWorkerForTask} onOpenWorker={openWorker} onOpenTask={(taskId) => { setTaskFocus((current) => ({ id: taskId, request: (current?.request ?? 0) + 1 })); void refreshControlRoom(); }} onFetchActivity={(taskId) => fetchTaskActivity(operatorToken, taskId)} onFetchJiraComments={(taskId) => fetchJiraComments(operatorToken, taskId)} onAddJiraComment={(taskId, body) => addJiraComment(operatorToken, taskId, body)} onRetryJira={retryTaskJira} onJiraImported={refreshControlRoom} onEmailImported={refreshControlRoom} onReorder={reorderOpenTasks} />
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
              sharedMachineResources={sharedMachineResources}
              dogfoodCollection={dogfoodCollection}
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
              onChooseWorkerMark={chooseWorkerMark}
              onRemoveWorker={removeWorkerProfile}
              onReorderWorkers={reorderWorkerProfiles}
              onRestartProviders={restartProviders} onForceWorkerReload={forceWorkerReload}
              onUpdateWorkerEngine={maintainWorkerEngine}
              onReloadDevelopment={reloadDevelopmentBuild}
              onHiveIdentityChange={setHiveIdentity}
            />
          </Suspense>
        ) : activeSession ? (
          <TerminalLoadBoundary key={`${operatorToken}:${activeSession.session_id}:${terminalRevision}`}>
            <Suspense fallback={<div className="terminal-empty">Preparing terminal…</div>}>
              <TerminalView operatorToken={operatorToken} session={activeSession} busy={busy} canStop={activeWorker?.role !== "queen"} mobileKeysVisible={mobileKeysVisible} onMobileKeysVisibleChange={changeMobileKeysVisibility} onRefresh={reloadTerminalView} queenAutomation={activeWorker?.role === "queen" ? queenAutomation : undefined} queenAutonomy={activeWorker?.role === "queen" ? queenPolicy?.[presence?.mode ?? "at_hive"] : undefined} onOpenQueenSettings={activeWorker?.role === "queen" ? () => openSettings("settings-workers") : undefined} onConnectionStateChange={setTerminalConnection} />
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
function BroadcastIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 9v6h4l6 4V5L8 9H4Z"/><path d="M17.5 8.5a5 5 0 0 1 0 7"/></svg>; }
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
  if (task.state === "awaiting_release") return "Awaiting release";
  return task.state[0].toUpperCase() + task.state.slice(1);
}

function workerAttentionLabel(worker: Worker): string {
  return workerAttention(worker).label;
}

function presenceModeLabel(mode: PresenceMode) {
  if (mode === "at_hive") return "At Hive";
  if (mode === "night_watch") return "Night Watch";
  return "Reachable";
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
    : surface === "queues" ? "Queues"
    : surface === "tasks" ? "Tasks"
    : surface === "workers" ? "Workers"
    : surface === "apiary" ? "Apiary"
    : "Settings";
}
function QueuesIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 6h16M4 12h10M4 18h6"/></svg>; }
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
