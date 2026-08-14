import type { Worker } from "../api";

export type TaskBoardFilter = "all" | "unassigned" | "assigned" | "active" | "jira" | "local" | "attention";
export type TaskBoardSort = "queue" | "priority" | "status" | "updated" | "worker" | "project";
export type TaskProjectChoice = { key: string; name: string; url?: string };

type Props = {
  query: string;
  filter: TaskBoardFilter;
  sort: TaskBoardSort;
  project: string;
  worker: string;
  workers: Worker[];
  projects: TaskProjectChoice[];
  openCount: number;
  busy?: boolean;
  onQueryChange: (value: string) => void;
  onFilterChange: (value: TaskBoardFilter) => void;
  onSortChange: (value: TaskBoardSort) => void;
  onProjectChange: (value: string) => void;
  onWorkerChange: (value: string) => void;
  onSync?: () => void;
};

export default function TaskBoardControls({ query, filter, sort, project, worker, workers, projects, openCount, busy = false, onQueryChange, onFilterChange, onSortChange, onProjectChange, onWorkerChange, onSync }: Props) {
  const assignableWorkers = workers.filter((candidate) => candidate.role !== "queen");

  return (
    <div className="task-board-controls">
      <label><span>Find work</span><input value={query} type="search" placeholder="Title or Jira key" onChange={(event) => onQueryChange(event.target.value)} /></label>
      <label><span>Show</span><select value={filter} onChange={(event) => onFilterChange(event.target.value as TaskBoardFilter)}><option value="all">All open tasks</option><option value="unassigned">Unassigned</option><option value="assigned">Assigned</option><option value="active">In progress</option><option value="attention">Blocked or review</option><option value="jira">Jira work</option><option value="local">Local work</option></select></label>
      <label><span>Worker</span><select value={worker} onChange={(event) => onWorkerChange(event.target.value)}><option value="all">All workers</option><option value="unassigned">Unassigned</option>{assignableWorkers.map((candidate) => <option key={candidate.id} value={candidate.id}>{candidate.name}</option>)}</select></label>
      <label><span>Sort by</span><select value={sort} onChange={(event) => onSortChange(event.target.value as TaskBoardSort)}><option value="queue">Queue order</option><option value="priority">Priority</option><option value="status">Needs attention</option><option value="updated">Recently updated</option><option value="worker">Worker</option><option value="project">Jira project</option></select></label>
      {projects.length > 0 && <div className="task-project-controls">
        <div className="task-control-heading"><span>Jira projects</span>{onSync && <button type="button" disabled={busy} onClick={onSync}>Sync now</button>}</div>
        <button type="button" className={project === "all" ? "selected" : ""} onClick={() => onProjectChange("all")}>All projects</button>
        {projects.map((choice) => <div className="task-project-row" key={choice.key}>
          <button type="button" className={project === choice.key ? "selected" : ""} onClick={() => onProjectChange(choice.key)}><strong>{choice.key}</strong><span>{choice.name}</span></button>
          {choice.url && <a href={choice.url} target="_blank" rel="noreferrer" aria-label={`Open ${choice.name} in Jira`}>↗</a>}
        </div>)}
      </div>}
      <small>{openCount} open · drag ordering is available in Queue order</small>
    </div>
  );
}
