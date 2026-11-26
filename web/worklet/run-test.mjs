// Opens dist/test.html in Chromium, Firefox and WebKit (Playwright), presses Start and waits
// for the self-test line. Serve dist/ first:  (cd dist && python3 -m http.server 8765)
//   node run-test.mjs [chromium firefox webkit]
import { chromium, firefox, webkit } from "playwright";
const all = { chromium, firefox, webkit };
const names = process.argv.slice(2).length ? process.argv.slice(2) : Object.keys(all);
let failed = 0;
for (const name of names) {
  const browser = await all[name].launch(name === "chromium" ? { args: ["--autoplay-policy=no-user-gesture-required"] } : {});
  const page = await browser.newPage();
  const logs = [];
  page.on("console", (m) => logs.push(m.text()));
  page.on("pageerror", (e) => logs.push("PAGEERROR " + e));
  await page.goto("http://localhost:8765/test.html");
  await page.click("#start");
  for (let i = 0; i < 40 && !logs.some((l) => l.includes("WORKLET_TEST")); i++) await page.waitForTimeout(500);
  const ok = logs.some((l) => l.includes("WORKLET_TEST PASS"));
  console.log(`== ${name}: ${ok ? "PASS" : "FAIL"}\n  ` + logs.join("\n  "));
  if (!ok) failed++;
  await browser.close();
}
process.exit(failed ? 1 : 0);
