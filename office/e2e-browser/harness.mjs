// Shared bounded-I/O helpers for the browser oracles (soak, journey).
// Every harness await must be bounded: a frozen renderer must fail the
// run, never hang it.
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

/**
 * Bounded browser shutdown. Returns true when the graceful close timed
 * out and SIGKILL was used (caller must record a failure/nonzero verdict).
 * The timer is always cleared, so a clean close never lingers.
 */
export async function closeBrowser(browser, timeoutMs = 20000) {
  let timer;
  try {
    await Promise.race([
      browser.close(),
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("browser-close timeout")), timeoutMs);
      }),
    ]);
    return false;
  } catch {
    try {
      browser.process()?.kill("SIGKILL");
    } catch {
      // Already dead; nothing to kill.
    }
    return true;
  } finally {
    clearTimeout(timer);
  }
}
