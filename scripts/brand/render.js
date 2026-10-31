/*
 * Open Memory Lab brand kit: PNG renderer.
 *
 * Usage:  python3 gen.py OUT_DIR && node render.js OUT_DIR
 * Needs:  Node 18+ and Playwright with Chromium (npm i playwright && npx playwright install chromium).
 * Writes: OUT_DIR/social-preview.png (1280x640) and OUT_DIR/oml-icon-512.png (512x512, transparent corners).
 */
const { chromium } = require('playwright');
const fs = require('fs'); const path = require('path');
const out = process.argv[2];
if (!out) { console.error('usage: node render.js OUT_DIR'); process.exit(1); }
(async () => {
  const browser = await chromium.launch();
  async function shot(svgFile, pngFile, w, h) {
    const page = await browser.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 1 });
    const svg = fs.readFileSync(path.join(out, svgFile), 'utf8')
      .replace('<svg ', `<svg style="display:block;width:${w}px;height:${h}px" `);
    await page.setContent(`<!doctype html><html><body style="margin:0;background:transparent">${svg}</body></html>`);
    await page.screenshot({ path: path.join(out, pngFile), omitBackground: true });
    await page.close();
  }
  await shot('social-preview.svg', 'social-preview.png', 1280, 640);
  await shot('oml-icon.svg', 'oml-icon-512.png', 512, 512);
  await browser.close();
  console.log('ok');
})();
