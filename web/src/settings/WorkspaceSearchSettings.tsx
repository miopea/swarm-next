import { useEffect, useState } from "react";
import { authenticatedFetch } from "../api/request";

type SearchSettings = { revision: number; folders: string[]; installation_folders: string[]; health: { path: string; available: boolean }[] };

export default function WorkspaceSearchSettings({ operatorToken, onSaved }: { operatorToken: string; onSaved: () => Promise<void> }) {
  const [open, setOpen] = useState(false);
  const [settings, setSettings] = useState<SearchSettings | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [saved, setSaved] = useState(false);
  const [refreshFailed, setRefreshFailed] = useState(false);
  const [reload, setReload] = useState(0);
  useEffect(() => {
    if (!open || settings) return;
    const controller = new AbortController();
    const deadline = window.setTimeout(() => {
      controller.abort();
      setBusy(false);
      setError("Search folders took too long to load. Retry when this Hive is reachable.");
    }, 8_000);
    setBusy(true); setError("");
    void authenticatedFetch(operatorToken, "/api/v1/workspace-search-settings", { signal: controller.signal })
      .then(response => response.json() as Promise<SearchSettings>)
      .then(value => { if (!controller.signal.aborted) { setBusy(false); setSettings(value); setDraft(value.folders.join("\n")); } })
      .catch(reason => { if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : "Search folders could not be loaded."); })
      .finally(() => { window.clearTimeout(deadline); if (!controller.signal.aborted) setBusy(false); });
    return () => { controller.abort(); window.clearTimeout(deadline); };
  }, [open, operatorToken, reload, settings]);

  async function refreshRepositories() {
    setBusy(true);
    try { await onSaved(); setRefreshFailed(false); }
    catch { setRefreshFailed(true); }
    finally { setBusy(false); }
  }

  async function save() {
    if (!settings || busy) return;
    setBusy(true); setError(""); setSaved(false);
    try {
      const response = await authenticatedFetch(operatorToken, "/api/v1/workspace-search-settings", {
        method: "PUT", headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ revision: settings.revision, folders: draft.split("\n").map(path => path.trim()).filter(Boolean) }),
      });
      const value = await response.json() as SearchSettings;
      setSettings(value); setDraft(value.folders.join("\n")); setSaved(true);
      await refreshRepositories();
    } catch (reason) { setError(reason instanceof Error ? reason.message : "Folders could not be saved. Your edit is preserved."); }
    finally { setBusy(false); }
  }

  return <details className="workspace-search-settings" open={open} onToggle={event => setOpen(event.currentTarget.open)}>
    <summary>Repository search folders</summary>
    {open && <div className="field-stack">
      <p>Where this Hive looks for repositories. These paths are on the machine running Swarm, not your browser’s computer.</p>
      {settings && <>
        <strong>Installation folders</strong>
        <ul>{settings.installation_folders.map(path => <li key={path}>{path}</li>)}</ul>
        <label htmlFor="workspace-search-folders">Additional trusted folders</label>
        <textarea id="workspace-search-folders" rows={4} value={draft} disabled={busy} onChange={event => { setDraft(event.target.value); setSaved(false); }} placeholder={"~/projects/personal\n~/projects/work"} />
        <small>One existing folder per line, up to 16. Saving trusts these folders for repository discovery. Removing one here does not remove files or change existing workers.</small>
        {settings.health.filter(folder => !folder.available).map(folder => <p className="field-error" key={folder.path}>Unavailable: {folder.path}. Check the folder or mount, or remove it from your additional folders.</p>)}
        <button type="button" disabled={busy} onClick={() => void save()}>Save search folders</button>
      </>}
      {busy && <p role="status">Checking search folders…</p>}
      {saved && <p role="status">Search folders saved. Existing workers are unchanged.</p>}
      {refreshFailed && <div role="alert"><p>Folders are saved, but the repository list could not refresh. You do not need to save again.</p><button type="button" disabled={busy} onClick={() => void refreshRepositories()}>Refresh repository list</button></div>}
      {error && <p role="alert">{error}</p>}
      {error && <button type="button" disabled={busy} onClick={() => { setSaved(false); setSettings(null); setReload(value => value + 1); }}>{settings ? "Reload saved folders (discard edit)" : "Retry loading folders"}</button>}
    </div>}
  </details>;
}
