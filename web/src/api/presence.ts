import { authenticatedFetch } from "./request";

export type PresenceMode = "at_hive" | "away" | "night_watch";
export type PresenceSource = "manual" | "scheduled" | "active_device" | "screen_locked" | "inactive_device" | "timed_out";
export type PresenceDeviceClass = "desktop" | "mobile";
export type PresenceObservationState = "active" | "idle" | "locked" | "hidden";
export type OperatorPresence = {
  mode: PresenceMode;
  manual_mode: PresenceMode | null;
  source: PresenceSource;
};

export async function fetchPresence(operatorToken: string): Promise<OperatorPresence> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/presence");
  return response.json() as Promise<OperatorPresence>;
}

export async function setManualPresence(
  operatorToken: string,
  manualMode: PresenceMode | null,
): Promise<OperatorPresence> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/presence", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ manual_mode: manualMode }),
  });
  return response.json() as Promise<OperatorPresence>;
}

export async function observePresence(
  operatorToken: string,
  deviceId: string,
  deviceClass: PresenceDeviceClass,
  state: PresenceObservationState,
  desktopReturn = false,
): Promise<OperatorPresence> {
  const response = await authenticatedFetch(
    operatorToken,
    `/api/v1/presence/devices/${encodeURIComponent(deviceId)}`,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ device_class: deviceClass, state, ...(desktopReturn ? { desktop_return: true } : {}) }),
    },
  );
  return response.json() as Promise<OperatorPresence>;
}
