// Real browser exercise against an already running disposable Compose candidate.
// The one-time confirmation must come from `./factory open`; never read a token file.
import assert from "node:assert/strict";
import { chromium } from "playwright-core";

const base = process.env.FACTORY_OFFICE_URL || "http://127.0.0.1:8893";
const enrollmentCode = process.env.FACTORY_ENROLLMENT_CODE;
const executablePath = process.env.CHROMIUM_EXE;
if (!enrollmentCode) throw new Error("FACTORY_ENROLLMENT_CODE is required");
if (!executablePath) throw new Error("CHROMIUM_EXE is required");

const browser = await chromium.launch({
  executablePath,
  headless: true,
  args: ["--no-sandbox", "--disable-dev-shm-usage"],
});
const desktopContext = await browser.newContext({
  viewport: { width: 1440, height: 1000 },
});
const desktop = await desktopContext.newPage();
const phoneContext = await browser.newContext({
  viewport: { width: 390, height: 844 },
});
const phone = await phoneContext.newPage();
const errors = [];
const notFound = [];
for (const page of [desktop, phone]) {
  page.on("pageerror", (error) => errors.push(String(error)));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  page.on("response", (response) => {
    if (response.status() === 404) notFound.push(response.url());
  });
}

async function session(page) {
  return page.evaluate(async () => {
    const response = await fetch("/core-api/auth/session", {
      credentials: "same-origin",
    });
    return response.json();
  });
}

