#!/usr/bin/env node

const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { chromium } = require("playwright");

const baseUrl = process.env.SWARM_BASE_URL || "http://127.0.0.1:8766";
const operatorToken = process.env.SWARM_OPERATOR_TOKEN;
const browserExecutable = process.env.SWARM_BROWSER_EXECUTABLE;
const outputRoot = process.env.SWARM_BROWSER_EVIDENCE || path.resolve("dist", "browser-smoke");
const compactOutput = process.env.SWARM_BROWSER_COMPACT === "1";
const resultPath = process.env.SWARM_BROWSER_RESULT;

if (!operatorToken) {
  throw new Error("SWARM_OPERATOR_TOKEN is required");
}

const configuredSurfaces = [
  { name: "desktop", viewport: { width: 1440, height: 900 }, mobile: false },
  { name: "mobile", viewport: { width: 412, height: 915 }, mobile: true },
];
const requestedSurfaceNames = new Set((process.env.SWARM_BROWSER_SURFACES || "desktop,mobile").split(",").map((name) => name.trim()).filter(Boolean));
const surfaces = configuredSurfaces.filter((surface) => requestedSurfaceNames.has(surface.name));
if (!surfaces.length) throw new Error("SWARM_BROWSER_SURFACES must include desktop or mobile");

async function main() {
  await fs.mkdir(outputRoot, { recursive: true });
  const browser = await chromium.launch({
    headless: true,
    ...(browserExecutable ? { executablePath: browserExecutable } : {}),
  });
  const results = [];
  const memberResults = [];
  try {
    for (const surface of surfaces) {
      results.push(await checkSurface(browser, surface));
    }
    for (const surface of surfaces) {
      memberResults.push(await checkMemberSurface(browser, surface));
    }
  } finally {
    await browser.close();
  }
  const browserRestartPersistence = await verifyBrowserRestartPersistence();
  const report = { baseUrl, results, memberResults, browserRestartPersistence };
  const output = compactOutput ? {
    baseUrl,
    surfaces: results.map(({ surface, status, surfaces, accessibleControlCount, apiaryGuideSteps }) => ({
      surface, status, surfaces, accessibleControlCount, apiaryGuideSteps,
    })),
    memberSurfaces: memberResults,
    browserRestartPersistence,
  } : report;
  const serialized = `${JSON.stringify(output, null, 2)}\n`;
  if (resultPath) await fs.writeFile(resultPath, serialized, "utf8");
  process.stdout.write(serialized);
}

async function checkMemberSurface(browser, surface) {
  const context = await browser.newContext({
    viewport: surface.viewport,
    isMobile: surface.mobile,
    hasTouch: surface.mobile,
    ...(surface.mobile ? { userAgent: "Mozilla/5.0 (Linux; Android 15; Swarm Dogfood) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36" } : {}),
  });
  const page = await context.newPage();
  const errors = [];
  page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
  page.on("pageerror", (error) => errors.push(error.message));
  const fulfill = (route, payload) => route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(payload) });
  await page.route("**/api/v1/hive", (route) => fulfill(route, {
    operator: { id: "visual-member-operator", display_name: "Cora" },
    hive: { id: "visual-member-hive", name: "Clover Hive", operator_id: "visual-member-operator", apiary_id: "visual-apiary" },
    apiary_context: { mode: "federated", apiary: { id: "visual-apiary", name: "Grand Garden", keeper_operator_id: "visual-keeper-operator", shared_work_backend: "jira" }, local_role: "member" },
  }));
  await page.route("**/api/v1/apiary/members", (route) => fulfill(route, [
    { hive_id: "visual-keeper-hive", hive_name: "Meadow Hive", operator_id: "visual-keeper-operator", operator_display_name: "Bea", role: "keeper", is_local: false },
    { hive_id: "visual-member-hive", hive_name: "Clover Hive", operator_id: "visual-member-operator", operator_display_name: "Cora", role: "member", is_local: true },
  ]));
  await page.route("**/api/v1/apiary/shared-work", (route) => fulfill(route, [{
    id: "visual-claim", apiary_id: "visual-apiary", project_id: "10001", issue_id: "20001", issue_key: "WWD-101", home_node_id: "private-node", home_hive_id: "visual-member-hive", home_operator_id: "visual-member-operator", state: "confirmed", reserved_at: 1, reservation_expires_at: 2, confirmed_at: 2, released_at: null, project_key: "WWD", project_name: "Website Development", home_hive_name: "Clover Hive", home_operator_display_name: "Cora",
  }]));
  await page.route("**/api/v1/apiary/sync-health", (route) => fulfill(route, { condition: "current", last_attempt_at: 1_786_780_000, last_success_at: 1_786_780_000, consecutive_failures: 0, next_attempt_at: null }));
  await page.route("**/api/v1/apiary/catalog-readiness", (route) => fulfill(route, {
    acknowledgement: { apiary_id: "visual-apiary", policy_revision: 1, promoted_project_catalog_digest: "private-digest", project_count: 2, snapshot_issued_at: 1, snapshot_expires_at: 2, acknowledged_at: 1 },
    jira_connection: "ready",
    projects: [
      { project: { project_id: "10001", project_key: "WWD", project_name: "Website Development" }, binding_id: "binding-1", access_verified: true, workflow_mapped: true },
      { project: { project_id: "10002", project_key: "IT", project_name: "Information Technology" }, binding_id: null, access_verified: false, workflow_mapped: false },
    ],
    blockers: ["project_access_not_ready"],
  }));

  try {
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    const tokenInput = page.getByLabel("Operator token");
    if (await tokenInput.isVisible().catch(() => false)) {
      await tokenInput.fill(operatorToken);
      await page.getByRole("button", { name: "Unlock Swarm" }).click();
    }
    await page.getByRole("button", { name: /Workers/ }).waitFor();
    // A fresh browser context probes the cookie session before the explicit
    // unlock. Its expected 401 is not an authenticated Member-surface error.
    errors.length = 0;
    await page.getByRole("button", { name: "Apiary", exact: true }).click();
    const memberControl = page.locator(".member-control-room");
    await memberControl.waitFor();
    await page.getByRole("heading", { name: "Grand Garden" }).waitFor();
    await page.getByText("Meadow Hive", { exact: true }).first().waitFor();
    await page.getByRole("list", { name: "Member promoted Jira projects" }).waitFor();
    await page.getByRole("list", { name: "Member shared work ownership" }).waitFor();
    if (await page.getByText("private-node", { exact: true }).count() || await page.getByText("private-digest", { exact: true }).count()) {
      throw new Error(`${surface.name}/member-apiary: private federation material was rendered`);
    }
    const dimensions = await page.evaluate(() => ({ scrollWidth: document.documentElement.scrollWidth, clientWidth: document.documentElement.clientWidth }));
    if (dimensions.scrollWidth > dimensions.clientWidth + 1) {
      throw new Error(`${surface.name}/member-apiary: horizontal overflow ${dimensions.scrollWidth}px > ${dimensions.clientWidth}px`);
    }
    const accessibleControlCount = await verifyAccessibleControls(page, `${surface.name}/member-apiary`);
    await page.screenshot({ path: path.join(outputRoot, `${surface.name}-apiary-member.png`), fullPage: true });
    if (surface.mobile) {
      await memberControl.locator(".keeper-panel").last().scrollIntoViewIfNeeded();
      await page.screenshot({ path: path.join(outputRoot, `${surface.name}-apiary-member-lower.png`), fullPage: true });
    }
    if (errors.length) throw new Error(`${surface.name}/member-apiary: browser errors: ${errors.join(" | ")}`);
    return { surface: surface.name, ...dimensions, accessibleControlCount, status: "passed" };
  } finally {
    await context.close();
  }
}

