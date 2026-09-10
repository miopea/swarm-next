import { useState } from "react";
import RuntimeUpdateConfirm from "../runtime/RuntimeUpdateConfirm";
import WhatsNewModal from "../runtime/WhatsNewModal";
import { ENGINE_RESTART_CONSEQUENCE } from "../runtime/workerEngine";
import BroadcastToWorkers from "../workers/BroadcastToWorkers";
import DevelopmentReloadAction from "../settings/DevelopmentReloadAction";

/** All actions below are local fiction, with no Hive request or worker input. */
export default function RuntimeDialogsFixture() {
  const [open, setOpen] = useState<"engine" | "broadcast" | "notes">();
  const [result, setResult] = useState("No simulated action requested");
  const [preparation, setPreparation] = useState<"failed" | "deferred">("failed");
  return <main style={{ padding: 24 }}>
    <h1>Runtime dialog interaction fixture</h1>
    <p>Fictional actions only. No updates, messages or Hive requests.</p>
    <button onClick={() => setOpen("engine")}>Open restart confirmation</button>
    <button onClick={() => setOpen("broadcast")}>Open broadcast draft</button>
    <button onClick={() => setOpen("notes")}>Open release notes</button>
    <p role="status">{result}</p>
    <DevelopmentReloadAction busy={false} runtime={{ enabled: true, version: "1.7.0",
      state: preparation, reload_available: true, source_revision: "abcdef012345",
      deployed_source_revision: "123456789abc", source_dirty: false,
      deployed_source_published: true, protocol_migration_required: true }}
      onReload={async () => { setResult("Unexpected ordinary reload callback"); }}
      onPrepare={async () => { setPreparation("deferred"); setResult("Fictional preparation recorded once; no build or worker change"); }} />
    {open === "engine" && <RuntimeUpdateConfirm busy={false}
      update={{ kind: "worker_engine", label: "Worker engine update", busy: false,
        detail: "A fictional engine update would restart loaded workers.",
        actionLabel: "Apply worker engine update", consequence: ENGINE_RESTART_CONSEQUENCE }}
      onCancel={() => setOpen(undefined)}
      onConfirm={() => { setResult("Simulated confirmation only; no worker was changed"); setOpen(undefined); }} />}
    <BroadcastToWorkers open={open === "broadcast"} onClose={() => setOpen(undefined)}
      onBroadcast={async () => ({ reached: 0, skipped: 0 })} />
    {open === "notes" && <WhatsNewModal onDismiss={() => setOpen(undefined)}
      releases={[{ version: "fixture", notes: [{ kind: "feature", summary: "A fictional improvement", needs_worker_engine_update: true }] }]} />}
  </main>;
}
