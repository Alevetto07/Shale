import { chromium } from "playwright";
import { PNG } from "pngjs";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/**
 * How much of each 3D canvas the part itself covers.
 *
 * `canvas-coverage.mjs` measures the bounding box of every lit pixel, which on
 * a viewer with a build plate is the plate, not the model. This measures only
 * pixels bright enough to be an extrusion (the palette is #f3f0e8 / #cbc4b8 /
 * #a8d4ff / #9eb8ff; the plate and its grid are #0c1018 / #2e4a6a / #1a2430,
 * an order of magnitude darker), so the number is the model's share of the
 * canvas on each axis and the larger of the two is what "fills 60-70%" means.
 */
const url = process.env.SITE_URL ?? "http://127.0.0.1:4321";
const shots = process.env.SHOTS_DIR;
if (shots) mkdirSync(shots, { recursive: true });

function modelBox(pngBuffer) {
  const png = PNG.sync.read(pngBuffer);
  const { width: w, height: h, data } = png;
  let minX = w;
  let minY = h;
  let maxX = -1;
  let maxY = -1;
  let lit = 0;

  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (w * y + x) << 2;
      const sum = data[i] + data[i + 1] + data[i + 2];
      if (sum < 400) continue;
      lit++;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
  }

  if (!lit) return null;
  const bw = maxX - minX + 1;
  const bh = maxY - minY + 1;
  return { w, h, bw, bh, fillW: bw / w, fillH: bh / h, litShare: lit / (w * h) };
}

const browser = await chromium.launch();

for (const { tag, width, height } of [
  { tag: "desktop", width: 1440, height: 900 },
  { tag: "mobile", width: 390, height: 844 },
]) {
  const page = await browser.newPage({ viewport: { width, height } });
  await page.goto(url, { waitUntil: "networkidle" });
  await page.waitForSelector("#app-demo", { timeout: 45000 });
  await page.evaluate(() => {
    document.querySelectorAll("[data-reveal]").forEach((el) => el.classList.add("is-visible"));
  });
  await page.locator("#app-demo").scrollIntoViewIfNeeded();
  await page.waitForSelector("#app-demo .gcode-viewer canvas", { timeout: 60000 });
  // The legend and caption are DOM on top of the canvas: an element screenshot
  // includes them, and their text is as bright as an extrusion. Hide them so
  // the measurement is the part and nothing else.
  await page.addStyleTag({
    content: ".gcode-legend, .gcode-viewer__caption, .gcode-viewer__sleep { display: none !important; }",
  });
  await page.waitForTimeout(tag === "mobile" ? 4000 : 3000);

  const appCanvas = page.locator("#app-demo .gcode-viewer canvas");
  const appBox = await appCanvas.boundingBox();
  if (appBox) {
    const appShot = await appCanvas.screenshot({ type: "png" });
    if (shots) writeFileSync(join(shots, `canvas-app-${tag}.png`), appShot);
    const m = modelBox(appShot);
    if (m) {
      console.log(
        `[${tag}] app-preview canvas=${m.w}x${m.h} model=${m.bw}x${m.bh} ` +
          `fillW=${m.fillW.toFixed(3)} fillH=${m.fillH.toFixed(3)} max=${Math.max(m.fillW, m.fillH).toFixed(3)}`,
      );
    } else {
      console.log(`[${tag}] app-preview: no model pixels found`);
    }
  }

  await page.locator("#demos [role='tab']", { hasText: "Workbench" }).click();
  await page.locator("#toolpath").scrollIntoViewIfNeeded({ block: "center" });
  await page.waitForTimeout(tag === "mobile" ? 7000 : 4500);

  const toolCanvas = page.locator("#toolpath .demo-panel__viewer canvas");
  const toolBox = await toolCanvas.boundingBox();
  if (toolBox) {
    const toolShot = await toolCanvas.screenshot({ type: "png" });
    if (shots) writeFileSync(join(shots, `canvas-toolpath-${tag}.png`), toolShot);
    const m = modelBox(toolShot);
    if (m) {
      console.log(
        `[${tag}] toolpath    canvas=${m.w}x${m.h} model=${m.bw}x${m.bh} ` +
          `fillW=${m.fillW.toFixed(3)} fillH=${m.fillH.toFixed(3)} max=${Math.max(m.fillW, m.fillH).toFixed(3)}`,
      );
    } else {
      console.log(`[${tag}] toolpath: no model pixels found`);
    }
  }

  await page.close();
}

await browser.close();
