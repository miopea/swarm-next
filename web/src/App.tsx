import { lazy, Suspense, useEffect, useMemo, useState, type FormEvent, type KeyboardEvent as ReactKeyboardEvent } from "react";

import {
  assignTask,
  BROWSER_SESSION_AUTH,
  createBrowserSession,
  createTask,
  fetchDecisions,
  resolveDecision,
  createWorker,
  fetchHive,
  fetchNotificationSettings,
  fetchQueenAutonomyPolicy,
  fetchPresence,
  fetchProviderCapabilities,
  fetchPresentationPreferences,
  fetchSessions,
  fetchTaskActivity,
  fetchTasks,
  fetchWorkers,
  fetchWorkspaces,
  reorderTasks,
  reorderWorkers,
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
  validateBrowserSession,
  type ControlRoomEvent,
  type DecisionRequest,
  type Health,
  type HiveIdentity,
  type NotificationPolicy,
  type NotificationSettings,
  type OperatorPresence,
  type PresenceMode,
  type ProviderKind,
  type ProviderCapabilities,
  type PresentationDeviceClass,
  type QueenAutonomyPolicy,
  type SessionSummary,
  type Task,
  type TaskDraftInput,
  type TaskState,
  type TaskUpdateInput,
  type Worker,
  type WorkspaceChoice,
} from "./api";
import BeeMascot from "./brand/BeeMascot";
import DecisionInbox from "./decisions/DecisionInbox";
import DogfoodFeedbackDialog from "./feedback/DogfoodFeedbackDialog";
import CommandPalette, { type CommandChoice } from "./navigation/CommandPalette";
import { applyColorTheme, initialColorTheme, type ColorTheme } from "./brand/theme";
import { ControlRoomLiveFeed, type LiveFeedState } from "./controlRoom/ControlRoomLiveFeed";
import SettingsWorkspace from "./settings/SettingsWorkspace";
import { PresenceController, deviceClass, presenceDeviceId, type LockDetectionState } from "./presence/PresenceController";
import { NotificationController, type NotificationCapabilityState } from "./notifications/NotificationController";
import TaskBoard, { workerName } from "./tasks/TaskBoard";
import TerminalLoadBoundary from "./terminal/TerminalLoadBoundary";
import { initialMobileKeysVisibility, rememberMobileKeysVisibility } from "./terminal/MobileTerminalComposer";
import { terminalWorkspace } from "./terminal/TerminalWorkspace";
import WorkerRosterItem from "./workers/WorkerRosterItem";

const loadTerminalView = () => import("./terminal/TerminalView");
const TerminalView = lazy(loadTerminalView);
const SURFACE_STORAGE_KEY = "swarm-next.surface.v1";
const ACTIVE_SESSION_STORAGE_KEY = "swarm-next.active-session.v1";

type LoadState = { kind: "loading" } | { kind: "ready"; health: Health } | { kind: "unavailable" };
type Surface = "decisions" | "tasks" | "workers" | "settings";

