import { useState } from "react";
import type { HiveIdentity } from "../api";
import ApiarySettings from "../settings/ApiarySettings";
import KeeperControlRoom from "./KeeperControlRoom";
import MemberControlRoom from "./MemberControlRoom";

/** One home for daily coordination and Apiary-owned configuration. */
export default function ApiaryWorkspace({ identity, operatorToken, busy, onIdentityChange, onOpenTasks }: {
  identity: HiveIdentity; operatorToken: string; busy: boolean;
  onIdentityChange: (identity: HiveIdentity) => void; onOpenTasks: () => void;
}) {
  const [managing, setManaging] = useState<false | "settings" | "invitations" | "profile">(false);
  const context = identity.apiary_context;
  if (managing || context?.mode !== "federated") return <div className="apiary-management">
    {context?.mode === "federated" ? <button className="secondary-button" type="button" onClick={() => setManaging(false)}>Back to Apiary overview</button> : null}
    <ApiarySettings busy={busy} hiveIdentity={identity} operatorToken={operatorToken} initialFocus={managing === "invitations" || managing === "profile" ? managing : undefined}
      onHiveIdentityChange={(next) => { onIdentityChange(next); if (context?.mode !== "federated" && next.apiary_context?.mode === "federated") setManaging(false); }} />
  </div>;
  return context.local_role === "keeper"
    ? <KeeperControlRoom identity={identity} operatorToken={operatorToken} onManage={() => setManaging("settings")} onReviewProfile={() => setManaging("profile")} onInvite={() => setManaging("invitations")} onOpenTasks={onOpenTasks} />
    : <MemberControlRoom identity={identity} operatorToken={operatorToken} onManage={() => setManaging("settings")} onReviewProfile={() => setManaging("profile")} onOpenTasks={onOpenTasks} />;
}
