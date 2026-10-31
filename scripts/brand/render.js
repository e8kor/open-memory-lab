const { chromium } = require('playwright');
const fs = require('fs'); const path = require('path');
(async () => {
  const dir = process.argv[2]; const out = process.argv[3];
  const jobs = JSON.parse(process.argv[4]);
  const b = await chromium.launch(); const p = await b.newPage();
  for (const [src, dst, w, h, scale, bg] of jobs) {
    await p.setViewportSize({ width: w, height: h });
    const svg = fs.readFileSync(path.join(dir, src), 'utf8');
    await p.setContent(`<html><body style="margin:0;background:${bg||'transparent'}"><img src="data:image/svg+xml;base64,${Buffer.from(svg).toString('base64')}" style="width:${w}px;height:${h}px;display:block"></body></html>`);
    await p.screenshot({ path: path.join(out, dst), omitBackground: !bg, clip: {x:0,y:0,width:w,height:h} });
    console.log(dst);
  }
  await b.close();
})();
