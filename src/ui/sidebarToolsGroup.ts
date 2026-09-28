// Usage: Persisted expand/collapse state for collapsible sidebar nav groups.
//
// Kept separate from `useSidebarState` so the whole-sidebar open toggle and the
// per-group disclosure state never share a storage key or reset each other.
// One key holds a JSON map so adding a second group needs no new storage key.
// Mirrors the safe read/write pattern of `hooks/useSidebarState.ts`: any
// storage failure or malformed payload falls back to all-collapsed.

export const SIDEBAR_NAV_GROUPS_STORAGE_KEY = "aio-sidebar-nav-groups";

export type SidebarNavGroupState = Record<string, boolean>;

export function readSidebarNavGroupStateFromStorage(): SidebarNavGroupState {
  if (typeof window === "undefined") return {};

  try {
    const raw = window.localStorage.getItem(SIDEBAR_NAV_GROUPS_STORAGE_KEY);
    if (!raw) return {};

    const parsed: unknown = JSON.parse(raw);
    if (parsed == null || typeof parsed !== "object" || Array.isArray(parsed)) return {};

    const result: SidebarNavGroupState = {};
    for (const [groupId, value] of Object.entries(parsed as Record<string, unknown>)) {
      if (typeof value === "boolean") result[groupId] = value;
    }
    return result;
  } catch {
    return {};
  }
}

export function writeSidebarNavGroupStateToStorage(state: SidebarNavGroupState) {
  if (typeof window === "undefined") return;

  try {
    window.localStorage.setItem(SIDEBAR_NAV_GROUPS_STORAGE_KEY, JSON.stringify(state));
  } catch {}
}
