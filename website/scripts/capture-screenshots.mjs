import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";
import { coverageFromPng } from "./canvas-coverage.mjs";

const outDir =
  process.env.SCREENSHOT_OUT ?? join(process.cwd(), "docs/screenshots-redesign");
const baseUrl = process.env.SITE_URL ?? "http://127.0.0.1:4321";
const MIN_CANVAS_COVERAGE = Number(process.env.MIN_CANVAS_COVERAGE ?? "0.15");

const viewports = [
  { tag: "desktop", width: 1440, height: 900 },
  { tag: "mobile", width: 390, height: 844 },
];

const sections = [
  {
    id: "hero",
    selector: ".hero-narrow",
    fullPage: false,
    wait: 5000,
    settle: "#app-demo .gcode-viewer canvas, #app-demo .ascii-frame",
  },
  { id: "app-demo", selector: "#app-demo", fullPage: false, wait: 4000, settle: "#app-demo" },
  { id: "activity", selector: "#activity", fullPage: false, wait: 2000, settle: "#activity" },
  { id: "coming-next", selector: "#coming-next", fullPage: false, wait: 4000, settle: "#coming-next canvas" },
  { id: "demos", selector: "#demos", fullPage: false, wait: 4500, settle: ".printer-row--rich" },
  {
    id: "toolpath",
    selector: "#toolpath",
    fullPage: false,
    wait: 3500,
    settle: ".demo-panel__viewer canvas",
    canvasAlt: ".demo-panel__viewer canvas",
  },
  { id: "printers", selector: "#printers", fullPage: false, wait: 2000, settle: ".printer-row--rich.is-active" },
  { id: "cli", selector: "#cli", fullPage: false, wait: 3500, settle: ".term-out" },
  { id: "downloads", selector: "#downloads", fullPage: false, wait: 600 },
  { id: "footer", selector: ".site-footer", fullPage: false, wait: 300 },
];

async function canvasInkCoverage(page, selector) {
  const loc = page.locator(selector).first();
  if ((await loc.count()) === 0) return { coverage: 0, litPixels: 0 };
  const box = await loc.boundingBox();
  if (!box) return { coverage: 0, litPixels: 0 };
  const buf = await page.screenshot({ clip: box, type: "png", animations: "disabled" });
  return coverageFromPng(buf);
}

await mkdir(outDir, { recursive: true });

const browser = await chromium.launch();
const regressions = [];

for (const vp of viewports) {
  const page = await browser.newPage({
    viewport: { width: vp.width, height: vp.height },
  });
  await page.goto(baseUrl, { waitUntil: "networkidle" });
  await page.evaluate(() => {
    document.querySelectorAll("[data-reveal], .slogan-display").forEach((el) => {
      el.classList.add("is-visible");
    });
  });
  await page.waitForSelector("#app-demo", { timeout: 35000 });
  await page.locator(".workflow-pause").click().catch(() => {});
  await page.locator(".workflow-timeline button", { hasText: /^Preview layers$/ }).click();
  await page.locator(".workflow-pause").click().catch(() => {});
  await page.waitForSelector("#app-demo .gcode-viewer canvas", { timeout: 45000 }).catch(() => {});

  await page.locator("#app-demo").scrollIntoViewIfNeeded();
  await page.waitForTimeout(4500);
  const heroCov = await canvasInkCoverage(page, "#app-demo .gcode-viewer canvas");
  console.log(`[${vp.tag}] hero canvas coverage=${heroCov.coverage?.toFixed(3)} lit=${heroCov.litPixels}`);
  if ((heroCov.coverage ?? 0) < MIN_CANVAS_COVERAGE) {
    console.warn(
      `[${vp.tag}] hero canvas low coverage=${heroCov.coverage?.toFixed(3)} (preview may still be loading)`,
    );
  }

  if (vp.tag === "mobile") {
    await page.setViewportSize({ width: vp.width, height: vp.height });
  }

  for (const section of sections) {
    const el = await page.$(section.selector);
    if (!el) {
      console.warn(`Missing ${section.selector}`);
      continue;
    }
    const headerPad = 88;
    await page.evaluate(
      ({ sel, pad }) => {
        const node = document.querySelector(sel);
        if (!node) return;
        const y = node.getBoundingClientRect().top + window.scrollY - pad;
        window.scrollTo({ top: Math.max(0, y), behavior: "instant" });
      },
      { sel: section.selector, pad: headerPad },
    );
    if (section.settle) {
      await page.waitForSelector(section.settle, { timeout: 15000 }).catch(() => {});
    }
    if (section.id === "app-demo") {
      await page.locator(".workflow-timeline button", { hasText: /^Preview layers$/ }).click();
      await page.locator(".workflow-pause").click().catch(() => {});
      await page.waitForSelector("#app-demo .gcode-viewer canvas", { timeout: 45000 }).catch(() => {});
      await page.waitForTimeout(vp.tag === "mobile" ? 4500 : 2800);
    }
    if (section.id === "toolpath") {
      await page.locator("#demos [role='tab']", { hasText: "Workbench" }).click().catch(() => {});
      await page
        .waitForFunction(
          () => document.querySelector("#toolpath .demo-panel__note")?.textContent?.includes("CuraEngine"),
          { timeout: 45000 },
        )
        .catch(() => {});
      await page.waitForSelector("#toolpath .demo-panel__viewer canvas", { timeout: 45000 }).catch(() => {});
      await page
        .waitForFunction(
          () => {
            const c = document.querySelector("#toolpath .demo-panel__viewer canvas");
            if (!c) return false;
            const gl = c.getContext("webgl") || c.getContext("webgl2");
            return Boolean(gl);
          },
          { timeout: 20000 },
        )
        .catch(() => {});
      await page.evaluate(() => {
        const n = document.querySelector("#toolpath");
        n?.scrollIntoView({ block: "center", behavior: "instant" });
      });
      await page.waitForTimeout(vp.tag === "mobile" ? 8500 : 3200);
      await page.evaluate(() => {
        window.dispatchEvent(new Event("resize"));
        window.dispatchEvent(new Event("scroll"));
      });
    } else {
      await page.waitForTimeout(section.wait);
    }

    if (section.id === "toolpath") {
      const toolCov = await canvasInkCoverage(page, ".demo-panel__viewer canvas");
      console.log(`[${vp.tag}] toolpath canvas coverage=${toolCov.coverage?.toFixed(3)} lit=${toolCov.litPixels}`);
      const toolMin = vp.tag === "mobile" ? 0.08 : 0.1;
      if ((toolCov.coverage ?? 0) < toolMin) {
        regressions.push(`${vp.tag} toolpath coverage ${toolCov.coverage} < ${toolMin}`);
      }
    }

    const path = join(outDir, `${section.id}-${vp.tag}.png`);
    try {
      await el.screenshot({ path, animations: "disabled", timeout: 45_000 });
    } catch {
      const box = await el.boundingBox();
      if (box && box.width > 2 && box.height > 2) {
        try {
          await page.screenshot({ path, clip: box, animations: "disabled" });
        } catch (err) {
          console.warn(`Clip screenshot failed for ${section.id}:`, err.message);
        }
      }
    }
    console.log(`Wrote ${path}`);
  }

  const fullPath = join(outDir, `full-page-${vp.tag}.png`);
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(500);
  await page.screenshot({ path: fullPath, fullPage: true });
  console.log(`Wrote ${fullPath}`);

  await page.close();
}
await browser.close();

if (regressions.length) {
  console.error("Visual regression failures:");
  for (const r of regressions) console.error(" -", r);
  process.exit(1);
}

console.log(`Screenshots in ${outDir}`);