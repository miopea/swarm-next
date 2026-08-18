import type { QueenAutonomyLevel } from "../api";

const PRESENTATION: Record<QueenAutonomyLevel, { label: string; detail: string }> = {
  advisory: {
    label: "Advise only",
    detail: "Queen reviews the Hive and asks you what to do. She does not assign, wake, or execute work.",
  },
  coordinate: {
    label: "Coordinate the Hive",
    detail: "Queen may assign tasks, wake workers, and keep queues moving. Workers and Scout perform implementation work.",
  },
  local_execution: {
    label: "Run approved work",
    detail: "Queen may coordinate unattended local work and use authority you already granted in the rules. She does not create new approvals or replace Scout.",
  },
};

export function queenAutonomyLabel(level: QueenAutonomyLevel) {
  return PRESENTATION[level].label;
}

export function queenAutonomyDetail(level: QueenAutonomyLevel) {
  return PRESENTATION[level].detail;
}
