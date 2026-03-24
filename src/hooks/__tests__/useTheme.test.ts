import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    setTheme: vi.fn().mockResolvedValue(undefined),
  }),
}));

describe("hooks/useTheme", () => {
  beforeEach(() => {
    localStorage.clear();
    document.documentElement.classList.remove("dark");
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  async function importFreshUseTheme() {
    vi.resetModules();
    return await import("../useTheme");
  }

  it("defaults to system theme", async () => {
    const { useTheme } = await importFreshUseTheme();
    const { result } = renderHook(() => useTheme());

    expect(result.current.theme).toBe("system");
    // matchMedia mock returns matches:false, so resolvedTheme = "light"
    expect(result.current.resolvedTheme).toBe("light");
  });

  it("setTheme(dark) updates theme and classList", async () => {
    const { useTheme } = await importFreshUseTheme();
    const { result } = renderHook(() => useTheme());

    act(() => {
      result.current.setTheme("dark");
    });

    expect(result.current.theme).toBe("dark");
    expect(result.current.resolvedTheme).toBe("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(localStorage.getItem("aio-theme")).toBe("dark");
  });

  it("setTheme(light) removes dark class", async () => {
    const { useTheme } = await importFreshUseTheme();
    const { result } = renderHook(() => useTheme());

    act(() => {
      result.current.setTheme("dark");
    });
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    act(() => {
      result.current.setTheme("light");
    });
    expect(result.current.theme).toBe("light");
    expect(result.current.resolvedTheme).toBe("light");
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("reads stored theme from localStorage", async () => {
    localStorage.setItem("aio-theme", "dark");
    const { useTheme } = await importFreshUseTheme();
    const { result } = renderHook(() => useTheme());

    expect(result.current.theme).toBe("dark");
    expect(result.current.resolvedTheme).toBe("dark");
  });

  it("setTheme(system) follows matchMedia", async () => {
    localStorage.setItem("aio-theme", "dark");
    const { useTheme } = await importFreshUseTheme();
    const { result } = renderHook(() => useTheme());

    act(() => {
      result.current.setTheme("system");
    });

    expect(result.current.theme).toBe("system");
    // matchMedia mock returns matches:false → light
    expect(result.current.resolvedTheme).toBe("light");
  });

  it("falls back safely when localStorage access throws during module init and updates", async () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });

    const { useTheme } = await importFreshUseTheme();
    const { result } = renderHook(() => useTheme());

    expect(result.current.theme).toBe("system");
    expect(result.current.resolvedTheme).toBe("light");

    act(() => {
      result.current.setTheme("dark");
    });

    expect(result.current.theme).toBe("dark");
    expect(result.current.resolvedTheme).toBe("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);
  });

  it("falls back safely when matchMedia is unavailable during module init", async () => {
    const original = window.matchMedia;
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: undefined,
    });

    const { useTheme } = await importFreshUseTheme();
    const { result } = renderHook(() => useTheme());

    expect(result.current.theme).toBe("system");
    expect(result.current.resolvedTheme).toBe("light");

    Object.defineProperty(window, "matchMedia", { writable: true, value: original });
  });

  it("still applies a stored dark theme when matchMedia is unavailable", async () => {
    const original = window.matchMedia;
    localStorage.setItem("aio-theme", "dark");
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: undefined,
    });

    const { useTheme } = await importFreshUseTheme();
    const { result } = renderHook(() => useTheme());

    expect(result.current.theme).toBe("dark");
    expect(result.current.resolvedTheme).toBe("dark");
    expect(document.documentElement.classList.contains("dark")).toBe(true);

    Object.defineProperty(window, "matchMedia", { writable: true, value: original });
  });

  it("uses addListener fallback when addEventListener is unavailable", async () => {
    const original = window.matchMedia;
    const addListener = vi.fn();

    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: vi.fn().mockReturnValue({
        matches: false,
        media: "(prefers-color-scheme: dark)",
        onchange: null,
        addListener,
        removeListener: vi.fn(),
        dispatchEvent: () => false,
      }),
    });

    await importFreshUseTheme();

    expect(addListener).toHaveBeenCalledTimes(1);

    Object.defineProperty(window, "matchMedia", { writable: true, value: original });
  });
});
