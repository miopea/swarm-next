import {
  fetchNotificationSettings,
  fetchNotificationSubscriptionStatus,
  recoverTransientRuntime,
  removeNotificationSubscription,
  saveNotificationSubscription,
  sendTestNotification,
  setNotificationPolicy,
  type NotificationPolicy,
  type NotificationSettings,
} from "../api";
import { deviceClass, presenceDeviceId } from "../presence/PresenceController";

export type NotificationCapabilityState = "unsupported" | "available" | "enabling" | "enabled" | "denied" | "error";

const ENABLED_INTENT_KEY = "swarm-next.notifications.enabled.v1";

type SettingsCallback = (settings: NotificationSettings) => void;
type StateCallback = (state: NotificationCapabilityState) => void;

export class NotificationController {
  #token?: string;
  #settings?: NotificationSettings;
  #generation = 0;
  #onSettings: SettingsCallback = () => undefined;
  #onState: StateCallback = () => undefined;

  async start(operatorToken: string, onSettings: SettingsCallback, onState: StateCallback) {
    this.stop();
    const generation = this.#generation;
    this.#token = operatorToken;
    this.#onSettings = onSettings;
    this.#onState = onState;
    if (!supportsPush()) this.#onState("unsupported");
    else if (Notification.permission === "denied") this.#onState("denied");
    else this.#onState("available");
    try {
      const settings = await recoverTransientRuntime(() => fetchNotificationSettings(operatorToken));
      if (generation !== this.#generation) return;
      this.#publish(settings);
      if (!supportsPush()) return;
      const registration = await navigator.serviceWorker.getRegistration("/");
      if (registration) {
        try {
          await registration.update();
        } catch {
          // An existing push subscription remains usable while temporarily offline.
        }
      }
      let subscription = await registration?.pushManager.getSubscription();
      if (subscription) {
        const deviceId = presenceDeviceId();
        const status = await recoverTransientRuntime(() =>
          fetchNotificationSubscriptionStatus(operatorToken, deviceId));
        if (!status.registered && registration) {
          // The push service rejected this endpoint and the API removed it.
          // Chromium can retain that same dead subscription indefinitely, so
          // explicitly rotate it before registering this device again.
          await subscription.unsubscribe();
          subscription = await registration.pushManager.subscribe({
            userVisibleOnly: true,
            applicationServerKey: decodeUrlSafeBase64(settings.vapid_public_key),
          });
        }
        await this.#save(subscription);
        if (generation === this.#generation) {
          rememberEnabledIntent(true);
          this.#onState("enabled");
        }
        return;
      }
      if (generation === this.#generation && Notification.permission === "granted" && enabledIntentRemembered()) {
        await this.enable();
      }
    } catch {
      if (generation === this.#generation) this.#onState("error");
    }
  }

  stop() {
    this.#generation += 1;
    this.#token = undefined;
    this.#settings = undefined;
  }

  async enable(): Promise<boolean> {
    if (!this.#token || !this.#settings || !supportsPush()) {
      this.#onState("unsupported");
      return false;
    }
    this.#onState("enabling");
    try {
      const permission = await Notification.requestPermission();
      if (permission !== "granted") {
        this.#onState("denied");
        return false;
      }
      const registration = await navigator.serviceWorker.register("/sw.js", { scope: "/", updateViaCache: "none" });
      const existing = await registration.pushManager.getSubscription();
      const subscription = existing ?? await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: decodeUrlSafeBase64(this.#settings.vapid_public_key),
      });
      await this.#save(subscription);
      rememberEnabledIntent(true);
      this.#onState("enabled");
      return true;
    } catch {
      if (await this.#reconcileExistingSubscription()) return true;
      this.#onState("error");
      return false;
    }
  }

  async disable(): Promise<void> {
    if (!this.#token) return;
    rememberEnabledIntent(false);
    const registration = supportsPush() ? await navigator.serviceWorker.getRegistration("/") : undefined;
    const subscription = await registration?.pushManager.getSubscription();
    if (subscription) await subscription.unsubscribe();
    const settings = await recoverTransientRuntime(() => removeNotificationSubscription(this.#token!, presenceDeviceId()));
    this.#publish(settings);
    this.#onState(supportsPush() && Notification.permission !== "denied" ? "available" : "denied");
  }

  async changePolicy(policy: NotificationPolicy): Promise<void> {
    if (!this.#token) return;
    this.#publish(await recoverTransientRuntime(() => setNotificationPolicy(this.#token!, policy)));
  }

  async test(): Promise<void> {
    if (!this.#token) return;
    this.#publish(await recoverTransientRuntime(() => sendTestNotification(this.#token!, presenceDeviceId())));
  }

  async #save(subscription: PushSubscription) {
    if (!this.#token) return;
    const json = subscription.toJSON();
    if (!json.endpoint || !json.keys?.p256dh || !json.keys.auth) throw new Error("Browser push keys are incomplete");
    this.#publish(await recoverTransientRuntime(() => saveNotificationSubscription(this.#token!, presenceDeviceId(), {
      device_class: deviceClass(),
      endpoint: json.endpoint!,
      keys: { p256dh: json.keys!.p256dh!, auth: json.keys!.auth! },
    })));
  }

  async #reconcileExistingSubscription(): Promise<boolean> {
    try {
      const registration = await navigator.serviceWorker.getRegistration("/");
      const subscription = await registration?.pushManager.getSubscription();
      if (!subscription) return false;
      await this.#save(subscription);
      rememberEnabledIntent(true);
      this.#onState("enabled");
      return true;
    } catch {
      return false;
    }
  }

  #publish(settings: NotificationSettings) {
    this.#settings = settings;
    this.#onSettings(settings);
  }
}

function enabledIntentRemembered(): boolean {
  try {
    return window.localStorage.getItem(ENABLED_INTENT_KEY) === "true";
  } catch {
    return false;
  }
}

function rememberEnabledIntent(enabled: boolean) {
  try {
    if (enabled) window.localStorage.setItem(ENABLED_INTENT_KEY, "true");
    else window.localStorage.removeItem(ENABLED_INTENT_KEY);
  } catch {
    // Private browsing can deny storage while push remains usable for this session.
  }
}

function supportsPush(): boolean {
  return "serviceWorker" in navigator && "PushManager" in window && "Notification" in window;
}

function decodeUrlSafeBase64(value: string): Uint8Array<ArrayBuffer> {
  const padded = value.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(value.length / 4) * 4, "=");
  const binary = atob(padded);
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}
