import { useState } from "react";

export function useReorderDrag(ids: string[], onReorder: (ids: string[]) => void) {
  const [draggedId, setDraggedId] = useState<string>();
  const [dropTargetId, setDropTargetId] = useState<string>();

  function end() {
    setDraggedId(undefined);
    setDropTargetId(undefined);
  }

  function leave(targetId: string) {
    setDropTargetId((current) => current === targetId ? undefined : current);
  }

  function dropBefore(targetId: string) {
    if (!draggedId || draggedId === targetId) return;
    const reordered = ids.filter((id) => id !== draggedId);
    const targetIndex = reordered.indexOf(targetId);
    if (targetIndex < 0) return;
    reordered.splice(targetIndex, 0, draggedId);
    end();
    onReorder(reordered);
  }

  return {
    draggedId,
    dropTargetId,
    start: setDraggedId,
    target: setDropTargetId,
    leave,
    end,
    dropBefore,
  };
}
