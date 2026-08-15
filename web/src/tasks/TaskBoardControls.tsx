import type { Worker } from "../api";
import type { TaskBoardFilter, TaskBoardSort, TaskBoardSource } from "./taskBoardModel";

export type { TaskBoardFilter, TaskBoardSort, TaskBoardSource } from "./taskBoardModel";
export type TaskProjectChoice = { key: string; name: string; url?: string };

type Props = {
  query: string;
  filter: TaskBoardFilter;
  source: TaskBoardSource;
  sort: TaskBoardSort;
  project: string;
  worker: string;
  workers: Worker[];
  projects: TaskProjectChoice[];
  openCount: number;
  busy?: boolean;
  onQueryChange: (value: string) => void;
  onFilterChange: (value: TaskBoardFilter) => void;
  onSourceChange: (value: TaskBoardSource) => void;
  onSortChange: (value: TaskBoardSort) => void;
  onProjectChange: (value: string) => void;
  onWorkerChange: (value: string) => void;
  onSync?: () => void;
};

export default function TaskBoardControls({ query, filter, source, sort, project, worker, workers, projects, openCount, busy = false, onQueryChange, onFilterChange, onSourceChange, onSortChange, onProjectChange, onWorkerChange, onSync }: Props) {
  const assignableWorkers = workers.filter((candidate) => candidate.role !== "queen");

  return (
    <div className="task-board-controls">
      <label><span>Find work</span><input value={query} type="search" placeholder="Title, Jira key, or email" onChange={(event) => onQueryChange(event.target.value)} /></label>
      <label><span>Show</span><select value={filter} onChange={(event) => onFilterChange(event.target.value as TaskBoardFilter)}><option value="all">All open tasks</option><option value="unassigned">Unassigned</option><option value="assigned">Assigned</option><option value="active">In progress</option><option value="attention">Blocked or review</option></select></label>
      <label><span>Source</span><select value={source} onChange={(event) => onSourceChange(event.target.value as TaskBoardSource)}><option value="all">All sources</option><option value="jira">Jira</option><option value="email">Email</option><option value="local">Created in Swarm</option></select></label>
      <label><span>Worker</span><select value={worker} onChange={(event) => onWorkerChange(event.target.value)}><option value="all">All workers</option><option value="unassigned">Unassigned</option>{assignableWorkers.map((candidate) => <option key={candidate.id} value={candidate.id}>{candidate.name}</option>)}</select></label>
      <label><span>Sort by</span><select value={sort} onChange={(event) => onSortChange(event.target.value as TaskBoardSort)}><option value="queue">Queue order</option><option value="priority">Priority</option><option value="status">Needs attention</option><option value="updated">Recently updated</option><option value="worker">Worker</option><option value="project">Jira project</option></select></label>
      {projects.length > 0 && (source === "all" || source === "jira") && <div className="task-project-controls">
        <div className="task-control-heading"><span>Jira projects</span>{onSync && <button type="button" disabled={busy} onClick={onSync}>Sync now</button>}</div>
        <button type="button" className={project === "all" ? "selected" : ""} onClick={() => onProjectChange("all")}>All projects</button>
        {projects.map((choice) => <div className="task-project-row" key={choice.key}>
          <button type="button" className={project === choice.key ? "selected" : ""} onClick={() => onProjectChange(choice.key)}><strong>{choice.key}</strong><span>{choice.name}</span></button>
          {choice.url && <a href={choice.url} target="_blank" rel="noreferrer" aria-label={`Open ${choice.name} in Jira`}>↗</a>}
        </div>)}
      </div>}
      <small>{openCount} open{projects.length ? " · Jira refreshes every minute" : ""} · drag ordering is available in Queue order</small>
    </div>
  );
}