try {
  await desktop.goto(base, { waitUntil: "domcontentloaded" });
  await desktop.getByRole("heading", { name: "Set up this Factory" }).waitFor();
  await desktop
    .getByRole("textbox", { name: "One-time owner confirmation" })
    .fill(enrollmentCode);
  await desktop
    .getByRole("textbox", { name: "This device name" })
    .fill("E2E desktop");
  await desktop.getByRole("button", { name: "Enroll this device" }).click();
  await desktop.getByRole("heading", { name: "Overview", level: 1 }).waitFor();

  const pwa = await desktop.evaluate(async () => {
    if (!("serviceWorker" in navigator))
      return { supported: false, caches: [] };
    const registration = await navigator.serviceWorker.ready;
    return {
      supported: true,
      scope: registration.scope,
      caches: await caches.keys(),
    };
  });
  assert.equal(
    pwa.supported,
    true,
    "Office registers its PWA worker on the secure test origin",
  );
  assert.deepEqual(
    pwa.caches,
    [],
    "Office does not cache API responses or page data",
  );

  const desktopSession = await session(desktop);
  assert.equal(desktopSession.authenticated, true);
  assert.equal(desktopSession.device.name, "E2E desktop");
  const desktopCookie = (await desktopContext.cookies()).find(
    (cookie) => cookie.name === "factory_session",
  );
  assert.ok(desktopCookie, "HttpOnly browser session cookie is present");
  assert.equal(desktopCookie.httpOnly, true);
  assert.equal(await desktop.evaluate(() => sessionStorage.length), 0);
  assert.equal(await desktop.evaluate(() => localStorage.length), 0);

  await desktop.reload();
  await desktop.getByRole("heading", { name: "Overview", level: 1 }).waitFor();
  await desktop.getByRole("link", { name: "Settings", exact: true }).click();
  await desktop.getByRole("tab", { name: "Access & Devices" }).click();
  await desktop.getByRole("heading", { name: "Enrolled devices" }).waitFor();

  for (const width of [1440, 834, 390]) {
    await desktop.setViewportSize({ width, height: 1000 });
    await desktop.waitForTimeout(100);
    const overflow = await desktop.evaluate(
      () => document.documentElement.scrollWidth > window.innerWidth,
    );
    assert.equal(overflow, false, `Access & Devices overflows at ${width}px`);
  }
  await desktop.setViewportSize({ width: 1440, height: 1000 });

  const challengeResponse = desktop.waitForResponse(
    (response) =>
      response.request().method() === "POST" &&
      new URL(response.url()).pathname.endsWith("/auth/device-challenges"),
  );
  await desktop.getByRole("button", { name: "Pair device" }).click();
  const challenge = await (await challengeResponse).json();
  assert.ok(challenge.challengeId);
  assert.equal(await desktop.evaluate(() => sessionStorage.length), 0);
  assert.equal(await desktop.evaluate(() => localStorage.length), 0);

  const pairUrl = new URL(base);
  pairUrl.hash = `/pair?challengeId=${encodeURIComponent(challenge.challengeId)}`;
  await phone.goto(pairUrl.toString(), { waitUntil: "domcontentloaded" });
  await phone.getByRole("heading", { name: "Pair this device" }).waitFor();
  await phone.getByRole("textbox", { name: "Device name" }).fill("E2E phone");
  const claimResponse = phone.waitForResponse(
    (response) =>
      response.request().method() === "POST" &&
      new URL(response.url()).pathname.includes("/auth/device-claims/"),
  );
  await phone.getByRole("button", { name: "Request owner approval" }).click();
  const claim = await (await claimResponse).json();
  assert.equal(claim.status, "PENDING");
  assert.equal(claim.verificationCode, challenge.verificationCode);
  assert.equal(new URL(phone.url()).hash, "#/pair");
  assert.equal(phone.url().includes(challenge.challengeId), false);
  await phone.getByText(claim.verificationCode, { exact: true }).waitFor();

  const pending = desktop.getByText("E2E phone", { exact: true });
  await pending.waitFor();
  const confirmCode = desktop.getByRole("textbox", {
    name: "Confirm the verification code shown on the phone",
  });
  await confirmCode.fill(claim.verificationCode);
  await desktop.getByRole("button", { name: "Approve device" }).click();
  await phone
    .getByRole("heading", { name: "Overview", level: 1 })
    .waitFor({ timeout: 15000 });

  const phoneSession = await session(phone);
  assert.equal(phoneSession.authenticated, true);
  assert.equal(phoneSession.device.name, "E2E phone");
  assert.notEqual(phoneSession.device.id, desktopSession.device.id);
  const phoneCookie = (await phoneContext.cookies()).find(
    (cookie) => cookie.name === "factory_session",
  );
  assert.ok(phoneCookie);
  assert.notEqual(phoneCookie.value, desktopCookie.value);
  assert.equal(phoneCookie.httpOnly, true);

  const replay = await browser.newPage();
  await replay.goto(pairUrl.toString(), { waitUntil: "domcontentloaded" });
  await replay.getByRole("button", { name: "Request owner approval" }).click();
  await replay.getByRole("alert").waitFor();
  await replay.close();

  const rejectedChallengeResponse = desktop.waitForResponse(
    (response) =>
      response.request().method() === "POST" &&
      new URL(response.url()).pathname.endsWith("/auth/device-challenges"),
  );
  await desktop.getByRole("button", { name: "Pair device" }).click();
  const rejectedChallenge = await (await rejectedChallengeResponse).json();
  const rejectedPairUrl = new URL(base);
  rejectedPairUrl.hash = `/pair?challengeId=${encodeURIComponent(rejectedChallenge.challengeId)}`;
  await phone.goto(rejectedPairUrl.toString(), {
    waitUntil: "domcontentloaded",
  });
  await phone
    .getByRole("textbox", { name: "Device name" })
    .fill("Rejected phone");
  const rejectedClaimResponse = phone.waitForResponse(
    (response) =>
      response.request().method() === "POST" &&
      new URL(response.url()).pathname.includes("/auth/device-claims/") &&
      !new URL(response.url()).pathname.endsWith("/poll"),
  );
  await phone.getByRole("button", { name: "Request owner approval" }).click();
  const rejectedClaim = await (await rejectedClaimResponse).json();
  assert.equal(rejectedClaim.status, "PENDING");
  assert.equal(
    rejectedClaim.verificationCode,
    rejectedChallenge.verificationCode,
  );
  await desktop.getByText("Rejected phone", { exact: true }).waitFor();
  await desktop
    .locator(".access-device")
    .filter({ hasText: "Rejected phone" })
    .getByRole("button", { name: "Reject" })
    .click();
  await phone.getByText(/owner rejected this device request/i).waitFor();

  await phone.goto(`${base}/#/settings?tab=access`);
  await phone.getByRole("tab", { name: "Access & Devices" }).waitFor();
  await phone.setViewportSize({ width: 390, height: 844 });
  const phoneOverflow = await phone.evaluate(
    () => document.documentElement.scrollWidth > window.innerWidth,
  );
  assert.equal(phoneOverflow, false);

  await desktop.getByText("E2E phone", { exact: true }).waitFor();
  const phoneDevice = desktop
    .locator(".access-device")
    .filter({ hasText: "E2E phone" });
  const revokeDialog = desktop
    .waitForEvent("dialog")
    .then((dialog) => dialog.accept());
  await phoneDevice.getByRole("button", { name: "Revoke" }).click();
  await revokeDialog;
  await phone.reload();
  await phone.getByRole("heading", { name: "Recover local access" }).waitFor();

  const unauthenticated = await phone.evaluate(async () => {
    const response = await fetch("/core-api/projects", {
      headers: { "x-tailscale-user": "forged-owner@example.invalid" },
    });
    return response.status;
  });
  assert.equal(
    unauthenticated,
    401,
    "forged proxy identity does not authorize Core access",
  );

  const allDialog = desktop
    .waitForEvent("dialog")
    .then((dialog) => dialog.accept());
  await desktop.getByRole("button", { name: "Revoke all devices" }).click();
  await allDialog;
  await desktop
    .getByRole("heading", { name: "Recover local access" })
    .waitFor();

  assert.deepEqual(errors, [], `browser errors: ${errors.join(" | ")}`);
  assert.deepEqual(notFound, [], `404 responses: ${notFound.join(" | ")}`);
  console.log(
    JSON.stringify({
      status: "PASS",
      desktopSession: "persisted across reload",
      phoneSession: "independent and explicitly approved",
      revocation: "phone session denied after owner revocation",
      viewportWidths: [1440, 834, 390],
      remoteMobileNetwork: "NOT_TESTED",
      tailscaleConnectivity:
        "reported by Core only; not inferred by this browser run",
    }),
  );
} finally {
  await browser.close();
}
