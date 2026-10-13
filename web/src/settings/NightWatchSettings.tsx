import { useEffect, useId, useRef, useState } from "react";
import { fetchNightWatchConfiguration, saveNightWatchConfiguration, type NightWatchConfiguration, type NightWatchHours, type NightWatchState } from "../api/presence";
import { RuntimeRequestError } from "../api/request";

function time(minutes: number) { return `${Math.floor(minutes / 60).toString().padStart(2, "0")}:${(minutes % 60).toString().padStart(2, "0")}`; }
function minutes(value: string) {
  if (!/^\d{2}:\d{2}$/.test(value)) return undefined;
  const [hour, minute] = value.split(":").map(Number);
  return hour! < 24 && minute! < 60 ? hour! * 60 + minute! : undefined;
}

export default function NightWatchSettings({ operatorToken }: { operatorToken: string }) {
  const [enabled, setEnabled] = useState(false);
  const [zone, setZone] = useState(() => Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC");
  const defaultZone = useRef(zone);
  const [start, setStart] = useState("22:00");
  const [end, setEnd] = useState("07:00");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const [reload, setReload] = useState(0);
  const [persisted, setPersisted] = useState<NightWatchConfiguration | null>(null);
  // Hours this Hive's Apiary sets (ADR 0113): locked here, in this Hive's own time zone.
  const [required, setRequired] = useState<NightWatchHours | null>(null);
  const zoneHintId = useId();
  const requiredNoteId = useId();

  function show(state: NightWatchState) {
    const config = state.configuration;
    setPersisted(config); setRequired(state.setByApiary);
    if (config) {
      setEnabled(config.enabled); setZone(config.timezone);
      setStart(time(config.start_minute)); setEnd(time(config.end_minute));
    } else if (state.setByApiary) {
      setEnabled(true); setZone(defaultZone.current);
      setStart(time(state.setByApiary.start_minute)); setEnd(time(state.setByApiary.end_minute));
    } else { setEnabled(false); setZone(defaultZone.current); setStart("22:00"); setEnd("07:00"); }
  }
  const saveRequest = useRef<AbortController | undefined>(undefined);

  useEffect(() => {
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), 8_000);
    let disposed = false;
    setLoading(true); setSaving(false); setError(undefined); setSaved(false);
    void fetchNightWatchConfiguration(operatorToken, controller.signal).then((state) => {
      if (disposed) return;
      show(state);
      setLoading(false);
    }).catch(() => {
      if (!disposed) setError("Could not load the Night Watch schedule. Retry before changing it.");
    }).finally(() => window.clearTimeout(deadline));
    return () => {
      disposed = true; controller.abort(); window.clearTimeout(deadline);
      const pendingSave = saveRequest.current;
      saveRequest.current = undefined;
      pendingSave?.abort();
    };
  }, [operatorToken, reload]);

  async function save() {
    const startMinute = minutes(start), endMinute = minutes(end);
    if (startMinute === undefined || endMinute === undefined || startMinute === endMinute || !zone.trim()) {
      setError("Choose a time zone and different start and end times."); return;
    }
    if (saveRequest.current) return;
    const controller = new AbortController();
    saveRequest.current = controller;
    const deadline = window.setTimeout(() => controller.abort(), 8_000);
    setSaving(true); setSaved(false); setError(undefined);
    try {
      const confirmed = await saveNightWatchConfiguration(operatorToken, { enabled, timezone: zone.trim(), start_minute: startMinute, end_minute: endMinute }, controller.signal);
      if (!controller.signal.aborted) { show(confirmed); setSaved(true); }
    } catch (failure) {
      if (saveRequest.current === controller) setError(failure instanceof RuntimeRequestError && failure.status === 400
        ? "Schedule not saved. Use a region/city time zone such as America/New_York or Europe/London, and different start and end times. Your edits are still here."
        : failure instanceof RuntimeRequestError && failure.status === 409
          ? "Schedule not saved. Your Apiary sets these hours, and only its Keeper can change them. The time zone is still yours."
          : "The schedule save was not confirmed. Your edits are still here; retry to confirm them.");
    } finally {
      window.clearTimeout(deadline);
      if (saveRequest.current === controller) { saveRequest.current = undefined; setSaving(false); }
    }
  }

  const edited = persisted
    ? enabled !== persisted.enabled || zone.trim() !== persisted.timezone || minutes(start) !== persisted.start_minute || minutes(end) !== persisted.end_minute
    : enabled || zone.trim() !== defaultZone.current || start !== "22:00" || end !== "07:00";

  return <div className="night-watch-settings">
    <h4>Night Watch schedule</h4>
    <p>Use local times in the zone below. Returning to Swarm on desktop ends this watch; phone use does not. The next scheduled night can begin normally.</p>
    {!loading && <p aria-label="Saved Night Watch schedule">{persisted?.enabled
      ? `Saved schedule: daily ${time(persisted.start_minute)}–${time(persisted.end_minute)}${persisted.end_minute < persisted.start_minute ? " the next day" : ""} · ${persisted.timezone}.`
      : "No automatic Night Watch is scheduled. You can still choose Night Watch manually."}</p>}
    {loading && !error && <p role="status">Loading schedule…</p>}
    {error && <p role="alert">{error}</p>}
    {loading && error && <button type="button" onClick={() => setReload((value) => value + 1)}>Retry schedule load</button>}
    <fieldset disabled={loading || saving} onChange={() => setSaved(false)}>
      <legend>Daily schedule</legend>
      {required && <p id={requiredNoteId} className="night-watch-set-by-apiary">{persisted
        ? "Set by your Apiary. Only its Keeper can change these hours. The time zone is this Hive's own."
        : "Your Apiary sets these hours, in this Hive's own time. Choose this Hive's time zone and save to put them into effect."}</p>}
      <label><input type="checkbox" checked={enabled} disabled={Boolean(required)} aria-describedby={required ? requiredNoteId : undefined} onChange={(event) => setEnabled(event.target.checked)} />Enable scheduled Night Watch</label>
      <label>Time zone<input value={zone} maxLength={128} onChange={(event) => setZone(event.target.value)} placeholder="America/New_York" aria-describedby={zoneHintId} /></label>
      <small id={zoneHintId}>Use region/city, such as America/New_York or Europe/London. Times follow that zone, including daylight-saving changes.</small>
      <label>Starts<input type="time" value={start} disabled={Boolean(required)} aria-describedby={required ? requiredNoteId : undefined} onChange={(event) => setStart(event.target.value)} /></label>
      <label>Ends<input type="time" value={end} disabled={Boolean(required)} aria-describedby={required ? requiredNoteId : undefined} onChange={(event) => setEnd(event.target.value)} /></label>
      <button type="button" onClick={() => void save()}>{saving ? "Saving schedule…" : "Save schedule"}</button>
    </fieldset>
    {!loading && edited && !saving && <p role="status">Unsaved schedule changes.</p>}
    {saved && <p role="status">Schedule saved.</p>}
  </div>;
}
