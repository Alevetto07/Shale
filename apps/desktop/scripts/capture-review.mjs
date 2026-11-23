import { chromium } from "playwright";
import { execSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { mkdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const base = process.env.SHALE_CAPTURE_URL ?? "http://127.0.0.1:5173";
const beforeBase = process.env.SHALE_BEFORE_URL ?? "";
const here = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(here, "../review-screens");
mkdirSync(outDir, { recursive: true });

const shots = [
  { path: "/?mock=home&pane=home&plateStill=1", name: "start-andromeda-after" },
  { path: "/?mock=home&pane=home&uiStyle=lunar&plateStill=1", name: "start-lunar-after" },
  { path: "/?mock=workspace-prepare&pane=prepare&plateStill=1", name: "workspace-andromeda-prepare" },
  { path: "/?mock=workspace-preview&pane=preview", name: "workspace-andromeda-preview" },
  { path: "/?mock=home&pane=home&showcase=ui", name: "components-showcase-andromeda" },
];

async function captureStill(page, shot) {
  await page.goto(`${base}${shot.path}`, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(1500);
  await page.waitForSelector(".plate-field, .ui-showcase", { timeout: 15_000 }).catch(() => undefined);
  await page.screenshot({ path: `${outDir}/${shot.name}.png`, fullPage: false });
}

async function captureBefore(browser) {
  const beforePath = `${outDir}/start-andromeda-before.png`;
  if (existsSync(beforePath) && !process.env.SHALE_FORCE_BEFORE) {
    return;
  }
  if (beforeBase) {
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    await page.goto(`${beforeBase}/?mock=home&pane=home&plateStill=1`, { waitUntil: "networkidle", timeout: 60_000 });
    await page.waitForTimeout(1500);
    await page.screenshot({ path: `${outDir}/start-andromeda-before.png`, fullPage: false });
    await page.close();
    return;
  }
  const worktree = "/tmp/shale-main-review-ui";
  const desktop = path.join(worktree, "apps/desktop");
  if (!existsSync(desktop)) {
    rmSync(worktree, { recursive: true, force: true });
    execSync(`git worktree add ${worktree} origin/main`, { cwd: path.resolve(here, "../../.."), stdio: "inherit" });
    execSync("npm ci", { cwd: desktop, stdio: "inherit" });
    execSync("VITE_SHALE_MOCK=1 npm run build", { cwd: desktop, stdio: "inherit" });
  }
  execSync(`npx vite preview --host 127.0.0.1 --port 5174`, {
    cwd: desktop,
    stdio: "ignore",
    detached: true,
  });
  await new Promise((resolve) => setTimeout(resolve, 2500));
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto("http://127.0.0.1:5174/?mock=home&pane=home&plateStill=1", {
    waitUntil: "networkidle",
    timeout: 90_000,
  });
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${outDir}/start-andromeda-before.png`, fullPage: false });
  await page.close();
}

async function recordMotion(browser) {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    recordVideo: { dir: outDir, size: { width: 1440, height: 900 } },
  });
  const page = await context.newPage();
  await page.goto(`${base}/?mock=home&pane=home&plateStill=1`, { waitUntil: "networkidle", timeout: 60_000 });
  await page.waitForTimeout(1200);

  await page.getByRole("button", { name: /Collapse sidebar/i }).first().click({ timeout: 5000 });
  await page.waitForTimeout(700);
  await page.getByRole("button", { name: /Expand sidebar/i }).first().click({ timeout: 5000 });
  await page.waitForTimeout(700);

  const row = page.locator(".ui-sidebar__row").first();
  await row.hover();
  await page.waitForTimeout(500);

  await page.locator(".ui-sidebar__header-actions button").first().click();
  await page.waitForTimeout(800);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);

  await page.locator(".ui-sidebar__footer .ui-btn", { hasText: "Settings" }).click({ timeout: 5000 });
  await page.waitForTimeout(1200);
  await page.getByRole("button", { name: /^Back$/i }).click({ timeout: 5000 }).catch(() => undefined);
  await page.waitForTimeout(600);

  await page.locator(".menubar .menu-title", { hasText: /^File$/ }).click({ timeout: 5000 });
  await page.waitForTimeout(900);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(400);

  await page.goto(`${base}/?mock=home&pane=home&showcase=ui`, { waitUntil: "networkidle" });
  await page.waitForTimeout(800);
  const primary = page.getByRole("button", { name: "Primary" });
  await primary.click();
  await page.waitForTimeout(400);
  await page.getByRole("button", { name: "Show toast" }).click();
  await page.waitForTimeout(1000);

  const slider = page.getByRole("slider", { name: "Layer height" });
  const box = await slider.boundingBox();
  if (box) {
    await page.mouse.move(box.x + box.width * 0.35, box.y + box.height / 2);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width * 0.7, box.y + box.height / 2, { steps: 12 });
    await page.mouse.up();
  }
  await page.waitForTimeout(600);
  await page.getByRole("button", { name: "Layer height exact value" }).click();
  await page.waitForTimeout(400);
  await page.keyboard.type("0.28");
  await page.waitForTimeout(1200);
  await page.getByRole("button", { name: "Open menu" }).click().catch(() => undefined);
  await page.waitForTimeout(1200);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(1500);
  await page.goto(`${base}/?mock=home&pane=home&plateStill=1`, { waitUntil: "networkidle" });
  await page.waitForTimeout(2000);

  const video = page.video();
  await context.close();
  const target = `${outDir}/ui-rework-motion-15s.webm`;
  if (video) await video.saveAs(target);
}

await mkdir(outDir, { recursive: true });
const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
for (const shot of shots) {
  await captureStill(page, shot);
}
await captureBefore(browser);
await recordMotion(browser);
await browser.close();
console.log(`Review assets written to ${outDir}`);