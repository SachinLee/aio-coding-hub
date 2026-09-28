import { afterEach, describe, expect, it, vi } from "vitest";
import {
  SIDEBAR_NAV_GROUPS_STORAGE_KEY,
  readSidebarNavGroupStateFromStorage,
  writeSidebarNavGroupStateToStorage,
} from "../sidebarToolsGroup";

describe("ui/sidebarToolsGroup", () => {
  afterEach(() => {
    window.localStorage.clear();
    vi.restoreAllMocks();
  });

  it("defaults to all-collapsed when nothing is stored", () => {
    expect(readSidebarNavGroupStateFromStorage()).toEqual({});
  });

  it("round-trips group state", () => {
    writeSidebarNavGroupStateToStorage({ tools: true });

    expect(readSidebarNavGroupStateFromStorage()).toEqual({ tools: true });
    expect(window.localStorage.getItem(SIDEBAR_NAV_GROUPS_STORAGE_KEY)).toBe('{"tools":true}');
  });

  it("falls back to collapsed when storage reads throw", () => {
    const getSpy = vi.spyOn(window.localStorage, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });

    expect(readSidebarNavGroupStateFromStorage()).toEqual({});
    getSpy.mockRestore();
  });

  it("swallows storage write failures", () => {
    const setSpy = vi.spyOn(window.localStorage, "setItem").mockImplementation(() => {
      throw new Error("blocked");
    });

    expect(() => writeSidebarNavGroupStateToStorage({ tools: true })).not.toThrow();
    setSpy.mockRestore();
  });

  it("ignores malformed or non-boolean payloads instead of throwing", () => {
    window.localStorage.setItem(SIDEBAR_NAV_GROUPS_STORAGE_KEY, "{not json");
    expect(readSidebarNavGroupStateFromStorage()).toEqual({});

    window.localStorage.setItem(SIDEBAR_NAV_GROUPS_STORAGE_KEY, '["tools"]');
    expect(readSidebarNavGroupStateFromStorage()).toEqual({});

    window.localStorage.setItem(SIDEBAR_NAV_GROUPS_STORAGE_KEY, '{"tools":"yes","other":true}');
    expect(readSidebarNavGroupStateFromStorage()).toEqual({ other: true });
  });
});