async function verifyBrowserRestartPersistence() {
  const results = [];
  for (const surface of surfaces) {
    const profileDirectory = await fs.mkdtemp(path.join(os.tmpdir(), `swarm-next-${surface.name}-smoke-`));
    const launchOptions = {
      headless: true,
      viewport: surface.viewport,
      isMobile: surface.mobile,
      hasTouch: surface.mobile,
      ...(surface.mobile ? { userAgent: "Mozilla/5.0 (Linux; Android 15; Swarm Dogfood) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36" } : {}),
      ...(browserExecutable ? { executablePath: browserExecutable } : {}),
    };

    try {
      let context = await chromium.launchPersistentContext(profileDirectory, launchOptions);
      try {
        const page = context.pages()[0] || await context.newPage();
        await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
        const tokenInput = page.getByLabel("Operator token");
        if (await tokenInput.isVisible().catch(() => false)) {
          await tokenInput.fill(operatorToken);
          await page.getByRole("button", { name: "Unlock Swarm" }).click();
        }
        await page.getByRole("button", { name: /Workers/ }).waitFor();
      } finally {
        await context.close();
      }

      context = await chromium.launchPersistentContext(profileDirectory, launchOptions);
      try {
        const page = context.pages()[0] || await context.newPage();
        await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
        await page.getByRole("button", { name: /Workers/ }).waitFor();
        if (await page.getByLabel("Operator token").isVisible().catch(() => false)) {
          throw new Error(`${surface.name}: browser session did not survive a complete browser restart`);
        }
        results.push({ surface: surface.name, preservedAcrossBrowserRestart: true, status: "passed" });
      } finally {
        await context.close();
      }
    } finally {
      await fs.rm(profileDirectory, { recursive: true, force: true });
    }
  }
  return results;
}

