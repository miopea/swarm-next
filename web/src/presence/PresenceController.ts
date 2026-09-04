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
  #generation = 0;
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
    this.#queue(this.#state);
    this.#timer = window.setInterval(() => this.#queue(this.#state), HEARTBEAT_MS);
    if (lockDetectionAvailable) void this.#restoreLockDetection(this.#generation);
  }

  stop() {
    this.#generation += 1;
    this.#idleAbort?.abort();
    this.#idleAbort = undefined;
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
    this.#sending = false;
  }

  async enableLockDetection(): Promise<boolean> {
    if (!this.#token || this.#deviceClass !== "desktop" || !("IdleDetector" in window)) {
      this.#onLockState("unsupported");
      return false;
    }
    this.#onLockState("enabling");
    try {
      const permission = await IdleDetector.requestPermission();
      if (permission !== "granted") {
        this.#onLockState("denied");
        return false;
      }
      await this.#startIdleDetector();
      return true;
    } catch {
      if (!controllerAborted(this.#idleAbort)) this.#onLockState("error");
      return false;
    }
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
    await detector.start({ threshold: 60_000, signal: controller.signal });
    if (controller.signal.aborted) return;
    this.#onLockState("enabled");
    update();
  }

  #handleVisibility = () => {
    this.#state = this.#screenLocked ? "locked"
      : this.#userIdle ? "idle"
        : document.visibilityState === "visible" ? "active" : "hidden";
    this.#queue(this.#state);
  };

  #handleInteraction = () => {
    if (document.visibilityState !== "visible" || this.#state === "locked") return;
    this.#state = "active";
    if (this.now() - this.#lastActiveSentAt >= HEARTBEAT_MS) this.#queue("active");
  };

  #queue(state: PresenceObservationState) {
    if (!this.#token) return;
    this.#pending = state;
    if (!this.#sending) void this.#flush();
  }

  async #flush() {
    this.#sending = true;
    const generation = this.#generation;
    while (this.#pending && this.#token && generation === this.#generation) {
      const state = this.#pending;
      this.#pending = undefined;
      const token = this.#token;
      try {
        const presence = await this.observe(token, this.#deviceId, this.#deviceClass, state);
        if (generation !== this.#generation) return;
        if (state === "active") this.#lastActiveSentAt = this.now();
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
