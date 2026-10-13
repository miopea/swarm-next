import { authenticatedFetch } from "./request";

// Reachable retains the installed API/storage token "away" (ADR 0018).
export type PresenceMode = "at_hive" | "away" | "night_watch";
export type PresenceSource = "manual" | "scheduled" | "active_device" | "screen_locked" | "inactive_device" | "timed_out";
export type PresenceDeviceClass = "desktop" | "mobile";
export type PresenceObservationState = "active" | "idle" | "locked" | "hidden";
export type OperatorPresence = {
  mode: PresenceMode;
  manual_mode: PresenceMode | null;
  source: PresenceSource;
};

export type NightWatchConfiguration = {
  enabled: boolean;
  timezone: string;
  start_minute: number;
  end_minute: number;
};

/** Daily hours, in minutes after local midnight. */
export type NightWatchHours = { start_minute: number; end_minute: number };

/**
 * The schedule this Hive runs, and the hours its Apiary sets, if any (ADR 0113).
 * Required hours are locked here; the time zone stays the Hive's own, and until
 * it has one there is no schedule to put them into.
 */
export type NightWatchState = { configuration: NightWatchConfiguration | null; setByApiary: NightWatchHours | null };

type NightWatchBody = (Partial<NightWatchConfiguration> & { set_by_apiary?: NightWatchHours | null }) | null;

function nightWatchState(body: NightWatchBody): NightWatchState {
  const configuration = body && typeof body.timezone === "string"
    ? { enabled: Boolean(body.enabled), timezone: body.timezone, start_minute: Number(body.start_minute), end_minute: Number(body.end_minute) }
    : null;
  return { configuration, setByApiary: body?.set_by_apiary ?? null };
}

export async function fetchNightWatchConfiguration(operatorToken: string, signal?: AbortSignal): Promise<NightWatchState> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/presence/night-watch", { signal });
  return nightWatchState(await response.json() as NightWatchBody);
}

export async function saveNightWatchConfiguration(operatorToken: string, configuration: NightWatchConfiguration, signal?: AbortSignal): Promise<NightWatchState> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/presence/night-watch", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(configuration),
    signal,
  });
  return nightWatchState(await response.json() as NightWatchBody);
}

export async function fetchPresence(operatorToken: string, signal?: AbortSignal): Promise<OperatorPresence> {
  const response = await authenticatedFetch(operatorToken, "/api/v1/presence", { signal });
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