async function checkSurface(browser, surface) {
  const context = await browser.newContext({
    viewport: surface.viewport,
    isMobile: surface.mobile,
    hasTouch: surface.mobile,
    ...(surface.mobile ? { userAgent: "Mozilla/5.0 (Linux; Android 15; Swarm Dogfood) AppleWebKit/537.36 Chrome/140 Mobile Safari/537.36" } : {}),
  });
  let page = await context.newPage();
  const consoleErrors = [];
  const pageErrors = [];
  const observePage = (candidate) => {
    candidate.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });
    candidate.on("pageerror", (error) => pageErrors.push(error.message));
  };
  observePage(page);

  try {
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    const tokenInput = page.getByLabel("Operator token");
    if (await tokenInput.isVisible().catch(() => false)) {
      await tokenInput.fill(operatorToken);
      await page.getByRole("button", { name: "Unlock Swarm" }).click();
    }
    await page.getByRole("button", { name: /Workers/ }).waitFor();
    await page.reload({ waitUntil: "domcontentloaded" });
    await page.getByRole("button", { name: /Workers/ }).waitFor();
    if (await tokenInput.isVisible().catch(() => false)) {
      throw new Error(`${surface.name}: browser session did not survive reload`);
    }
    await page.close();
    page = await context.newPage();
    observePage(page);
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    await page.getByRole("button", { name: /Workers/ }).waitFor();
    const hiveContext = page.locator(".hive-context-indicator:visible").first();
    await hiveContext.waitFor();
    if (!/Hive/i.test(await hiveContext.getAttribute("aria-label") || "")) {
      throw new Error(`${surface.name}: current Hive and Apiary role are not visible in the control room`);
    }
    if (await page.getByLabel("Operator token").isVisible().catch(() => false)) {
      throw new Error(`${surface.name}: browser session did not survive page reopen`);
    }
    // A fresh context probes the saved session before the first explicit unlock;
    // discard that expected 401 and inspect only the authenticated app below.
    consoleErrors.length = 0;
    pageErrors.length = 0;

    const surfaceResults = [];
    let accessibleControlCount = 0;
    let workerSelections = [];
    let completedTaskCount = 0;
    let completedTaskTitle;
    let jiraIssueReview = false;
    let jiraIssueFilter = false;
    let emailIntakeReview = false;
    let activityReview = false;
    const navigationTargets = [
      { name: "needs-you", nav: /Needs you/, ready: () => page.getByRole("heading", { name: "Needs you" }) },
      { name: "tasks", nav: /Tasks/, ready: () => page.getByRole("heading", { name: "Task board" }) },
      { name: "workers", nav: /Workers/, ready: () => page.locator(".terminal-panel") },
      { name: "settings", nav: /Settings/, ready: () => page.getByRole("heading", { name: "Settings" }) },
    ];
    if (await page.getByRole("button", { name: "Apiary", exact: true }).count()) {
      navigationTargets.splice(3, 0, { name: "apiary", nav: /^Apiary$/, ready: () => page.locator(".keeper-control-room") });
    }
    for (const target of navigationTargets) {
      await page.getByRole("button", { name: target.nav }).click();
      await target.ready().first().waitFor();
      if (target.name === "needs-you") {
        await page.getByRole("tab", { name: "Activity", exact: true }).click();
        await page.getByRole("heading", { name: "What changed recently" }).waitFor();
        const activityRows = page.locator(".work-activity-list > li");
        if (await activityRows.count() === 0) throw new Error(`${surface.name}: durable work activity is empty`);
        const activityBounds = await page.locator(".work-activity").evaluate((element) => ({
          scrollWidth: element.scrollWidth,
          clientWidth: element.clientWidth,
        }));
        if (activityBounds.scrollWidth > activityBounds.clientWidth + 1) {
          throw new Error(`${surface.name}: durable work activity is horizontally clipped`);
        }
        await page.getByLabel("Show").selectOption("changes");
        if (await activityRows.count() === 0) throw new Error(`${surface.name}: change activity filter has no durable events`);
        await page.getByRole("button", { name: "Refresh" }).click();
        await activityRows.first().waitFor();
        await page.screenshot({ path: path.join(outputRoot, `${surface.name}-activity.png`), fullPage: true });
        activityReview = true;
        await page.getByRole("tab", { name: /Needs you/ }).click();
      }
      if (target.name === "tasks") {
        const taskTitleVisible = await page.getByLabel("Task title").isVisible().catch(() => false);
        if (taskTitleVisible) throw new Error(`${surface.name}: task composer obscures active work by default`);
        if (surface.mobile) {
          await page.getByRole("heading", { name: "Active work" }).waitFor();
        }
        await page.getByRole("button", { name: "Choose Jira work" }).click();
        const jiraSource = page.getByRole("region", { name: "Choose unassigned work from Jira" });
        await jiraSource.waitFor({ state: "visible", timeout: 15_000 }).catch(() => undefined);
        if (await jiraSource.isVisible().catch(() => false)) {
          const chooseWork = jiraSource.locator(".jira-project-actions button").first();
          await chooseWork.click();
          const intake = jiraSource.getByRole("region", { name: /Choose .* work/ });
          await intake.waitFor();
          const issueCount = await intake.getByRole("checkbox").count();
          const intakeBounds = await intake.evaluate((element) => ({
            scrollWidth: element.scrollWidth,
            clientWidth: element.clientWidth,
          }));
          if (issueCount === 0 || intakeBounds.scrollWidth > intakeBounds.clientWidth + 1) {
            throw new Error(`${surface.name}: Jira task intake is empty or horizontally clipped`);
          }
          if (!/unassigned .*open only/i.test(await intake.innerText())) {
            throw new Error(`${surface.name}: Jira task intake does not explain its Hive scope`);
          }
          const checkedCount = await intake.getByRole("checkbox", { checked: true }).count();
          const addButton = intake.getByRole("button", { name: "Add 0 to this board" });
          if (checkedCount !== 0 || !(await addButton.isDisabled())) {
            throw new Error(`${surface.name}: Jira task intake did not open with a safe empty selection`);
          }
          const firstIssueLabel = await intake.getByRole("checkbox").first().evaluate((input) => input.labels?.[0]?.innerText ?? "");
          const firstIssueKey = firstIssueLabel.match(/[A-Z][A-Z0-9]+-\d+/)?.[0];
          if (!firstIssueKey) throw new Error(`${surface.name}: Jira task intake did not expose an issue key`);
          await intake.getByLabel("Find an issue").fill(firstIssueKey);
          const filteredCount = await intake.getByRole("checkbox").count();
          if (filteredCount === 0 || filteredCount > issueCount) {
            throw new Error(`${surface.name}: Jira task filtering did not retain the matching issue`);
          }
          jiraIssueFilter = true;
          await page.screenshot({ path: path.join(outputRoot, `${surface.name}-tasks-jira-intake.png`), fullPage: true });
          await intake.getByRole("button", { name: "Close" }).click();
          jiraIssueReview = true;
        }
        const emailToggle = page.locator(".task-entry-actions button").filter({ hasText: /^(Import email|Close email)$/ });
        if (await emailToggle.count()) {
          await emailToggle.scrollIntoViewIfNeeded();
          if (await emailToggle.innerText() === "Close email") await emailToggle.click();
          await page.getByRole("button", { name: "Import email" }).click();
          const emailSource = page.locator(".email-task-source");
          await emailSource.waitFor();
          const inbox = emailSource.getByRole("list", { name: "Inbox messages" });
          await inbox.waitFor({ state: "visible", timeout: 15_000 }).catch(() => undefined);
          if (await inbox.isVisible().catch(() => false)) {
            const emailChoices = inbox.getByRole("checkbox");
            await emailChoices.first().waitFor({ state: "visible", timeout: 15_000 }).catch(() => undefined);
            const emailCount = await emailChoices.count();
            if (emailCount === 0) throw new Error(`${surface.name}: connected email intake is empty`);
            const reviewCount = Math.min(emailCount, 2);
            for (let index = 0; index < reviewCount; index += 1) await emailChoices.nth(index).check();
            await emailSource.getByRole("button", {
              name: `Review ${reviewCount} message${reviewCount === 1 ? "" : "s"}`,
              exact: true,
            }).click();
            await emailSource.getByRole("heading", { name: "Review the task before import" }).waitFor();
            await emailSource.getByRole("heading", { name: "Shape the work before it joins the board" }).waitFor();
            await page.screenshot({ path: path.join(outputRoot, `${surface.name}-tasks-email-review.png`), fullPage: true });
            for (const field of ["Task title", "Task description", "Priority", "Starting status", "Worker"]) {
              const control = emailSource.getByLabel(field, { exact: true });
              if (!await control.isVisible()) {
                const diagnostics = await control.evaluateAll((elements) => elements.map((element) => {
                  const bounds = element.getBoundingClientRect();
                  const style = getComputedStyle(element);
                  return {
                    tag: element.tagName,
                    bounds: { x: bounds.x, y: bounds.y, width: bounds.width, height: bounds.height },
                    display: style.display,
                    visibility: style.visibility,
                  };
                }));
                throw new Error(`${surface.name}: email import field ${field} is unavailable (${JSON.stringify(diagnostics)})`);
              }
            }
            if (surface.mobile) {
              const sourcePicker = emailSource.getByLabel("Preview source email", { exact: true });
              if (!await sourcePicker.isVisible()) {
                throw new Error(`${surface.name}: compact email source selector is unavailable`);
              }
              await sourcePicker.scrollIntoViewIfNeeded();
              await page.screenshot({ path: path.join(outputRoot, `${surface.name}-tasks-email-source-review.png`), fullPage: true });
            } else if (!await emailSource.getByRole("tablist", { name: "Selected source emails" }).isVisible()) {
              throw new Error(`${surface.name}: desktop email source tabs are unavailable`);
            }
            const reviewBounds = await emailSource.evaluate((element) => ({
              scrollWidth: element.scrollWidth,
              clientWidth: element.clientWidth,
            }));
            if (reviewBounds.scrollWidth > reviewBounds.clientWidth + 1) {
              throw new Error(`${surface.name}: email task review is horizontally clipped`);
            }
            emailIntakeReview = true;
          }
          await page.getByRole("button", { name: "Close email" }).click();
        }
        const completedTasks = page.locator("details.completed-tasks");
        if (await completedTasks.count()) {
          await completedTasks.locator("summary").click();
          completedTaskCount = await completedTasks.locator(".task-card").count();
          completedTaskTitle = await completedTasks.locator(".task-card h4").first().innerText();
          const overflowingCompletedCards = await completedTasks.locator(".task-card").evaluateAll((cards) => cards.filter((card) => card.scrollWidth > card.clientWidth + 1).length);
          const overlappingCompletedCopy = await completedTasks.locator(".task-card").evaluateAll((cards) => cards.filter((card) => {
            const title = card.querySelector("h4");
            const description = card.querySelector(".task-description");
            return title && description && description.getBoundingClientRect().top < title.getBoundingClientRect().bottom + 2;
          }).length);
          if (completedTaskCount === 0 || overflowingCompletedCards > 0 || overlappingCompletedCopy > 0) {
            throw new Error(`${surface.name}: completed work is not a usable task history`);
          }
          await page.screenshot({ path: path.join(outputRoot, `${surface.name}-tasks-completed.png`), fullPage: true });
          await completedTasks.locator("summary").click();
        }
      }
      if (target.name === "workers") {
        workerSelections = await verifyRunningWorkerSelection(page, surface.name);
        if (surface.mobile && !await page.getByRole("button", { name: "Add image" }).isVisible()) {
          throw new Error(`${surface.name}: terminal image picker is unavailable`);
        }
      }
      await page.waitForTimeout(250);
      const dimensions = await page.evaluate(() => ({
        scrollWidth: document.documentElement.scrollWidth,
        clientWidth: document.documentElement.clientWidth,
      }));
      if (dimensions.scrollWidth > dimensions.clientWidth + 1) {
        throw new Error(`${surface.name}/${target.name}: horizontal overflow ${dimensions.scrollWidth}px > ${dimensions.clientWidth}px`);
      }
      accessibleControlCount += await verifyAccessibleControls(page, `${surface.name}/${target.name}`);
      await page.screenshot({
        path: path.join(outputRoot, `${surface.name}-${target.name}.png`),
        fullPage: true,
      });
      surfaceResults.push({ surface: target.name, ...dimensions });
    }

    if (surface.mobile) {
      await page.getByRole("button", { name: /Tasks/ }).click();
      const createTask = page.getByRole("button", { name: "Create task" });
      await createTask.click();
      await page.getByLabel("Task title").waitFor();
      if (await page.getByRole("button", { name: "Close task form" }).getAttribute("aria-expanded") !== "true") {
        throw new Error(`${surface.name}: task composer did not expose its expanded state`);
      }
      await page.screenshot({ path: path.join(outputRoot, `${surface.name}-tasks-compose.png`), fullPage: true });
      await page.getByRole("button", { name: /Settings/ }).click();
      await page.getByRole("heading", { name: "Settings" }).waitFor();
    }

    await page.getByRole("heading", { name: "Your familiar crew" }).waitFor();
    const repositoryPicker = page.getByRole("combobox", { name: "Repository", exact: true });
    if (await repositoryPicker.getAttribute("placeholder") !== "Search by name or path") {
      throw new Error(`${surface.name}: worker creation still leads with a filesystem path`);
    }
    if (!await page.getByText("Start with a repository name and Swarm completes the path. Full paths still work.", { exact: true }).isVisible()) {
      throw new Error(`${surface.name}: repository completion guidance is unavailable`);
    }
    if (await page.getByRole("button", { name: /Settings 3/ }).count()) {
      throw new Error(`${surface.name}: Settings exposes a false pending-item count`);
    }
    const provider = page.getByLabel("Coding provider");
    const codexDisabled = await provider.locator('option[value="codex"]').isDisabled();
    const workerEngineRow = page.getByText("Worker engine", { exact: true }).locator("..");
    await workerEngineRow.getByText(/^(Current|Update ready)/).waitFor();
    const workerEngineText = await workerEngineRow.innerText();
    if (!/Current|Update ready/.test(workerEngineText)) {
      throw new Error(`${surface.name}: worker-engine maintenance state is unclear`);
    }
    const appApiStatus = page.getByLabel("App and API status");
    await appApiStatus.getByText(/^(App and API are current|Development reload available|Building development changes|Development build failed)$/).waitFor();
    const appApiText = await appApiStatus.innerText();
    const normalizedAppApiText = appApiText.toLocaleLowerCase();
    await appApiStatus.scrollIntoViewIfNeeded();
    await page.screenshot({ path: path.join(outputRoot, `${surface.name}-settings-runtime.png`), fullPage: true });
    if (
      normalizedAppApiText.includes("app and api are current")
      && (!normalizedAppApiText.includes("activated") || !normalizedAppApiText.includes("no reload is pending"))
    ) {
      throw new Error(`${surface.name}: current App/API state does not explain its activation or detector result (${JSON.stringify(appApiText)})`);
    }
    let maintenanceConfirmation = false;
    if (workerEngineText.includes("Update ready")) {
      await page.getByRole("button", { name: "Prepare worker engine update" }).click();
      const confirmation = page.getByRole("group", { name: "Confirm worker engine update" });
      await confirmation.waitFor();
      const dimensions = await page.evaluate(() => ({
        scrollWidth: document.documentElement.scrollWidth,
        clientWidth: document.documentElement.clientWidth,
      }));
      if (dimensions.scrollWidth > dimensions.clientWidth + 1) {
        throw new Error(`${surface.name}/maintenance: horizontal overflow ${dimensions.scrollWidth}px > ${dimensions.clientWidth}px`);
      }
      await page.screenshot({
        path: path.join(outputRoot, `${surface.name}-settings-maintenance.png`),
        fullPage: true,
      });
      await page.getByRole("button", { name: "Not now" }).click();
      maintenanceConfirmation = true;
    }
    if (consoleErrors.length || pageErrors.length) {
      throw new Error(`${surface.name}: browser errors: ${[...consoleErrors, ...pageErrors].join(" | ")}`);
    }
    await page.getByRole("button", { name: "Report a problem" }).click();
    const feedbackDialog = page.getByRole("dialog", { name: "Capture what felt wrong" });
    await feedbackDialog.waitFor();
    await page.getByLabel("What did you expect?").fill("Acceptance preview only");
    await feedbackDialog.evaluate((dialog) => {
      const clipboard = new DataTransfer();
      clipboard.items.add(new File([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], "acceptance-screenshot.png", { type: "image/png" }));
      dialog.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: clipboard }));
    });
    await page.getByText("acceptance-screenshot.png", { exact: true }).waitFor();
    const feedbackImagePaste = true;
    const privateSaveVisible = await page.getByRole("button", { name: "Save to this Hive" }).isVisible();
    if (!privateSaveVisible) throw new Error(`${surface.name}: private feedback save is unavailable`);
    await page.getByRole("button", { name: "Close" }).click();
    const savedFeedbackVisible = await page.getByRole("heading", { name: "Saved dogfood reports" }).isVisible();
    if (!savedFeedbackVisible) throw new Error(`${surface.name}: saved feedback queue is unavailable`);
    const settingsNavigation = page.getByRole("navigation", { name: "Settings sections" });
    if (!await settingsNavigation.isVisible()) throw new Error(`${surface.name}: Settings section navigation is unavailable`);
    const settingsNavigationSize = await settingsNavigation.evaluate((navigation) => ({
      scrollWidth: navigation.scrollWidth,
      clientWidth: navigation.clientWidth,
      clientHeight: navigation.clientHeight,
      button: navigation.firstElementChild ? {
        height: navigation.firstElementChild.getBoundingClientRect().height,
        display: getComputedStyle(navigation.firstElementChild).display,
        minHeight: getComputedStyle(navigation.firstElementChild).minHeight,
        visibility: getComputedStyle(navigation.firstElementChild).visibility,
      } : null,
    }));
    if (settingsNavigationSize.clientHeight < 40) {
      throw new Error(`${surface.name}: Settings section navigation collapsed: ${JSON.stringify(settingsNavigationSize)}`);
    }
    const apiaryJump = settingsNavigation.getByRole("button", { name: "Apiary", exact: true });
    await apiaryJump.click();
    await page.getByRole("button", { name: "Edit names" }).click();
    const identityEditor = page.getByRole("group", { name: "Hive and Apiary names" });
    await identityEditor.waitFor();
    const identityFields = await identityEditor.locator("input").count();
    const identityEditorOverflow = await identityEditor.evaluate((editor) => ({
      scrollWidth: editor.scrollWidth,
      clientWidth: editor.clientWidth,
      overflowingControls: [...editor.querySelectorAll("input, button")]
        .filter((control) => control.scrollWidth > control.clientWidth + 1).length,
    }));
    if (identityFields < 1 || identityEditorOverflow.scrollWidth > identityEditorOverflow.clientWidth + 1 || identityEditorOverflow.overflowingControls > 0) {
      throw new Error(`${surface.name}: Hive and Apiary name editor is incomplete or overflowing`);
    }
    await page.screenshot({
      path: path.join(outputRoot, `${surface.name}-settings-apiary-names.png`),
      fullPage: true,
    });
    await page.getByRole("button", { name: "Close names" }).click();
    const personalGuide = page.getByRole("list", { name: "How to join an Apiary" });
    const keeperGuide = page.getByRole("list", { name: "How to invite a Hive" });
    const apiaryGuide = await personalGuide.isVisible().catch(() => false) ? personalGuide : keeperGuide;
    await apiaryGuide.waitFor();
    const apiaryGuideSteps = await apiaryGuide.locator(":scope > li").count();
    const personalExchange = await personalGuide.isVisible().catch(() => false);
    const firstExchangeControl = personalExchange
      ? page.getByRole("button", { name: "Copy connection link" })
      : page.getByLabel("Hive connection link");
    const secondExchangeControl = personalExchange
      ? page.getByLabel("Invitation link")
      : page.getByText("Hives in this Apiary", { exact: true });
    const apiaryOverflow = await page.locator("#settings-apiary").evaluate((section) => ({
      scrollWidth: section.scrollWidth,
      clientWidth: section.clientWidth,
      overflowingSteps: [...section.querySelectorAll(".apiary-exchange-guide > li")]
        .filter((step) => step.scrollWidth > step.clientWidth + 1).length,
      overflowingDrops: [...section.querySelectorAll(".apiary-card-drop")]
        .filter((drop) => drop.scrollWidth > drop.clientWidth + 1).length,
    }));
    if (apiaryGuideSteps !== 3 || !await firstExchangeControl.isVisible() || !await secondExchangeControl.isVisible()) {
      throw new Error(`${surface.name}: Apiary invitation exchange is incomplete`);
    }
    if (apiaryOverflow.scrollWidth > apiaryOverflow.clientWidth + 1 || apiaryOverflow.overflowingSteps > 0 || apiaryOverflow.overflowingDrops > 0) {
      throw new Error(`${surface.name}: Apiary invitation exchange overflows its layout`);
    }
    await page.screenshot({
      path: path.join(outputRoot, `${surface.name}-settings-apiary.png`),
      fullPage: true,
    });
    const diagnosticsJump = settingsNavigation.getByRole("button", { name: "Diagnostics", exact: true });
    await diagnosticsJump.click();
    if (await diagnosticsJump.getAttribute("aria-current") !== "location") {
      throw new Error(`${surface.name}: Settings section selection is not exposed`);
    }
    const diagnosticsHeading = page.getByRole("heading", { name: "Know which layer needs attention" });
    await diagnosticsHeading.waitFor();
    await page.waitForTimeout(400);
    const diagnosticsTop = await diagnosticsHeading.evaluate((heading) => heading.getBoundingClientRect().top);
    if (diagnosticsTop < 0 || diagnosticsTop > surface.viewport.height) {
      throw new Error(`${surface.name}: Settings section jump did not reveal Diagnostics`);
    }
    const settingsOverflow = await page.locator(".settings-workspace").evaluate((workspace) => ({
      scrollWidth: workspace.scrollWidth,
      clientWidth: workspace.clientWidth,
      overflowingCards: [...workspace.querySelectorAll(".settings-card")]
        .filter((card) => card.scrollWidth > card.clientWidth + 1).length,
    }));
    if (settingsOverflow.scrollWidth > settingsOverflow.clientWidth + 1 || settingsOverflow.overflowingCards > 0) {
      throw new Error(`${surface.name}/settings-detail: internal horizontal overflow`);
    }
    await page.screenshot({
      path: path.join(outputRoot, `${surface.name}-settings-diagnostics.png`),
      fullPage: true,
    });
    const jiraRegion = page.getByRole("region", { name: "Bring Jira into your Hive" });
    await jiraRegion.waitFor();
    await page.waitForFunction(() => {
      const status = document.querySelector("#settings-integrations .integration-status")?.textContent || "";
      return /Jira (?:not connected|connected|credentials need attention|access was denied|is temporarily unavailable)/i.test(status)
        || /Connected as /i.test(status);
    }, undefined, { timeout: 15_000 });
    const jiraReadiness = await jiraRegion.locator(".integration-status").innerText();
    if (!/Jira (?:not connected|connected|credentials need attention|access was denied|is temporarily unavailable)/i.test(jiraReadiness)
      && !/Connected as /i.test(jiraReadiness)) {
      throw new Error(`${surface.name}: Jira readiness is unavailable`);
    }
    const restoreGuide = page.getByText("How to restore this backup", { exact: true });
    await restoreGuide.click();
    const restoreCommandVisible = await page.getByText(/swarm-next-package restore/).isVisible();
    if (!restoreCommandVisible) throw new Error(`${surface.name}: Hive restore guidance is unavailable`);
    await page.keyboard.press("Alt+K");
    await page.getByRole("dialog", { name: "Where would you like to go?" }).waitFor();
    const addWorkerShortcutVisible = await page.getByRole("option", { name: /Add worker Configure a repository worker/ }).isVisible();
    if (!addWorkerShortcutVisible) throw new Error(`${surface.name}: worker creation is not discoverable from quick navigation`);
    let commandSearch = page.getByRole("combobox", { name: "Find work, decisions, or workers" });
    if (completedTaskTitle) {
      await commandSearch.fill(completedTaskTitle);
      const completedChoice = page.getByRole("option").filter({ hasText: completedTaskTitle }).filter({ hasText: /Completed/i }).first();
      await completedChoice.waitFor();
      await completedChoice.click();
      const completedCard = page.locator(".completed-tasks .task-card", { has: page.getByRole("heading", { name: completedTaskTitle, exact: true }) });
      await completedCard.waitFor();
      const completedTaskFocused = await completedCard.evaluate((card) => card === document.activeElement && card.closest("details")?.open === true);
      if (!completedTaskFocused) throw new Error(`${surface.name}: completed-task search did not reveal and focus the result`);
      await page.getByRole("button", { name: "Open quick navigation" }).click();
      commandSearch = page.getByRole("combobox", { name: "Find work, decisions, or workers" });
    }
    await commandSearch.fill("Create task");
    const commandBounds = await page.locator(".command-palette").evaluate((palette) => {
      const bounds = palette.getBoundingClientRect();
      return {
        left: bounds.left,
        right: bounds.right,
        top: bounds.top,
        bottom: bounds.bottom,
        scrollWidth: palette.scrollWidth,
        clientWidth: palette.clientWidth,
      };
    });
    if (commandBounds.left < 0 || commandBounds.right > surface.viewport.width || commandBounds.top < 0 || commandBounds.bottom > surface.viewport.height || commandBounds.scrollWidth > commandBounds.clientWidth + 1) {
      throw new Error(`${surface.name}: quick navigation exceeds the viewport`);
    }
    await page.screenshot({ path: path.join(outputRoot, `${surface.name}-quick-navigation.png`), fullPage: true });
    await commandSearch.press("Enter");
    await page.getByRole("heading", { name: "Task board" }).waitFor();
    const taskTitle = page.getByLabel("Task title");
    await taskTitle.waitFor();
    await page.waitForFunction(() => document.activeElement?.id === "task-title");
    const createTaskFocused = await taskTitle.evaluate((input) => input === document.activeElement);
    if (!createTaskFocused) throw new Error(`${surface.name}: quick task creation did not focus the title`);
    await page.getByRole("button", { name: /Settings/ }).click();
    await page.getByRole("button", { name: "Download Hive backup" }).waitFor();
    const backup = surface.mobile ? undefined : await verifyBackupDownload(page);
    accessibleControlCount += await verifyAccessibleControls(page, `${surface.name}/settings-detail`);
    return { surface: surface.name, surfaces: surfaceResults, workerSelections, completedTaskCount, accessibleControlCount, repositoryPicker: "name-first", addWorkerShortcutVisible, commandBounds, createTaskFocused, restoreCommandVisible, settingsNavigationSize, identityFields, identityEditorOverflow, apiaryGuideSteps, apiaryOverflow, diagnosticsTop, settingsOverflow, codexDisabled, workerEngineText, maintenanceConfirmation, feedbackImagePaste, privateSaveVisible, savedFeedbackVisible, jiraReadiness, jiraIssueReview, jiraIssueFilter, emailIntakeReview, activityReview, backup, status: "passed" };
  } finally {
    await context.close();
  }
}

