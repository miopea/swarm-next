#!/usr/bin/env node
"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const CAPABILITIES = [
  ["terminal", /terminal|pty|tmux|xterm|ansi|scrollback|resize|render/i],
  ["tasks", /task|assignment|assign|backlog|queue|workflow|dispatch/i],
  ["queen", /queen|oversight|orchestrat|autonom/i],
  ["drones", /drone|poll(?:ing)?|scheduler|watcher|housekeep/i],
  ["worker_state", /buzzing|sleeping|resting|awaiting|worker.?state|classif|idle/i],
  ["workers", /worker|roster|revive|spawn|launch|kill|fleet/i],
  ["jira", /jira|atlassian|issue sync|project sync/i],
  ["email", /email|outlook|mail|inbox/i],
  ["mobile_pwa", /mobile|android|pwa|service worker|manifest|notification/i],
  ["security_auth", /security|auth|token|credential|xss|csp|csrf|permission/i],
  ["messaging", /message|broadcast|inbox|outbox|inject/i],
  ["recovery", /recover|resume|reconnect|restart|crash|stuck|retry|fallback/i],
  ["resources", /memory|resource|pressure|cpu|load|swap|oom|leak|budget/i],
  ["settings", /setting|config|preference|toggle|option/i],
  ["providers", /claude|codex|provider|model|mcp/i],
  ["deploy_release", /deploy|release|install|upgrade|update|systemd|version/i],
  ["dev_mode", /dev mode|development mode|hot reload|reload build/i],
  ["ui_ux", /\bui\b|\bux\b|layout|style|css|dashboard|sidebar|modal|menu|icon/i],
  ["testing_quality", /test|lint|ruff|mypy|coverage|review|refactor|cleanup/i],
];

function fail(message) {
  process.stderr.write(`${message}\n`);
  process.exit(1);
}

function classifyType(subject) {
  const conventional = subject.match(/^([a-z]+)(?:\([^)]*\))?[!:]/i)?.[1]?.toLowerCase();
  if (conventional) return conventional;
  if (/^release\b/i.test(subject)) return "release";
  if (/^revert\b/i.test(subject)) return "revert";
  if (/^(fix|correct|prevent|avoid|restore|bound|stop|repair)\b/i.test(subject)) return "fix";
  if (/^(add|implement|introduce|enable|support|create|replace|migrate)\b/i.test(subject)) return "feature";
  if (/^(refactor|extract|centralize|split|decompose|rename)\b/i.test(subject)) return "refactor";
  if (/^(test|verify|prove)\b/i.test(subject)) return "test";
  if (/^(docs?|document)\b/i.test(subject)) return "docs";
  return "other";
}

function capabilitiesForText(text) {
  return CAPABILITIES.filter(([, pattern]) => pattern.test(text)).map(([name]) => name);
}

function capabilitiesFor(commit) {
  return capabilitiesForText(`${commit.subject}\n${commit.body}\n${commit.files.join("\n")}`);
}

