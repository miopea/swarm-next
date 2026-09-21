import type { FleetHiveVersion, FleetVersions as Fleet, FleetVersionStanding } from "../api";

/**
 * ⚠️ THE WORDS CARRY THE MEANING, NOT A COLOUR. "Behind" and "Schema behind"
 * are different problems rather than two shades of one, and a reader using a
 * screen reader or a monochrome display has to be able to tell them apart.
 */
const standingLabel: Record<FleetVersionStanding, string> = {
  current: "Up to date",
  development: "Development build",
  behind_within_grace: "Behind, within grace",
  behind: "Behind",
  schema_behind: "Schema behind",
  unreadable: "Version unreadable",
  unknown: "Not compared",
};

/** Shows what every Hive runs, and says plainly which ones are a problem. */
export default function FleetVersions({ fleet, nameFor }: { fleet?: Fleet; nameFor?: (hiveId: string) => string | undefined }) {
  // ⚠️ A PANEL MUST NOT TAKE THE ROOM DOWN WITH IT. Reading straight off the
  // body threw on anything unshaped and unmounted the whole Keeper control
  // room — every other panel lost, to report a version. A surface whose job is
  // to raise a problem is the last thing that should be able to cause a bigger
  // one.
  const hives: FleetHiveVersion[] = Array.isArray(fleet?.hives) ? fleet.hives : [];
  const expected = fleet?.expected_release ?? null;
  const raised = hives.filter((hive) => hive.raises);
  const row = (hive: FleetHiveVersion) => <li key={hive.node_id}>
    <span><strong>{nameFor?.(hive.hive_id) ?? hive.hive_id}</strong><small>{hive.swarm_version} · schema {hive.database_schema_version}</small></span>
    <span className={`keeper-role-badge ${hive.raises ? "keeper" : "member"}`}>{standingLabel[hive.standing] ?? "Not compared"}</span>
  </li>;

  return <article className="keeper-panel">
    <header><div><p className="eyebrow">Fleet</p><h4>Swarm versions</h4></div><small>{expected ? `Current release ${expected}` : "No release known"}</small></header>
    {/*
      ⚠️ SAID OUT LOUD RATHER THAN DRAWN AS A HEALTHY FLEET. With no release
      ever seen, every Hive reads "Not compared" and NOTHING has been checked —
      rendering that as a clean table is the check failing open.
    */}
    {fleet && expected === null
      ? <p className="keeper-empty" role="status">No release check has returned a version yet, so no Hive can be compared against one. These are reports, not verdicts.</p>
      : null}
    {raised.length
      ? <p className="member-blocker-list" role="alert">{raised.length} {raised.length === 1 ? "Hive is" : "Hives are"} behind what the Apiary expects.</p>
      : null}
    {hives.length
      ? <ul className="keeper-hive-list" aria-label="Swarm versions by Hive">{hives.map(row)}</ul>
      : <p className="keeper-empty">No Hive has reported its version yet.</p>}
  </article>;
}