async function verifyAccessibleControls(page, surfaceName) {
  const result = await page.locator("button, input, select, textarea, a[href]").evaluateAll((controls) => {
    const visible = controls.filter((control) => {
      if (control instanceof HTMLInputElement && control.type === "hidden") return false;
      const style = getComputedStyle(control);
      const bounds = control.getBoundingClientRect();
      return style.display !== "none" && style.visibility !== "hidden" && Number(style.opacity) !== 0 && bounds.width > 0 && bounds.height > 0;
    });
    const name = (control) => {
      const labelledBy = control.getAttribute("aria-labelledby")?.split(/\s+/)
        .map((id) => document.getElementById(id)?.textContent?.trim() || "")
        .filter(Boolean)
        .join(" ");
      const labels = "labels" in control && control.labels
        ? [...control.labels].map((label) => label.textContent?.trim() || "").filter(Boolean).join(" ")
        : "";
      return control.getAttribute("aria-label")?.trim()
        || labelledBy
        || labels
        || control.textContent?.trim()
        || control.getAttribute("title")?.trim()
        || "";
    };
    return {
      count: visible.length,
      unlabeled: visible.filter((control) => !name(control)).map((control) => ({
        tag: control.tagName.toLowerCase(),
        type: control.getAttribute("type"),
        className: control.className,
      })),
    };
  });
  if (result.unlabeled.length) {
    throw new Error(`${surfaceName}: visible controls without accessible names: ${JSON.stringify(result.unlabeled)}`);
  }
  return result.count;
}

