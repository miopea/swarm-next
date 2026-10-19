import { cloneElement, useEffect, useState, type ReactElement } from "react";
import type { HiveIdentity } from "../api";

const identity = { operator: { id: "fictional", display_name: "Bea" }, hive: { id: "meadow", name: "Meadow Hive", operator_id: "fictional", apiary_id: "garden" }, apiary_context: { mode: "federated", local_role: "member", apiary: { id: "garden", name: "Meadow Test Garden", keeper_operator_id: "keeper", shared_work_backend: "jira" } } } satisfies HiveIdentity;

/**
 * No-proxy fixture only: the Tasks board as a member of an Apiary, with shared
 * work waiting on an outside party, on this Hive's operator and on another
 * Hive's (ADR 0119). Every read is fictional and nothing reaches a Hive.
 */
export default function SharedWaitsFixture({ board }: { board: ReactElement<{ hiveIdentity?: HiveIdentity }> }) {
  const [mounted, setMounted] = useState(false);
  useEffect(() => {
    const previous = window.fetch;
    window.fetch = async (input) => {
      const url = String(input);
      const now = Math.floor(Date.now() / 1000);
      const shared = (id: string, title: string, home: string, state: string, wait?: object) => ({ id, apiary_id: "garden", source: "swarm", title, description: "", priority: "normal", state, home_node_id: `node-${home}`, home_hive_id: home, revision: 4, created_at: now - 86_400, updated_at: now - 600, ...(wait ? { wait } : {}) });
      const payload = url.endsWith("/apiary/tasks") ? [
          shared("shared-outside", "Move the office onto the backup circuit", "clover", "blocked", { kind: "outside", needs: "The carrier restores the backup circuit to Available", party: "the carrier", resume_at: now + 2 * 86_400 }),
          shared("shared-mine", "Turn on second-step sign-in for the shared mailbox", "meadow", "blocked", { kind: "operator_action", needs: "Sign in to the identity portal and approve the second step", hive_id: "meadow" }),
          shared("shared-theirs", "Renew the shared certificate", "clover", "blocked", { kind: "operator_action", needs: "Upload the renewed certificate to the gateway", hive_id: "clover" }),
          shared("shared-active", "Rotate the shared signing key", "clover", "active"),
        ]
        : url.endsWith("/apiary/members") ? [
          { hive_id: "meadow", hive_name: "Meadow Hive", operator_id: "fictional", operator_display_name: "Bea", role: "member", is_local: true },
          { hive_id: "clover", hive_name: "Clover Hive", operator_id: "operator-2", operator_display_name: "Cora", role: "member", is_local: false },
        ]
        : url.includes("stewardship") ? { stewardship: null }
        : [];
      return new Response(JSON.stringify(payload), { headers: { "Content-Type": "application/json" } });
    };
    setMounted(true);
    return () => { window.fetch = previous; };
  }, []);
  return mounted ? cloneElement(board, { hiveIdentity: identity }) : null;
}
