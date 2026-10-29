//! One-time cleanup of credentials written by pre-device-session Office builds.

/**
 * `sessionStorage` key that older Office builds used for the permanent Core
 * operator bearer token. It must never be read by the current application.
 */
export const LEGACY_OPERATOR_TOKEN_KEY = "factory-api-token";

/**
 * Remove the retired operator credential during frontend initialization.
 *
 * The value is removed without being read, transmitted or logged. Storage
 * access is best-effort because some browsers reject `sessionStorage` in
 * private or partitioned contexts; in that case there is no accessible value
 * to purge.
 */
export function purgeLegacyOperatorToken(): void {
  try {
    globalThis.sessionStorage?.removeItem(LEGACY_OPERATOR_TOKEN_KEY);
  } catch {
    // Storage is unavailable; no reachable legacy credential remains.
  }
}

/** Initialize browser-side Office security state before the first request. */
export function initializeOffice(): void {
  purgeLegacyOperatorToken();
}