async function verifyRunningWorkerSelection(page, surfaceName) {
  const workers = await page.evaluate(async () => {
    const response = await fetch("/api/v1/workers", { cache: "no-store", credentials: "same-origin" });
    if (!response.ok) throw new Error(`worker list returned ${response.status}`);
    return (await response.json()).filter((worker) => worker.running && worker.active_session_id)
      .map((worker) => ({ name: worker.name, sessionId: worker.active_session_id }));
  });
  if (workers.length === 0) throw new Error(`${surfaceName}: no running worker was available`);

  const selected = [];
  for (const worker of workers) {
    let button;
    if (surfaceName === "mobile") {
      await page.locator(".mobile-worker-switcher-trigger").click();
      const dialog = page.getByRole("dialog", { name: "Where do you want to work?" });
      await dialog.waitFor();
      button = dialog.locator(".mobile-worker-choice").filter({ hasText: worker.name }).first();
    } else {
      button = page.locator(".worker-row .worker-button").filter({ hasText: worker.name }).first();
    }
    await button.click();
    if (surfaceName === "mobile") {
      await page.locator(".mobile-worker-switcher-trigger").filter({ hasText: worker.name }).waitFor();
    } else {
      await page.getByRole("heading", { name: worker.name, exact: true }).waitFor();
    }
    await page.locator(".terminal-panel").waitFor();
    await page.getByText("connected", { exact: true }).waitFor();
    try {
      await page.waitForFunction(() => document.activeElement?.closest(".terminal-panel") !== null, undefined, { timeout: 5_000 });
    } catch {
      const focus = await page.evaluate(() => ({
        tag: document.activeElement?.tagName,
        className: document.activeElement instanceof HTMLElement ? document.activeElement.className : undefined,
        text: document.activeElement?.textContent?.trim().slice(0, 80),
      }));
      throw new Error(`${surfaceName}: selecting ${worker.name} did not focus its terminal: ${JSON.stringify(focus)}`);
    }
    const selectedWorker = surfaceName === "mobile"
      ? page.locator(".mobile-worker-switcher-trigger").filter({ hasText: worker.name })
      : button;
    if (surfaceName !== "mobile" && await selectedWorker.getAttribute("aria-current") !== "page") {
      throw new Error(`${surfaceName}: ${worker.name} did not become the selected terminal`);
    }
    selected.push({ name: worker.name, sessionId: worker.sessionId, connected: true });
  }
  return selected;
}

async function verifyBackupDownload(page) {
  const [download] = await Promise.all([
    page.waitForEvent("download"),
    page.getByRole("button", { name: "Download Hive backup" }).click(),
  ]);
  const stream = await download.createReadStream();
  let size = 0;
  let header = Buffer.alloc(0);
  for await (const chunk of stream) {
    const bytes = Buffer.from(chunk);
    size += bytes.length;
    if (header.length < 16) header = Buffer.concat([header, bytes]).subarray(0, 16);
  }
  await download.delete();
  if (header.toString("binary") !== "SQLite format 3\u0000" || size < 4096) {
    throw new Error(`desktop: Hive backup is not a valid SQLite artifact (${size} bytes)`);
  }
  return { sqliteHeader: true, bytes: size };
}

main().catch(async (error) => {
  if (resultPath) {
    await fs.mkdir(path.dirname(resultPath), { recursive: true });
    await fs.writeFile(resultPath, `${JSON.stringify({ baseUrl, status: "failed", error: error.message }, null, 2)}\n`, "utf8");
  }
  process.stderr.write(`${error.stack || error.message}\n`);
  process.exitCode = 1;
});
