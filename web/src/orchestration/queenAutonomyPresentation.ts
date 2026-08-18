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
    detail: "Most useful during Night Watch. Queen may keep approved work moving and use durable rules you already granted, including scoped deployment authority. She never creates a new approval or replaces Scout.",
  },
};

export function queenAutonomyLabel(level: QueenAutonomyLevel) {
  return PRESENTATION[level].label;
}

export function queenAutonomyDetail(level: QueenAutonomyLevel) {
  return PRESENTATION[level].detail;
}
