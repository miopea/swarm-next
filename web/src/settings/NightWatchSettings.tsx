import { useEffect, useRef, useState } from "react";
import { fetchNightWatchConfiguration, saveNightWatchConfiguration } from "../api/presence";

function time(minutes: number) { return `${Math.floor(minutes / 60).toString().padStart(2, "0")}:${(minutes % 60).toString().padStart(2, "0")}`; }
function minutes(value: string) {
  if (!/^\d{2}:\d{2}$/.test(value)) return undefined;
  const [hour, minute] = value.split(":").map(Number);
  return hour! < 24 && minute! < 60 ? hour! * 60 + minute! : undefined;
}

export default function NightWatchSettings({ operatorToken }: { operatorToken: string }) {
  const [enabled, setEnabled] = useState(false);
  const [zone, setZone] = useState(() => Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC");
  const [start, setStart] = useState("22:00");
  const [end, setEnd] = useState("07:00");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const [saved, setSaved] = useState(false);
  const [reload, setReload] = useState(0);
  const saveRequest = useRef<AbortController | undefined>(undefined);

  useEffect(() => {
    const controller = new AbortController();
    const deadline = window.setTimeout(() => controller.abort(), 8_000);
    let disposed = false;
    setLoading(true); setSaving(false); setError(undefined); setSaved(false);
    void fetchNightWatchConfiguration(operatorToken, controller.signal).then((config) => {
      if (disposed) return;
      if (config) {
        setEnabled(config.enabled); setZone(config.timezone);
        setStart(time(config.start_minute)); setEnd(time(config.end_minute));
      } else { setEnabled(false); }
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
      await saveNightWatchConfiguration(operatorToken, { enabled, timezone: zone.trim(), start_minute: startMinute, end_minute: endMinute }, controller.signal);
      if (!controller.signal.aborted) setSaved(true);
    } catch {
      if (saveRequest.current === controller) setError("The schedule save was not confirmed. Your edits are still here; retry to confirm them.");
    } finally {
      window.clearTimeout(deadline);
      if (saveRequest.current === controller) { saveRequest.current = undefined; setSaving(false); }
    }
  }

  return <div className="night-watch-settings">
    <h4>Night Watch schedule</h4>
    <p>Use local times in the zone below. Returning to Swarm on desktop ends this watch; phone use does not. The next scheduled night can begin normally.</p>
    {loading && !error && <p role="status">Loading schedule…</p>}
    {error && <p role="alert">{error}</p>}
    {loading && error && <button type="button" onClick={() => setReload((value) => value + 1)}>Retry schedule load</button>}
    <fieldset disabled={loading || saving} onChange={() => setSaved(false)}>
      <legend>Daily schedule</legend>
      <label><input type="checkbox" checked={enabled} onChange={(event) => setEnabled(event.target.checked)} />Enable scheduled Night Watch</label>
      <label>Time zone<input value={zone} maxLength={128} onChange={(event) => setZone(event.target.value)} placeholder="America/New_York" /></label>
      <label>Starts<input type="time" value={start} onChange={(event) => setStart(event.target.value)} /></label>
      <label>Ends<input type="time" value={end} onChange={(event) => setEnd(event.target.value)} /></label>
      <button type="button" onClick={() => void save()}>{saving ? "Saving schedule…" : "Save schedule"}</button>
    </fieldset>
    {saved && <p role="status">Schedule saved.</p>}
  </div>;
}
