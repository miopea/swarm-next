import {
  observePresence,
  type OperatorPresence,
  type PresenceDeviceClass,
  type PresenceObservationState,
} from "../api";

const DEVICE_ID_STORAGE_KEY = "swarm-next.presence-device.v1";
const HEARTBEAT_MS = 60_000;

type Observe = typeof observePresence;
export type LockDetectionState = "unsupported" | "available" | "enabling" | "enabled" | "denied" | "error";

export class PresenceController {
  #token?: string;
  #deviceId = "";
  #deviceClass: PresenceDeviceClass = "desktop";
  #state: PresenceObservationState = "hidden";
  #timer?: number;
  #lastActiveSentAt = 0;
  #sending = false;
  #pending?: PresenceObservationState;
  #pendingReturn = false;
  #nightWatch = false;
  #generation = 0;
  #observationRevision = 0;
  #policyRevision = 0;
  #lockRequest?: Promise<boolean>;
  #idleAbort?: AbortController;
  #screenLocked = false;
  #userIdle = false;
  #onPresence: (presence: OperatorPresence) => void = () => undefined;
  #onLockState: (state: LockDetectionState) => void = () => undefined;

  constructor(
    private readonly observe: Observe = observePresence,
    private readonly now: () => number = () => Date.now(),
  ) {}

