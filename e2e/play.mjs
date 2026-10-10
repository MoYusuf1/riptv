// End-to-end check in a real browser: sign in to the mock provider, play a live channel that
// needs conversion and a movie that needs conversion, seek it, and require moving picture and
// decoded sound each time. Runs against Google Chrome (Playwright's own Chromium has no H.264
// or AAC, so it can't judge playback).
//
//   cd e2e && npm ci && npm test        (after `dx build --web --release -p app`)
//   RIPTV_BIN=path/to/riptv npm test    (check a packaged app instead of `cargo run`)
import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { chromium } from "playwright-core";

const root = new URL("..", import.meta.url).pathname;
const children = [];
const free = () =>
  new Promise((ok) => {
    const s = createServer().listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => ok(port));
    });
  });
const start = (cmd, args, env) => {
  const child = spawn(cmd, args, { cwd: root, env: { ...process.env, ...env }, stdio: ["ignore", "inherit", "inherit"] });
  children.push(child);
  return child;
};
const up = async (url) => {
  for (let i = 0; i < 1200; i++) {
    try { if ((await fetch(url)).status < 500) return; } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`${url} never answered`);
};

// Picture moving and sound decoding: currentTime advances by `seconds`, frames and audio bytes decode.
async function plays(page, what, seconds = 3) {
  const handle = await page.waitForFunction(
    ({ seconds }) => {
      const v = document.querySelector("video");
      if (!v || v.error) return v?.error ? { error: v.error.message || String(v.error.code) } : false;
      v.dataset.from ??= String(v.currentTime);
      const moved = v.currentTime - Number(v.dataset.from);
      const frames = v.getVideoPlaybackQuality?.().totalVideoFrames ?? 0;
      const sound = v.webkitAudioDecodedByteCount ?? 1;
      return moved >= seconds && frames > 0 && sound > 0 ? { at: v.currentTime, frames, sound } : false;
    },
    { seconds },
    { timeout: 45_000, polling: 250 },
  );
  const result = await handle.jsonValue();
  if (result.error) throw new Error(`${what}: media error ${result.error}`);
  console.log(`PASS ${what}: at ${result.at.toFixed(1)} s, ${result.frames} frames, ${result.sound} audio bytes`);
}

// Milliseconds from `t0` until the video shows moving picture (from a new source, if `after` is
// the old one: a converted title restarts its stream to seek).
async function firstPicture(page, t0, after = null) {
  await page.waitForFunction(
    (after) => {
      const v = document.querySelector("video");
      return v && v.currentSrc && v.currentSrc !== after && v.currentTime > 0 && !v.paused && v.readyState >= 3;
    },
    after,
    { timeout: 45_000, polling: 50 },
  );
  return Date.now() - t0;
}

const failures = [];
async function step(what, run, page) {
  try {
    await run();
  } catch (e) {
    failures.push(what);
    console.log(`FAIL ${what}: ${e.message.split("\n")[0]}`);
    await page.screenshot({ path: `${root}target/e2e-${what.replace(/\W+/g, "-")}.png` }).catch(() => {});
  }
}

const [mock, app] = await Promise.all([free(), free()]);
start("cargo", ["run", "-q", "--release", "-p", "riptv", "--example", "mock_provider"], { MOCK_PORT: String(mock) });
if (process.env.RIPTV_BIN) start(process.env.RIPTV_BIN, ["--no-open"], { IPTV_PORT: String(app) });
else start("cargo", ["run", "-q", "--release", "-p", "riptv", "--", "--no-open"], { IPTV_PORT: String(app) });
const browser = await chromium.launch({ channel: "chrome", args: ["--autoplay-policy=no-user-gesture-required"] });
try {
  await Promise.all([up(`http://127.0.0.1:${mock}/player_api.php`), up(`http://127.0.0.1:${app}/`)]);
  const page = await browser.newPage();
  page.on("pageerror", (e) => console.log(`page error: ${e.message}`));
  const profile = { id: "e2e", name: "Mock", source: "xtream", url: `http://127.0.0.1:${mock}`, user: "demo", pass: "demo", color: 0 };
  await page.addInitScript((p) => localStorage.setItem("riptv.profiles", JSON.stringify([p])), profile);
  await page.goto(`http://127.0.0.1:${app}/`);
  await page.getByText("Mock", { exact: true }).click();

  await step("live channel, H.264 + AC-3", async () => {
    await page.getByText("H.264 video + AC-3 sound", { exact: true }).first().click();
    console.log(`TIME live channel, H.264 + AC-3: first picture ${await firstPicture(page, Date.now())} ms after choosing it`);
    await plays(page, "live channel, H.264 + AC-3");
  }, page);
  await step("live channel, HEVC + AC-3 (transcoded)", async () => {
    const before = await page.evaluate(() => document.querySelector("video").currentSrc);
    await page.getByText("HEVC video + AC-3 sound (needs conversion)", { exact: true }).first().click();
    console.log(`TIME live channel, HEVC + AC-3: first picture ${await firstPicture(page, Date.now(), before)} ms after choosing it`);
    await plays(page, "live channel, HEVC + AC-3 (transcoded)");
  }, page);
  await page.getByRole("button", { name: "Movies" }).click();
  for (const [what, title] of [
    ["movie, MKV with AC-3", "AC-3 sound in an MKV"],
    ...(process.env.MOCK_MOVIE ? [["movie, your own file", "Your own file (MOCK_MOVIE)"]] : []),
  ]) {
    await step(what, async () => {
      await page.getByText(title, { exact: true }).first().click();
      // Timed from the press: Playwright's click first waits for the page's fade-in to settle.
      await page.getByRole("button", { name: /^(Play|Resume)$/ }).first().click();
      const t0 = Date.now();
      console.log(`TIME ${what}: first picture ${await firstPicture(page, t0)} ms after Play`);
      await plays(page, what, 2);
    }, page);
    await step(`${what}, seek`, async () => {
      // What a drag on the seek bar ends with: the range input's value and a change event.
      const seek = page.locator("input.w-seek");
      const src = await page.evaluate(() => document.querySelector("video").currentSrc);
      const t0 = Date.now();
      const target = await seek.evaluate((el) => {
        el.value = String(Math.floor(Number(el.max) / 2));
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new Event("change", { bubbles: true }));
        return Number(el.value);
      });
      console.log(`TIME ${what}, seek: picture again ${await firstPicture(page, t0, src)} ms after seeking`);
      await page.evaluate(() => { delete document.querySelector("video").dataset.from; });
      await plays(page, `${what}, seek`, 2);
      // A converted title restarts the video's own clock at the seek; the bar shows the film's time.
      const at = Number(await seek.inputValue());
      if (at < target) throw new Error(`seek bar at ${at} s after seeking to ${target} s and playing`);
      console.log(`PASS ${what}, seek bar: sought to ${target} s, now at ${at.toFixed(1)} s`);
    }, page);
    // Out of the player, then off the title's page, back to the list.
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Back", exact: true }).first().click();
  }
} finally {
  await browser.close();
  for (const c of children) c.kill();
}
if (failures.length) {
  console.log(`${failures.length} failed (screenshots in target/e2e-*.png)`);
  process.exit(1);
}
console.log("All playback checks passed.");
