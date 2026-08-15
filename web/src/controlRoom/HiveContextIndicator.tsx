import type { HiveIdentity } from "../api";

type Props = {
  identity: HiveIdentity | undefined;
  compact?: boolean;
};

export default function HiveContextIndicator({ identity, compact = false }: Props) {
  if (!identity) return null;

  const context = hiveContextPresentation(identity);
  return (
    <span
      className={`hive-context-indicator${compact ? " compact" : ""}`}
      aria-label={context.accessibleLabel}
      title={context.accessibleLabel}
    >
      <span className="hive-context-name">{context.name}</span>
      <span className={`hive-context-role ${context.roleClass}`}>{context.role}</span>
    </span>
  );
}

export function hiveContextPresentation(identity: HiveIdentity) {
  const context = identity.apiary_context;
  if (context?.mode === "federated") {
    const role = context.local_role === "keeper" ? "Keeper" : "Hive member";
    return {
      name: context.apiary.name,
      role,
      roleClass: context.local_role,
      accessibleLabel: `${identity.hive.name} is ${role === "Keeper" ? "the Keeper" : "a member"} of ${context.apiary.name}`,
    };
  }

  return {
    name: identity.hive.name,
    role: "Personal Hive",
    roleClass: "personal",
    accessibleLabel: `${identity.hive.name} is a personal Hive`,
  };
}