function parseLog(raw) {
  const commits = [];
  let current;
  for (const line of raw.split(/\r?\n/)) {
    if (line.startsWith("@@@")) {
      const [hash, date, subject, body = ""] = line.slice(3).split("\x1f");
      current = { hash, date, subject, body, files: [], insertions: 0, deletions: 0 };
      commits.push(current);
      continue;
    }
    if (!current || !line) continue;
    const match = line.match(/^(\d+|-)\t(\d+|-)\t(.+)$/);
    if (!match) continue;
    current.insertions += match[1] === "-" ? 0 : Number(match[1]);
    current.deletions += match[2] === "-" ? 0 : Number(match[2]);
    current.files.push(match[3]);
  }
  return commits.map((commit, index) => ({
    ...commit,
    sequence: index + 1,
    short_hash: commit.hash.slice(0, 8),
    type: classifyType(commit.subject),
    capabilities: capabilitiesFor(commit),
    subject_capabilities: capabilitiesForText(commit.subject),
    references: [...new Set(`${commit.subject} ${commit.body}`.match(/(?:#[0-9]+|\b[A-Z][A-Z0-9]{1,9}-[0-9]+\b)/g) || [])],
  }));
}

function csvCell(value) {
  const text = String(value ?? "").replace(/\r?\n/g, " ");
  return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
}

function buildLedger(commits) {
  const columns = ["sequence", "hash", "date", "type", "release", "capabilities", "subject_capabilities", "references", "files", "insertions", "deletions", "subject"];
  const rows = commits.map((commit) => [
    commit.sequence,
    commit.hash,
    commit.date,
    commit.type,
    commit.type === "release" ? "yes" : "no",
    commit.capabilities.join(";"),
    commit.subject_capabilities.join(";"),
    commit.references.join(";"),
    commit.files.length,
    commit.insertions,
    commit.deletions,
    commit.subject,
  ]);
  return `${columns.join(",")}\n${rows.map((row) => row.map(csvCell).join(",")).join("\n")}\n`;
}

function candidateRegressionChains(commits) {
  const featureTypes = new Set(["feature", "feat"]);
  const correctiveTypes = new Set(["fix", "revert", "perf"]);
  const candidates = [];
  for (let index = 0; index < commits.length; index += 1) {
    const anchor = commits[index];
    if (!featureTypes.has(anchor.type) || !anchor.subject_capabilities.length || anchor.subject_capabilities.length > 4) continue;
    const anchorTime = Date.parse(anchor.date);
    const followups = [];
    for (let cursor = index + 1; cursor < commits.length; cursor += 1) {
      const candidate = commits[cursor];
      const ageDays = (Date.parse(candidate.date) - anchorTime) / 86_400_000;
      if (ageDays > 4) break;
      if (!correctiveTypes.has(candidate.type)) continue;
      if (candidate.subject_capabilities[0] !== anchor.subject_capabilities[0]) continue;
      followups.push(candidate);
    }
    if (followups.length >= 2) candidates.push({ anchor, followups });
  }
  return candidates
    .sort((left, right) => right.followups.length - left.followups.length || left.anchor.sequence - right.anchor.sequence)
    .slice(0, 30);
}

function buildChainReport(commits) {
  const candidates = candidateRegressionChains(commits);
  const lines = [
    "# Legacy regression-chain candidates",
    "",
    "Generated from commit subjects and dates. Candidates share their primary subject capability and occur within four days. These are review candidates, not proof of causality; each accepted chain must be checked against its diff and final stable replacement.",
    "",
  ];
  for (const { anchor, followups } of candidates) {
    lines.push(`## ${anchor.short_hash} — ${anchor.subject}`, "");
    lines.push(`- Date: ${anchor.date.slice(0, 10)}`);
    lines.push(`- Subject capability overlap: ${anchor.subject_capabilities.join(", ")}`);
    lines.push(`- Corrective follow-ups within four days: ${followups.length}`);
    for (const followup of followups.slice(0, 12)) lines.push(`  - ${followup.short_hash} (${followup.date.slice(0, 10)}): ${followup.subject}`);
    if (followups.length > 12) lines.push(`  - ... ${followups.length - 12} more candidates remain in the complete ledger`);
    lines.push("");
  }
  return lines.join("\n");
}

function buildSummary(commits) {
  const countBy = (selector) => Object.fromEntries([...commits.reduce((map, commit) => {
    for (const key of selector(commit)) map.set(key, (map.get(key) || 0) + 1);
    return map;
  }, new Map())].sort((left, right) => right[1] - left[1] || left[0].localeCompare(right[0])));
  return {
    schema_version: 1,
    generated_from: { first: commits[0]?.hash, last: commits.at(-1)?.hash },
    commits: commits.length,
    first_date: commits[0]?.date,
    last_date: commits.at(-1)?.date,
    by_type: countBy((commit) => [commit.type]),
    by_capability: countBy((commit) => commit.capabilities),
    commits_with_references: commits.filter((commit) => commit.references.length).length,
    regression_chain_candidates: candidateRegressionChains(commits).length,
  };
}

function selfTest() {
  const commits = parseLog("@@@abc123456789\x1f2026-02-07T00:00:00Z\x1fAdd mobile terminal\x1fbody\n3\t1\tweb/terminal.ts\n@@@def123456789\x1f2026-02-08T00:00:00Z\x1ffix(tasks): prevent duplicate assignment #42\x1f\n4\t2\tsrc/tasks.py\n");
  if (commits.length !== 2) fail("self-test: commit parsing failed");
  if (!commits[0].capabilities.includes("terminal") || !commits[0].capabilities.includes("mobile_pwa")) fail("self-test: capability parsing failed");
  if (commits[1].type !== "fix" || commits[1].references[0] !== "#42") fail("self-test: metadata parsing failed");
  process.stdout.write("legacy ledger self-test passed\n");
}

if (process.argv.includes("--self-test")) {
  selfTest();
  process.exit(0);
}

const source = path.resolve(process.argv[2] || "../swarm");
const output = path.resolve(process.argv[3] || "docs/legacy");
const revision = process.argv[4] || "HEAD";
const git = spawnSync("git", ["-c", `safe.directory=${source.replace(/\\/g, "/")}`, "-C", source, "log", revision, "--reverse", "--date=iso-strict", "--numstat", "--format=@@@%H%x1f%ad%x1f%s%x1f%b"], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
if (git.status !== 0) fail(git.stderr || "legacy git log failed");
const commits = parseLog(git.stdout);
if (!commits.length) fail("legacy history was empty");
fs.mkdirSync(output, { recursive: true });
fs.writeFileSync(path.join(output, "commit-capability-ledger.csv"), buildLedger(commits));
fs.writeFileSync(path.join(output, "regression-chain-candidates.md"), buildChainReport(commits));
fs.writeFileSync(path.join(output, "commit-capability-summary.json"), `${JSON.stringify(buildSummary(commits), null, 2)}\n`);
process.stdout.write(`wrote ${commits.length} commits to ${output}\n`);