  start(
    operatorToken: string,
    onPresence: (presence: OperatorPresence) => void,
    onLockState: (state: LockDetectionState) => void,
  ) {
    this.stop();
    this.#token = operatorToken;
    this.#onPresence = onPresence;
    this.#onLockState = onLockState;
    this.#deviceId = presenceDeviceId();
    this.#deviceClass = deviceClass();
    this.#state = document.visibilityState === "visible" ? "active" : "hidden";
    const lockDetectionAvailable = this.#deviceClass === "desktop" && "IdleDetector" in window;
    this.#onLockState(lockDetectionAvailable ? "available" : "unsupported");
    document.addEventListener("visibilitychange", this.#handleVisibility);
    window.addEventListener("pointerdown", this.#handleInteraction, { passive: true });
    window.addEventListener("keydown", this.#handleInteraction);
    window.addEventListener("touchstart", this.#handleInteraction, { passive: true });
    this.#queue(this.#state, this.#state === "active");
    this.#timer = window.setInterval(() => this.#queue(this.#state), HEARTBEAT_MS);
    if (lockDetectionAvailable) void this.#restoreLockDetection(this.#generation);
  }

  stop() {
    this.#generation += 1;
    this.#idleAbort?.abort();
    this.#idleAbort = undefined;
    this.#lockRequest = undefined;
    this.#screenLocked = false;
    this.#userIdle = false;
    if (this.#timer !== undefined) window.clearInterval(this.#timer);
    this.#timer = undefined;
    document.removeEventListener("visibilitychange", this.#handleVisibility);
    window.removeEventListener("pointerdown", this.#handleInteraction);
    window.removeEventListener("keydown", this.#handleInteraction);
    window.removeEventListener("touchstart", this.#handleInteraction);
    this.#token = undefined;
    this.#pending = undefined;
    this.#pendingReturn = false;
    this.#nightWatch = false;
    this.#sending = false;
  }

  enableLockDetection(): Promise<boolean> {
    if (this.#lockRequest) return this.#lockRequest;
    const generation = this.#generation;
    const pending = this.#enableLockDetection(generation).finally(() => {
      if (this.#lockRequest === pending) this.#lockRequest = undefined;
    });
    this.#lockRequest = pending;
    return pending;
  }

  async #enableLockDetection(generation: number): Promise<boolean> {
    if (!this.#token || this.#deviceClass !== "desktop" || !("IdleDetector" in window)) {
      this.#onLockState("unsupported");
      return false;
    }
    this.#onLockState("enabling");
    try {
      const permission = await IdleDetector.requestPermission();
      if (generation !== this.#generation || !this.#token) return false;
      if (permission !== "granted") {
        this.#onLockState("denied");
        return false;
      }
      return await this.#startIdleDetector();
    } catch {
      if (generation === this.#generation && !controllerAborted(this.#idleAbort)) this.#onLockState("error");
      return false;
    }
  }

  setPresenceMode(mode: OperatorPresence["mode"] | undefined) {
    this.#policyRevision += 1;
    this.#nightWatch = mode === "night_watch";
  }

  async #restoreLockDetection(generation: number) {
    try {
      const permission = await navigator.permissions?.query(
        { name: "idle-detection" } as unknown as PermissionDescriptor,
      );
      if (generation !== this.#generation || permission?.state !== "granted") return;
      await this.#startIdleDetector();
    } catch {
      // Permission restoration is optional. The explicit button remains available.
    }
  }

  async #startIdleDetector() {
    this.#idleAbort?.abort();
    const controller = new AbortController();
    this.#idleAbort = controller;
    const detector = new IdleDetector();
    const update = () => {
      if (controller.signal.aborted) return;
      this.#screenLocked = detector.screenState === "locked";
      this.#userIdle = detector.userState === "idle";
      this.#state = this.#screenLocked
        ? "locked"
        : this.#userIdle
          ? "idle"
          : document.visibilityState === "visible" ? "active" : "hidden";
      this.#queue(this.#state);
    };
    detector.addEventListener("change", update, { signal: controller.signal });
    try {
      await detector.start({ threshold: 60_000, signal: controller.signal });
    } catch (error) {
      if (controller.signal.aborted) return false;
      throw error;
    }
    if (controller.signal.aborted) return false;
    this.#onLockState("enabled");
    update();
    return true;
  }

  #handleVisibility = () => {
    this.#state = this.#screenLocked ? "locked"
      : this.#userIdle ? "idle"
        : document.visibilityState === "visible" ? "active" : "hidden";
    this.#queue(this.#state, this.#state === "active");
  };

  #handleInteraction = () => {
    if (document.visibilityState !== "visible" || this.#state === "locked") return;
    this.#state = "active";
    if (this.#nightWatch || this.now() - this.#lastActiveSentAt >= HEARTBEAT_MS) this.#queue("active", true);
  };

  #queue(state: PresenceObservationState, desktopReturn = false) {
    if (!this.#token) return;
    this.#observationRevision += 1;
    this.#pending = state;
    this.#pendingReturn = state === "active" && this.#deviceClass === "desktop" && (desktopReturn || this.#pendingReturn);
    if (!this.#sending) void this.#flush();
  }

  async #flush() {
    this.#sending = true;
    const generation = this.#generation;
    while (this.#pending && this.#token && generation === this.#generation) {
      const state = this.#pending;
      const desktopReturn = this.#pendingReturn;
      this.#pendingReturn = false;
      this.#pending = undefined;
      const token = this.#token;
      const observationRevision = this.#observationRevision;
      const policyRevision = this.#policyRevision;
      try {
        const presence = await this.observe(token, this.#deviceId, this.#deviceClass, state, desktopReturn);
        if (generation !== this.#generation) return;
        // A newer device observation or independently read/manual mode owns the UI.
        // Keep flushing the latest pending state without publishing old evidence.
        if (observationRevision !== this.#observationRevision || policyRevision !== this.#policyRevision) continue;
        if (state === "active") this.#lastActiveSentAt = this.now();
        this.#nightWatch = presence.mode === "night_watch";
        this.#onPresence(presence);
      } catch {
        // The next explicit event or bounded heartbeat retries. Presence expires safely server-side.
      }
    }
    if (generation === this.#generation) this.#sending = false;
  }
}

export function presenceDeviceId(): string {
  try {
    const saved = window.localStorage.getItem(DEVICE_ID_STORAGE_KEY);
    if (saved) return saved;
    const created = crypto.randomUUID();
    window.localStorage.setItem(DEVICE_ID_STORAGE_KEY, created);
    return created;
  } catch {
    return crypto.randomUUID();
  }
}

export function deviceClass(): PresenceDeviceClass {
  return /Android|iPhone|iPad|Mobile/i.test(navigator.userAgent) ? "mobile" : "desktop";
}

function controllerAborted(controller: AbortController | undefined) {
  return controller?.signal.aborted ?? false;
}
