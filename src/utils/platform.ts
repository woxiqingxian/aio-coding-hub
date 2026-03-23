/**
 * Returns `true` when running on a Windows host.
 * Works in both browser and Tauri WebView environments.
 */
export function isWindowsRuntime() {
  return typeof navigator !== "undefined" && /Win/.test(navigator.userAgent);
}
