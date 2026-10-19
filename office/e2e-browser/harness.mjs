// Bounded async helpers and browser lifecycle for the e2e-browser oracles.
// Every harness await must be bounded: a frozen renderer or a hung launch
// must fail the run, never hang it.
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chromium } from "playwright-core";

export async function withTimeout(promise, ms, message) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), ms);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

// Bounded shutdown. Returns true when the graceful close failed or timed
// out, so callers can make that failure part of the oracle verdict.
export async function closeBrowser(browser, timeoutMs = 20000) {
  try {
    await withTimeout(browser.close(), timeoutMs, "browser-close timeout");
    return false;
  } catch {
    try {
      browser.process()?.kill("SIGKILL");
    } catch {
      // Already disconnected or dead.
    }
    return true;
  }
}

async function availablePort() {
  const server = createServer();
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const { port } = server.address();
  await new Promise((resolve, reject) =>
    server.close((err) => (err ? reject(err) : resolve())),
  );
  return port;
}

function waitForEndpoint(child, port) {
  return new Promise((resolve, reject) => {
    let poll;
    let checking = false;
    const cleanup = () => {
      clearInterval(poll);
      child.off("exit", onExit);
      child.off("error", onError);
    };
    const done =
      (fn) =>
      (...a) => {
        cleanup();
        fn(...a);
      };
    const onExit = (code, signal) =>
      done(reject)(
        new Error(
          `browser exited during launch (code ${code}, signal ${signal})`,
        ),
      );
    const onError = (err) => done(reject)(err);
    const checkEndpoint = async () => {
      if (checking) return;
      checking = true;
      try {
        const response = await fetch(`http://127.0.0.1:${port}/json/version`, {
          signal: AbortSignal.timeout(250),
        });
        if (response.ok) {
          const version = await response.json();
          if (typeof version.webSocketDebuggerUrl === "string") {
            done(resolve)(version.webSocketDebuggerUrl);
          }
        }
      } catch {
        // Chromium may need a few hundred milliseconds to bind the port.
      } finally {
        checking = false;
      }
    };
    child.on("exit", onExit);
    child.on("error", onError);
    poll = setInterval(() => void checkEndpoint(), 100);
    void checkEndpoint();
  });
}

/**
 * Launch Chromium with the harness owning the child process from spawn,
 * then attach Playwright over CDP. Ownership is the point: on ANY failure
 * the exact spawned process group is SIGKILLed, so a hung launch binary
 * can never orphan a browser (Playwright's own launch timeout was
 * observed to reject while leaving the hung spawn alive).
 *
 * Returns { browser, page, close }. close() is bounded, always SIGKILLs
 * the owned process tree and removes the profile, and reports whether
 * the graceful close timed out.
 */
export async function launchBrowser(
  executablePath,
  { timeoutMs = 60000, headless = true, forceX11 = false } = {},
) {
  const userDataDir = mkdtempSync(join(tmpdir(), "factory-chromium-"));
  const headedArgs = headless
    ? []
    : forceX11
      ? ["--ozone-platform=x11", "--disable-gpu"]
      : process.env.WAYLAND_DISPLAY
        ? ["--ozone-platform=wayland"]
        : ["--ozone-platform=x11", "--disable-gpu"];
  let port;
  try {
    port = await availablePort();
  } catch (e) {
    rmSync(userDataDir, { recursive: true, force: true });
    throw e;
  }
  const args = [
    ...(headless ? ["--headless"] : []),
    "--no-sandbox",
    "--disable-dev-shm-usage",
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-background-networking",
    ...headedArgs,
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${userDataDir}`,
    "about:blank",
  ];
  const browserEnv = { ...process.env };
  if (forceX11) delete browserEnv.WAYLAND_DISPLAY;
  // The browser child never needs Factory credentials: the Office token
  // gate is filled over the automation protocol, not from the environment.
  // Scrub token-shaped variables so a compromised renderer cannot read them
  // from its own process environment.
  for (const key of Object.keys(browserEnv)) {
    if (/TOKEN/i.test(key)) delete browserEnv[key];
  }
  const child = spawn(executablePath, args, {
    detached: true,
    stdio: "ignore",
    env: browserEnv,
  });
  const killTree = () => {
    // Negative pid targets the detached process group, so wedged
    // grandchildren die with the browser instead of orphaning.
    if (child.pid === undefined) return;
    try {
      process.kill(-child.pid, "SIGKILL");
    } catch {
      try {
        child.kill("SIGKILL");
      } catch {
        // Already dead; nothing to kill.
      }
    }
  };
  const cleanupDir = () => {
    try {
      rmSync(userDataDir, { recursive: true, force: true });
    } catch {
      // Best effort; tmpdir reclamation is not a verdict.
    }
  };
  const fail = async (e) => {
    killTree();
    cleanupDir();
    throw e;
  };

  let browser;
  try {
    const wsUrl = await withTimeout(
      waitForEndpoint(child, port),
      timeoutMs,
      "devtools endpoint timeout",
    );
    const httpUrl = new URL(wsUrl);
    httpUrl.protocol = "http:";
    httpUrl.pathname = "/";
    browser = await withTimeout(
      chromium.connectOverCDP(httpUrl.href, { timeout: timeoutMs }),
      timeoutMs,
      "cdp connect timeout",
    );
  } catch (e) {
    await fail(e);
  }
  let context;
  try {
    const contexts = browser.contexts();
    context =
      contexts.length > 0
        ? contexts[0]
        : await withTimeout(
            browser.newContext(),
            15000,
            "browser context timeout",
          );
  } catch (e) {
    await fail(e);
  }
  context.setDefaultTimeout(15000);
  context.setDefaultNavigationTimeout(30000);
  let page;
  try {
    page = await withTimeout(context.newPage(), 15000, "new page timeout");
  } catch (e) {
    await fail(e);
  }

  const close = async (closeTimeoutMs = 20000) => {
    const timedOut = await closeBrowser(browser, closeTimeoutMs);
    killTree();
    cleanupDir();
    return timedOut;
  };
  return {
    browser,
    page,
    close,
    profilePath: userDataDir,
    processGroupId: child.pid,
  };
}
