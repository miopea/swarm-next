import {
  fetchNotificationSettings,
  removeNotificationSubscription,
  saveNotificationSubscription,
  sendTestNotification,
  setNotificationPolicy,
  type NotificationPolicy,
  type NotificationSettings,
} from "../api";
import { deviceClass, presenceDeviceId } from "../presence/PresenceController";

export type NotificationCapabilityState = "unsupported" | "available" | "enabling" | "enabled" | "denied" | "error";

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
      const settings = await fetchNotificationSettings(operatorToken);
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
      const subscription = await registration?.pushManager.getSubscription();
      if (!subscription || generation !== this.#generation) return;
      await this.#save(subscription);
      if (generation === this.#generation) this.#onState("enabled");
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
    const registration = supportsPush() ? await navigator.serviceWorker.getRegistration("/") : undefined;
    const subscription = await registration?.pushManager.getSubscription();
    if (subscription) await subscription.unsubscribe();
    const settings = await removeNotificationSubscription(this.#token, presenceDeviceId());
    this.#publish(settings);
    if (registration) await registration.unregister();
    this.#onState(supportsPush() && Notification.permission !== "denied" ? "available" : "denied");
  }

  async changePolicy(policy: NotificationPolicy): Promise<void> {
    if (!this.#token) return;
    this.#publish(await setNotificationPolicy(this.#token, policy));
  }

  async test(): Promise<void> {
    if (!this.#token) return;
    this.#publish(await sendTestNotification(this.#token, presenceDeviceId()));
  }

  async #save(subscription: PushSubscription) {
    if (!this.#token) return;
    const json = subscription.toJSON();
    if (!json.endpoint || !json.keys?.p256dh || !json.keys.auth) throw new Error("Browser push keys are incomplete");
    this.#publish(await saveNotificationSubscription(this.#token, presenceDeviceId(), {
      device_class: deviceClass(),
      endpoint: json.endpoint,
      keys: { p256dh: json.keys.p256dh, auth: json.keys.auth },
    }));
  }

  async #reconcileExistingSubscription(): Promise<boolean> {
    try {
      const registration = await navigator.serviceWorker.getRegistration("/");
      const subscription = await registration?.pushManager.getSubscription();
      if (!subscription) return false;
      await this.#save(subscription);
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