export function App() {
  const [loadState, setLoadState] = useState<LoadState>({ kind: "loading" });
  const [tokenDraft, setTokenDraft] = useState("");
  const [operatorToken, setOperatorToken] = useState<string>();
  const [hiveIdentity, setHiveIdentity] = useState<HiveIdentity>();
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [workers, setWorkers] = useState<Worker[]>([]);
  const [workspaces, setWorkspaces] = useState<WorkspaceChoice[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string>();
  const [decisions, setDecisions] = useState<DecisionRequest[]>([]);
  const [showFeedback, setShowFeedback] = useState(false);
  const [feedbackRevision, setFeedbackRevision] = useState(0);
  const [showCommands, setShowCommands] = useState(false);
  const [surface, setSurface] = useState<Surface>(readSavedSurface);
  const [taskFocus, setTaskFocus] = useState<{ id: string; request: number }>();
  const [taskComposeRequest, setTaskComposeRequest] = useState(0);
  const [decisionFocus, setDecisionFocus] = useState<{ id: string; request: number }>();
  const [operationError, setOperationError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [colorTheme, setColorTheme] = useState<ColorTheme>(initialColorTheme);
  const [mobileKeysVisible, setMobileKeysVisible] = useState(initialMobileKeysVisibility);
  const [liveFeedState, setLiveFeedState] = useState<LiveFeedState>("connecting");
  const [recentEvents, setRecentEvents] = useState<ControlRoomEvent[]>([]);
  const [presence, setPresence] = useState<OperatorPresence>();
  const [lockDetectionState, setLockDetectionState] = useState<LockDetectionState>("unsupported");
  const [notificationSettings, setNotificationSettings] = useState<NotificationSettings>();
  const [queenPolicy, setQueenPolicy] = useState<QueenAutonomyPolicy>();
  const [providers, setProviders] = useState<ProviderCapabilities>({ claude_code: true, codex: false });
  const [notificationState, setNotificationState] = useState<NotificationCapabilityState>("unsupported");
  const presenceController = useMemo(() => new PresenceController(), []);
  const notificationController = useMemo(() => new NotificationController(), []);
  const presentationDevice = useMemo<PresentationDeviceClass>(() => deviceClass(), []);

  useEffect(() => applyColorTheme(colorTheme), [colorTheme]);
  useEffect(() => saveSurface(surface), [surface]);
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
    presenceController.start(operatorToken, setPresence, setLockDetectionState);
    return () => presenceController.stop();
  }, [operatorToken, presenceController]);

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
      return;
    }
    void fetchQueenAutonomyPolicy(operatorToken)
      .then(setQueenPolicy)
      .catch((error: unknown) => setOperationError(error instanceof Error ? error.message : "Queen policy could not be loaded"));
  }, [operatorToken]);

  useEffect(() => {
    if (!operatorToken) return;
    void fetchProviderCapabilities(operatorToken)
      .then(setProviders)
      .catch(() => setProviders({ claude_code: true, codex: false }));
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
    const controller = new AbortController();
    void fetch("/health", { signal: controller.signal })
      .then((response) => {
        if (!response.ok) throw new Error(`Health returned ${response.status}`);
        return response.json() as Promise<Health>;
      })
      .then((health) => setLoadState({ kind: "ready", health }))
      .catch((error: unknown) => {
        if (!(error instanceof DOMException && error.name === "AbortError")) setLoadState({ kind: "unavailable" });
      });
    return () => controller.abort();
  }, []);

  useEffect(() => {
    let cancelled = false;
    void validateBrowserSession()
      .then(() => loadControlRoom(BROWSER_SESSION_AUTH))
      .then(({ hive, sessions: nextSessions, workers: nextWorkers, workspaces: nextWorkspaces, tasks: nextTasks, decisions: nextDecisions }) => {
        if (cancelled) return;
        terminalWorkspace.authenticate(BROWSER_SESSION_AUTH);
        setOperatorToken(BROWSER_SESSION_AUTH);
        setHiveIdentity(hive);
        setSessions(nextSessions);
        setWorkers(nextWorkers);
        setWorkspaces(nextWorkspaces);
        setTasks(nextTasks);
        setActiveSessionId(restoredSessionId(nextWorkers, nextSessions));
        setDecisions(nextDecisions);
      })
      .catch((error: unknown) => {
        if (cancelled) return;
        terminalWorkspace.logout();
        if (!(error instanceof Error && error.message.includes("401"))) {
          setOperationError(error instanceof Error ? error.message : "Saved authentication could not be restored");
        }
      })
    return () => { cancelled = true; };
  }, []);

  useEffect(() => {
    if (!operatorToken) {
      setLiveFeedState("connecting");
      return;
    }
    let cancelled = false;
    const feed = new ControlRoomLiveFeed();
    feed.start(
      operatorToken,
      async (page) => {
        const runtimeChanged = page.events.some((event) => event.kind === "runtime_changed");
        const [controlRoom, refreshedPresence, refreshedNotifications, refreshedQueenPolicy, refreshedPresentation, refreshedProviders] = await Promise.all([
          loadControlRoom(operatorToken),
          page.events.some((event) => event.kind === "presence_changed")
            ? fetchPresence(operatorToken)
            : Promise.resolve(undefined),
          page.events.some((event) => event.kind === "notifications_changed")
            ? fetchNotificationSettings(operatorToken)
            : Promise.resolve(undefined),
          runtimeChanged
            ? fetchQueenAutonomyPolicy(operatorToken)
            : Promise.resolve(undefined),
          runtimeChanged
            ? fetchPresentationPreferences(operatorToken, presentationDevice)
            : Promise.resolve(undefined),
          runtimeChanged
            ? fetchProviderCapabilities(operatorToken)
            : Promise.resolve(undefined),
        ]);
        if (cancelled) return;
        setHiveIdentity(controlRoom.hive);
        setSessions(controlRoom.sessions);
        setWorkers(controlRoom.workers);
        setWorkspaces(controlRoom.workspaces);
        setTasks(controlRoom.tasks);
        setDecisions(controlRoom.decisions);
        if (refreshedPresence) setPresence(refreshedPresence);
        if (refreshedNotifications) setNotificationSettings(refreshedNotifications);
        if (refreshedQueenPolicy) setQueenPolicy(refreshedQueenPolicy);
        if (refreshedProviders) setProviders(refreshedProviders);
        if (refreshedPresentation?.configured) {
          setColorTheme(refreshedPresentation.color_theme);
          setMobileKeysVisible(refreshedPresentation.terminal_keys_visible);
          rememberMobileKeysVisibility(refreshedPresentation.terminal_keys_visible);
        }
        setRecentEvents((current) => page.reset_required
          ? page.events.slice(-16)
          : [...current, ...page.events].filter((event, index, events) =>
              events.findIndex((candidate) => candidate.sequence === event.sequence) === index,
            ).slice(-16));
        setActiveSessionId((current) =>
          current && controlRoom.sessions.some((session) => session.session_id === current)
            ? current
            : preferredSessionId(controlRoom.workers, controlRoom.sessions),
        );
      },
      setLiveFeedState,
    );
    return () => {
      cancelled = true;
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
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setWorkspaces(controlRoom.workspaces);
      setTasks(controlRoom.tasks);
      setActiveSessionId((current) => current ?? preferredSessionId(controlRoom.workers, controlRoom.sessions));
      setDecisions(controlRoom.decisions);
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
  async function refreshControlRoom() {
    if (!operatorToken) return;
    await perform(async () => {
      const controlRoom = await loadControlRoom(operatorToken);
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setWorkspaces(controlRoom.workspaces);
      setTasks(controlRoom.tasks);
      setDecisions(controlRoom.decisions);
      setActiveSessionId((current) =>
        current && controlRoom.sessions.some((session) => session.session_id === current)
          ? current
          : preferredSessionId(controlRoom.workers, controlRoom.sessions),
      );
    });
  }

  async function configureWorker(name: string, workspace: string, provider: ProviderKind) {
    if (!operatorToken) return;
    await perform(async () => {
      await createWorker(operatorToken, { name, workspace, provider });
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

  async function maintainWorkerProfile(workerId: string, name: string, autostart: boolean) {
    if (!operatorToken) return;
    await perform(async () => {
      const updated = await updateWorker(operatorToken, workerId, { name, autostart });
      setWorkers((current) => current.map((worker) => worker.id === updated.id ? updated : worker));
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
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setTasks(controlRoom.tasks);
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
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setTasks(controlRoom.tasks);
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
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setTasks(controlRoom.tasks);
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

  async function moveTask(task: Task, state: TaskState) {
    if (!operatorToken) return;
    await perform(async () => {
      const updated = await transitionTask(operatorToken, task.id, state);
      replaceTask(updated);
    });
  }

  async function setTaskWorker(task: Task, workerId: string) {
    if (!operatorToken) return;
    await perform(async () => {
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

  async function resolveInboxDecision(decision: DecisionRequest, action: string, note: string) {
    if (!operatorToken) return;
    await perform(async () => {
      const updated = await resolveDecision(operatorToken, decision.id, action, note);
      setDecisions((current) => current.map((item) => item.id === updated.id ? updated : item));
    });
  }
  function replaceTask(updated: Task) {
    setTasks((current) => current.map((task) => task.id === updated.id ? updated : task));
  }

  async function perform(action: () => Promise<void>) {
    setBusy(true);
    setOperationError(undefined);
    try {
      await action();
    } catch (error) {
      setOperationError(error instanceof Error ? error.message : "The operation could not be completed");
    } finally {
      setBusy(false);
    }
  }

  function handleShortcut(event: ReactKeyboardEvent<HTMLElement>) {
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
    terminalWorkspace.focusSession(sessionId, shouldFocusTerminalInput());
  }

  async function maintainWorkerEngine() {
    if (!operatorToken) return;
    const previousSessionIds = sessions.map((session) => session.session_id);
    await perform(async () => {
      await updateWorkerEngine(operatorToken);
      previousSessionIds.forEach((sessionId) => terminalWorkspace.closeSession(sessionId));
      const [controlRoom, nextProviders] = await Promise.all([
        loadControlRoom(operatorToken),
        fetchProviderCapabilities(operatorToken),
      ]);
      setHiveIdentity(controlRoom.hive);
      setSessions(controlRoom.sessions);
      setWorkers(controlRoom.workers);
      setWorkspaces(controlRoom.workspaces);
      setTasks(controlRoom.tasks);
      setDecisions(controlRoom.decisions);
      setProviders(nextProviders);
      setActiveSessionId(preferredSessionId(controlRoom.workers, controlRoom.sessions));
    });
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
    setHiveIdentity(undefined);
    setSessions([]);
    setWorkers([]);
    setTasks([]);
    setDecisions([]);
    setNotificationSettings(undefined);
    setNotificationState("unsupported");
    setActiveSessionId(undefined);
    setOperationError(undefined);
  }

  const activeSession = sessions.find((session) => session.session_id === activeSessionId);
  const activeWorker = workers.find((worker) => worker.active_session_id === activeSessionId);
  const openTaskCount = tasks.filter((task) => task.state !== "completed").length;
  const pendingDecisionCount = decisions.filter((decision) => decision.state === "pending").length;
  const orphanSessions = useMemo(
    () => sessions.filter((session) => session.running && !workers.some((worker) => worker.active_session_id === session.session_id)),
    [sessions, workers],
  );
  const tasksBySession = useMemo(
    () => new Map(
      tasks
        .filter((task) => task.assigned_session_id && task.state !== "completed")
        .map((task) => [task.assigned_session_id, task]),
    ),
    [tasks],
  );
  const activeTask = activeSession ? tasksBySession.get(activeSession.session_id) : undefined;
  const commandChoices = useMemo<CommandChoice[]>(() => [
    { id: "decisions", label: "Needs you", detail: `${pendingDecisionCount} pending`, group: "Go to", run: () => setSurface("decisions") },
    { id: "tasks", label: "Tasks", detail: `${openTaskCount} open`, group: "Go to", run: () => setSurface("tasks") },
    { id: "new-task", label: "Create task", detail: "Plan work for a worker", group: "Go to", run: () => { setTaskComposeRequest((current) => current + 1); setSurface("tasks"); } },
    { id: "workers", label: "Workers", detail: `${workers.filter((worker) => worker.running).length} running`, group: "Go to", run: () => setSurface("workers") },
    { id: "add-worker", label: "Add worker", detail: "Configure a repository worker", group: "Go to", run: () => setSurface("settings") },
    { id: "settings", label: "Settings", detail: "Preferences and diagnostics", group: "Go to", run: () => setSurface("settings") },
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
      detail: `${taskStateLabel(task.state)} · ${workers.find((worker) => worker.id === task.assigned_worker_id)?.name ?? "Unassigned"}`,
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
  ], [openTaskCount, pendingDecisionCount, workers, tasks, decisions, activeSessionId, operatorToken]);

  useEffect(() => {
    if (surface !== "workers" || !activeSessionId) return;
    const frame = requestAnimationFrame(() => {
      terminalWorkspace.focusSession(activeSessionId, shouldFocusTerminalInput());
    });
    return () => cancelAnimationFrame(frame);
  }, [surface, activeSessionId]);

  return (
    <main className="app-shell" onKeyDown={handleShortcut}>
      <aside className={`control-rail surface-${surface}`} aria-label="Swarm navigation">
        <div className="brand-lockup">
          <div className="brand-mark"><BeeMascot expression="available" /></div>
          <div><p className="eyebrow">Swarm Next</p><h1>Control room</h1></div>
        </div>

        {operatorToken ? (
          <>
            <nav className="surface-nav" aria-label="Primary">
              <button className={surface === "decisions" ? "selected" : ""} aria-current={surface === "decisions" ? "page" : undefined} onClick={() => setSurface("decisions")}>
                <span><DecisionIcon /> Needs you</span><small>{pendingDecisionCount}</small>
              </button>
              <button className={surface === "tasks" ? "selected" : ""} aria-current={surface === "tasks" ? "page" : undefined} onClick={() => setSurface("tasks")}>
                <span><TaskIcon /> Tasks</span><small>{openTaskCount}</small>
              </button>
              <button className={surface === "workers" ? "selected" : ""} aria-current={surface === "workers" ? "page" : undefined} onClick={() => setSurface("workers")}>
                <span><TerminalIcon /> Workers</span><small>{workers.filter((worker) => worker.running).length + orphanSessions.filter((session) => session.running).length}</small>
              </button>
              <button className={surface === "settings" ? "selected" : ""} aria-current={surface === "settings" ? "page" : undefined} onClick={() => setSurface("settings")}>
                <span><SettingsIcon /> Settings</span>
              </button>
            </nav>

            {surface !== "settings" && surface !== "decisions" && <div className="rail-context">
              <div className="rail-heading"><span>{surface === "tasks" ? "Open tasks" : "Live sessions"}</span></div>
              {surface === "tasks" ? (
                tasks.filter((task) => task.state !== "completed").length === 0 ? <p className="empty-rail">Nothing queued yet.</p> :
                  <div className="mini-task-list">{tasks.filter((task) => task.state !== "completed").slice(0, 8).map((task) => <div key={task.id}><span className={`state-dot state-${task.state}`} /><span>{task.title}</span></div>)}</div>
              ) : workers.length === 0 && orphanSessions.length === 0 ? (
                <p className="empty-rail">No workers configured.</p>
              ) : (
                <div className="worker-list">
                  {workers.map((worker) => {
                    const sessionId = worker.active_session_id;
                    const task = sessionId ? tasksBySession.get(sessionId) : undefined;
                    return (
                      <WorkerRosterItem
                        key={worker.id}
                        worker={worker}
                        selected={sessionId === activeSessionId}
                        detail={worker.runtime_error ?? task?.title ?? (worker.role === "queen" ? "Always-active command terminal" : worker.running ? `${repositoryName(worker.workspace)} · Ready for work` : `${repositoryName(worker.workspace)} · Sleeping`)}
                        busy={busy}
                        onOpen={() => sessionId && openWorker(sessionId)}
                        onStart={() => void startExistingWorker(worker)}
                        onStop={() => sessionId && void stopSession(sessionId)}
                      />
                    );
                  })}
                  {orphanSessions.map((session) => {
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
                <button type="button" onClick={() => setSurface("settings")}>Manage workers</button>
              </div>
            )}
          </>
        ) : <p className="empty-rail">Unlock this runtime to access tasks and workers.</p>}

        <div className="rail-footer"><RuntimeStatus state={loadState} /></div>
      </aside>

      <section className="workspace">
        <header className="workspace-header">
          <div>
            <p className="eyebrow">{surface === "decisions" ? "Attention without interruption" : surface === "tasks" ? "Plan and dispatch" : surface === "settings" ? "Preferences and diagnostics" : activeTask?.title ?? "Persistent terminal"}</p>
            <h2>{surface === "decisions" ? "Needs you" : surface === "tasks" ? "Task board" : surface === "settings" ? "Settings" : activeSession ? activeWorker?.name ?? workerName(activeSession.session_id) : "Worker terminal"}</h2>
          </div>
          <div className="header-actions">
            {busy && <span className="saving-state">Saving…</span>}
            {operatorToken && presence && <span className={`operator-presence-chip ${presence.mode}`} title={`Operator presence: ${presenceModeLabel(presence.mode)}`}><span className="state-dot" /><span>{presenceModeLabel(presence.mode)}</span></span>}
            {operatorToken && <button className="icon-button feedback-button" aria-label="Report a problem" onClick={() => setShowFeedback(true)}><FeedbackIcon /></button>}
            {operatorToken && <button className="icon-button command-button" aria-label="Open quick navigation" onClick={() => setShowCommands(true)}><CommandIcon /></button>}
            <button className="icon-button" aria-label={`Switch to ${colorTheme === "light" ? "dark" : "light"} theme`} onClick={() => changeColorTheme(colorTheme === "light" ? "dark" : "light")}><ThemeIcon theme={colorTheme} /></button>
            {operatorToken && <button className="icon-button refresh-button" aria-label="Refresh control room" onClick={() => void refreshControlRoom()} disabled={busy}><RefreshIcon /></button>}
            {operatorToken && <button className="secondary-button" onClick={() => void logout()} disabled={busy}>Lock</button>}
          </div>
        </header>
        {operationError && <div className="operation-error" role="alert">{operationError}</div>}
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
          </form>
        ) : surface === "decisions" ? (
          <DecisionInbox decisions={decisions} tasks={tasks} workers={workers} busy={busy} focusDecisionId={decisionFocus?.id} focusRequest={decisionFocus?.request} onResolve={resolveInboxDecision} />
        ) : surface === "tasks" ? (
          <TaskBoard tasks={tasks} focusTaskId={taskFocus?.id} focusRequest={taskFocus?.request} composeRequest={taskComposeRequest} sessions={sessions} workers={workers} busy={busy} onCreate={addTask} onUpdate={editTask} onTransition={moveTask} onAssign={setTaskWorker} onStartWorker={startWorkerForTask} onFetchActivity={(taskId) => fetchTaskActivity(operatorToken, taskId)} onReorder={reorderOpenTasks} />
        ) : surface === "settings" ? (
          <SettingsWorkspace
            busy={busy}
            colorTheme={colorTheme}
            feedbackRevision={feedbackRevision}
            hiveIdentity={hiveIdentity}
            liveFeedState={liveFeedState}
            health={loadState.kind === "ready" ? loadState.health : undefined}
            operatorToken={operatorToken}
            recentEvents={recentEvents}
            presence={presence}
            lockDetectionState={lockDetectionState}
            notificationSettings={notificationSettings}
            queenPolicy={queenPolicy}
            providers={providers}
            notificationState={notificationState}
            sessions={sessions}
            workers={workers}
            workspaces={workspaces}
            onThemeChange={changeColorTheme}
            onPresenceChange={changePresenceMode}
            onEnableLockDetection={enableLockDetection}
            onNotificationPolicyChange={changeNotificationPolicy}
            onQueenPolicyChange={changeQueenPolicy}
            onEnableNotifications={enableNotifications}
            onDisableNotifications={disableNotifications}
            onTestNotification={testNotification}
            onCreateWorker={configureWorker}
            onUpdateWorker={maintainWorkerProfile}
            onReorderWorkers={reorderWorkerProfiles}
            onUpdateWorkerEngine={maintainWorkerEngine}
          />
        ) : activeSession ? (
          <TerminalLoadBoundary key={`${operatorToken}:${activeSession.session_id}`}>
            <Suspense fallback={<div className="terminal-empty">Preparing terminal…</div>}>
              <TerminalView operatorToken={operatorToken} session={activeSession} onStop={() => void stopSession(activeSession.session_id)} busy={busy} canStop={activeWorker?.role !== "queen"} mobileKeysVisible={mobileKeysVisible} onMobileKeysVisibleChange={changeMobileKeysVisibility} />
            </Suspense>
          </TerminalLoadBoundary>
        ) : (
          <div className="terminal-empty"><BeeMascot className="empty-bee" expression="sleeping" /><p className="eyebrow">No active session</p><h3>Start with a task or workspace</h3><p>Launch Claude from a ready task to preserve its assignment, or start an unassigned worker from the sidebar.</p></div>
        )}
      </section>
    </main>
  );
}

async function loadControlRoom(operatorToken: string) {
  const [hive, sessions, workers, workspaces, tasks, decisions] = await Promise.all([
    fetchHive(operatorToken),
    fetchSessions(operatorToken),
    fetchWorkers(operatorToken),
    fetchWorkspaces(operatorToken),
    fetchTasks(operatorToken),
    fetchDecisions(operatorToken),
  ]);
  return { hive, sessions, workers, workspaces, tasks, decisions };
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

function DecisionIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3a7 7 0 0 0-7 7v3l-2 3h18l-2-3v-3a7 7 0 0 0-7-7ZM9 20h6"/></svg>; }
function FeedbackIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 5h16v11H9l-5 4V5Z"/><path d="M12 8v4M12 14h.01"/></svg>; }
function CommandIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="11" cy="11" r="6"/><path d="m16 16 4 4M8 11h6M11 8v6"/></svg>; }
function requireActiveSession(worker: Worker): string {
  if (!worker.active_session_id) throw new Error(`${worker.name} did not receive a terminal session`);
  return worker.active_session_id;
}

function repositoryName(workspace: string): string {
  return workspace.split(/[\\/]/).filter(Boolean).at(-1) ?? workspace;
}

function taskStateLabel(state: Task["state"]): string {
  if (state === "active") return "In progress";
  return state[0].toUpperCase() + state.slice(1);
}

function presenceModeLabel(mode: PresenceMode) {
  if (mode === "at_hive") return "At Hive";
  if (mode === "night_watch") return "Night Watch";
  return "Away";
}

function RuntimeStatus({ state }: { state: LoadState }) {
  if (state.kind === "ready") return <span className="runtime-status"><span className="presence online" /> Runtime {state.health.version}</span>;
  if (state.kind === "unavailable") return <span className="runtime-status error"><span className="presence offline" /> Runtime unavailable</span>;
  return <span className="runtime-status"><span className="presence" /> Connecting…</span>;
}

function TaskIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01" /></svg>; }
function TerminalIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m5 7 4 4-4 4M11 17h8" /></svg>; }
function readSavedSurface(): Surface { try { const linked = new URLSearchParams(window.location.search).get("surface"); if (linked === "decisions" || linked === "tasks" || linked === "workers" || linked === "settings") return linked; const saved = window.sessionStorage.getItem(SURFACE_STORAGE_KEY); return saved === "decisions" || saved === "workers" || saved === "settings" ? saved : "tasks"; } catch { return "tasks"; } }
function saveSurface(surface: Surface) { try { window.sessionStorage.setItem(SURFACE_STORAGE_KEY, surface); } catch { /* Surface persistence is a non-critical convenience. */ } }
function RefreshIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 6v5h-5M4 18v-5h5M6.1 9a7 7 0 0 1 11.4-2.4L20 9M4 15l2.5 2.4A7 7 0 0 0 17.9 15" /></svg>; }
function SettingsIcon() { return <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1A1.7 1.7 0 0 0 9 4.6a1.7 1.7 0 0 0 1-1.6v-.2h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z"/></svg>; }
function ThemeIcon({ theme }: { theme: ColorTheme }) { return theme === "light" ? <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3v2m0 14v2M3 12h2m14 0h2M5.6 5.6 7 7m10 10 1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4"/><circle cx="12" cy="12" r="4"/></svg> : <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 15.5A8 8 0 0 1 8.5 4 8.5 8.5 0 1 0 20 15.5Z"/></svg>; }

function isTypingTarget(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) || Boolean(target.closest("[role='menu']")));
}

function shouldFocusTerminalInput(): boolean {
  return !window.matchMedia?.("(pointer: coarse)").matches;
}
