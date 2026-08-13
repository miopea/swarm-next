import { useEffect, useMemo, useState } from "react";

import {
  beginJiraAuthorization,
  createJiraBinding,
  disconnectJira,
  fetchJiraBindings,
  fetchJiraProjects,
  fetchJiraProjectStatuses,
  replaceJiraMappings,
  syncJiraBinding,
  type JiraProject,
  type JiraProjectBinding,
  type JiraProjectStatus,
  type JiraReadiness,
  type JiraStatusMapping,
  type TaskState,
  type Worker,
} from "../api";

type Props = {
  operatorToken: string;
  readiness: JiraReadiness | undefined;
  unavailable: boolean;
  workers: Worker[];
  onNavigate?: (url: string) => void;
};

const taskStates: { value: TaskState; label: string }[] = [
  { value: "draft", label: "Inbox" },
  { value: "ready", label: "Ready" },
  { value: "active", label: "Active" },
  { value: "blocked", label: "Blocked" },
  { value: "review", label: "Review" },
  { value: "completed", label: "Done" },
];

export default function JiraSettings({ operatorToken, readiness, unavailable, workers, onNavigate = (url) => window.location.assign(url) }: Props) {
  const repositoryWorkers = useMemo(() => workers.filter((worker) => worker.role === "worker"), [workers]);
  const [bindings, setBindings] = useState<JiraProjectBinding[]>([]);
  const [query, setQuery] = useState("");
  const [projects, setProjects] = useState<JiraProject[]>([]);
  const [selectedProject, setSelectedProject] = useState<JiraProject>();
  const [statuses, setStatuses] = useState<JiraProjectStatus[]>([]);
  const [mapping, setMapping] = useState<Record<string, TaskState>>({});
  const [workerId, setWorkerId] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");

  useEffect(() => {
    let cancelled = false;
    void fetchJiraBindings(operatorToken)
      .then((next) => { if (!cancelled) setBindings(next); })
      .catch(() => { if (!cancelled) setBindings([]); });
    return () => { cancelled = true; };
  }, [operatorToken]);

  useEffect(() => {
    if (readiness?.connection !== "ready" || selectedProject) return;
    let cancelled = false;
    const timer = window.setTimeout(() => {
      void fetchJiraProjects(operatorToken, query)
        .then((next) => { if (!cancelled) setProjects(next); })
        .catch((error: unknown) => { if (!cancelled) setMessage(error instanceof Error ? error.message : "Jira projects could not be loaded."); });
    }, 250);
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [operatorToken, query, readiness?.connection, selectedProject]);

  async function chooseProject(project: JiraProject) {
    setSelectedProject(project);
    setProjects([]);
    setQuery(`${project.key} · ${project.name}`);
    setBusy(true);
    setMessage("");
    try {
      const nextStatuses = await fetchJiraProjectStatuses(operatorToken, project.id);
      setStatuses(nextStatuses);
      setMapping(Object.fromEntries(nextStatuses.map((status) => [status.id, status.recommended_task_state])));
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "This Jira workflow could not be read.");
      setSelectedProject(undefined);
    } finally {
      setBusy(false);
    }
  }

  async function saveProject() {
    if (!selectedProject || !workerId || statuses.length === 0) return;
    setBusy(true);
    setMessage("");
    try {
      const binding = await createJiraBinding(operatorToken, selectedProject, workerId);
      const mappings: JiraStatusMapping[] = statuses.map((status) => ({
        jira_status_id: status.id,
        jira_status_name: status.name,
        task_state: mapping[status.id] ?? status.recommended_task_state,
      }));
      await replaceJiraMappings(operatorToken, binding.id, mappings);
      setBindings(await fetchJiraBindings(operatorToken));
      setMessage(`${selectedProject.name} is ready for this Hive.`);
      setSelectedProject(undefined);
      setStatuses([]);
      setMapping({});
      setWorkerId("");
      setQuery("");
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "The Jira project could not be connected.");
    } finally {
      setBusy(false);
    }
  }

  async function syncProject(binding: JiraProjectBinding) {
    setBusy(true);
    setMessage("");
    try {
      const tasks = await syncJiraBinding(operatorToken, binding.id);
      setMessage(`${binding.project_name} refreshed ${tasks.length} Jira issue${tasks.length === 1 ? "" : "s"}.`);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Jira issues could not be refreshed.");
    } finally {
      setBusy(false);
    }
  }

  async function connectJira() {
    setBusy(true);
    setMessage("");
    try {
      const authorizationUrl = await beginJiraAuthorization(operatorToken);
      onNavigate(authorizationUrl);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Atlassian authorization could not start.");
      setBusy(false);
    }
  }

  async function disconnect() {
    setBusy(true);
    setMessage("");
    try {
      await disconnectJira(operatorToken);
      window.location.reload();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "Jira could not be disconnected.");
      setBusy(false);
    }
  }

  return (
    <section id="settings-integrations" className="settings-card integration-settings" aria-labelledby="integration-heading">
      <div><p className="eyebrow">Integrations</p><h3 id="integration-heading">Bring Jira into your Hive</h3></div>
      <p>Connect Jira projects to the worker who owns the repository. Queen can then reason about work and assignment without handling credentials or workspace paths.</p>
      <div className="integration-status" role="status">
        <span className={`presence ${readiness?.connection === "ready" ? "online" : unavailable || readiness?.connection === "credentials_invalid" || readiness?.connection === "permission_denied" ? "offline" : "waiting"}`} />
        <span><strong>{jiraReadinessLabel(readiness, unavailable)}</strong><small>{jiraReadinessDetail(readiness, unavailable)}</small></span>
      </div>

      {readiness?.connection === "ready" ? (
        <button className="secondary-button jira-auth-action" type="button" disabled={busy} onClick={() => void disconnect()}>
          Disconnect Jira
        </button>
      ) : (
        <div className="jira-connect-panel">
          <button className="primary-action jira-auth-action" type="button" disabled={busy || unavailable || readiness?.configured === false} onClick={() => void connectJira()}>
            {busy ? "Opening Atlassian…" : readiness?.configured === false ? "Atlassian app setup required" : readiness?.connection === "credentials_invalid" ? "Reconnect with Atlassian" : "Connect with Atlassian"}
          </button>
          <small className="privacy-note">A browser consent page opens, then returns you here. Swarm stores the rotating token privately on this host.</small>
        </div>
      )}

      {bindings.length > 0 ? (
        <div className="jira-binding-list" aria-label="Connected Jira projects">
          {bindings.map((binding) => {
            const worker = workers.find((candidate) => candidate.id === binding.default_worker_id);
            return (
              <div key={binding.id} className="jira-binding-card">
                <span><strong>{binding.project_key}</strong><small>{binding.project_name}</small></span>
                <span><strong>{worker?.name ?? "Choose a worker"}</strong><small>{binding.workflow_mapped ? "Workflow mapped" : "Mapping needed"}</small></span>
                <button className="secondary-button" type="button" disabled={busy || !binding.workflow_mapped || !binding.default_worker_id} onClick={() => void syncProject(binding)}>Sync now</button>
              </div>
            );
          })}
        </div>
      ) : <small className="privacy-note">No Jira projects are connected to this Hive yet.</small>}

      {readiness?.connection === "ready" ? (
        <div className="jira-project-setup">
          <label>
            <span>Find a Jira project</span>
            <input
              value={query}
              placeholder="Type a project name or key"
              autoComplete="off"
              onChange={(event) => { setQuery(event.target.value); setSelectedProject(undefined); setStatuses([]); }}
            />
          </label>
          {!selectedProject && projects.length > 0 ? (
            <div className="jira-project-results" role="listbox" aria-label="Visible Jira projects">
              {projects.slice(0, 12).map((project) => (
                <button key={project.id} type="button" role="option" onClick={() => void chooseProject(project)}>
                  <strong>{project.key}</strong><span>{project.name}</span>
                </button>
              ))}
            </div>
          ) : null}
          {selectedProject ? (
            <div className="jira-workflow-setup">
              <label>
                <span>Repository worker</span>
                <select value={workerId} onChange={(event) => setWorkerId(event.target.value)}>
                  <option value="">Choose the worker who owns this code</option>
                  {repositoryWorkers.map((worker) => <option key={worker.id} value={worker.id}>{worker.name}</option>)}
                </select>
              </label>
              <div className="jira-status-map" aria-label="Jira workflow mapping">
                {statuses.map((status) => (
                  <label key={status.id}>
                    <span>{status.name}</span>
                    <select value={mapping[status.id] ?? status.recommended_task_state} onChange={(event) => setMapping((current) => ({ ...current, [status.id]: event.target.value as TaskState }))}>
                      {taskStates.map((state) => <option key={state.value} value={state.value}>{state.label}</option>)}
                    </select>
                  </label>
                ))}
              </div>
              <button className="primary-action" type="button" disabled={busy || !workerId || statuses.length === 0} onClick={() => void saveProject()}>
                {busy ? "Connecting…" : "Connect project"}
              </button>
            </div>
          ) : null}
        </div>
      ) : null}
      {message ? <p className="settings-message" role="status">{message}</p> : null}
      <small className="privacy-note">Owned tasks continue; new shared claims wait.</small>
      <small className="privacy-note">Jira remains the authority for issue identity, workflow status, and human assignee. Swarm keeps worker assignment, execution evidence, and terminal history local.</small>
    </section>
  );
}

function jiraReadinessLabel(readiness: JiraReadiness | undefined, unavailable: boolean) {
  if (unavailable) return "Jira status unavailable";
  switch (readiness?.connection) {
    case "ready": return readiness.account_name ? `Connected as ${readiness.account_name}` : "Jira connected";
    case "credentials_invalid": return "Jira credentials need attention";
    case "permission_denied": return "Jira access was denied";
    case "network_unavailable": return "Jira is temporarily unavailable";
    case "not_connected": return "Jira not connected";
    default: return "Checking Jira";
  }
}

function jiraReadinessDetail(readiness: JiraReadiness | undefined, unavailable: boolean) {
  if (unavailable) return "Local workers and tasks remain available.";
  switch (readiness?.connection) {
    case "ready": return "Project discovery uses your Jira identity.";
    case "credentials_invalid": return "Update the local Jira credential adapter.";
    case "permission_denied": return "This identity cannot access the requested Jira resource.";
    case "network_unavailable": return "Owned work stays available; new shared claims wait.";
    case "not_connected": return readiness?.configured ? "Connect your Atlassian account to choose projects." : "This host needs its one-time Atlassian app configuration.";
    default: return "Credentials never enter Queen or the browser.";
  }
}
