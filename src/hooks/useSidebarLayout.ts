import { useCallback, useEffect, useRef, useState, type MouseEvent as ReactMouseEvent } from "react";

export interface SidebarLayoutState {
  workspaceCollapsed: boolean;
  sessionsCollapsed: boolean;
}

export interface SidebarWidthsState {
  workspace: number | null;
  session: number | null;
}

interface UseSidebarLayoutOptions {
  disabled: boolean;
  hasSessions: boolean;
}

export function useSidebarLayout({ disabled, hasSessions }: UseSidebarLayoutOptions) {
  const [sidebarLayout, setSidebarLayout] = useState<SidebarLayoutState>(() => {
    try {
      const raw = localStorage.getItem("ai-gent.sidebar-layout.v1");
      if (raw) {
        const saved = JSON.parse(raw);
        return {
          workspaceCollapsed: Boolean(saved?.workspaceCollapsed),
          sessionsCollapsed: Boolean(saved?.sessionsCollapsed),
        };
      }
    } catch {}
    return {
      workspaceCollapsed: typeof window !== "undefined" && window.innerWidth < 980,
      sessionsCollapsed: typeof window !== "undefined" && window.innerWidth < 720,
    };
  });

  const savedBeforeCollapseRef = useRef<SidebarLayoutState | null>(null);

  const updateSidebarLayout = useCallback(
    (updater: (prev: SidebarLayoutState) => SidebarLayoutState, persist = true) => {
      setSidebarLayout(prev => {
        const next = updater(prev);
        if (persist) {
          try {
            localStorage.setItem("ai-gent.sidebar-layout.v1", JSON.stringify(next));
          } catch {}
        }
        return next;
      });
    },
    [],
  );

  const toggleWorkspaceSidebar = useCallback(
    (collapsed?: boolean) =>
      updateSidebarLayout(prev => ({ ...prev, workspaceCollapsed: collapsed ?? !prev.workspaceCollapsed })),
    [updateSidebarLayout],
  );

  const toggleSessionsSidebar = useCallback(
    (collapsed?: boolean) =>
      updateSidebarLayout(prev => ({ ...prev, sessionsCollapsed: collapsed ?? !prev.sessionsCollapsed })),
    [updateSidebarLayout],
  );

  const [sidebarWidths, setSidebarWidths] = useState<SidebarWidthsState>(() => {
    try {
      const raw = localStorage.getItem("ai-gent.sidebar-widths.v1");
      if (raw) {
        const saved = JSON.parse(raw);
        const w = typeof saved?.workspace === "number" && Number.isFinite(saved.workspace) ? Math.round(saved.workspace) : null;
        const s = typeof saved?.session === "number" && Number.isFinite(saved.session) ? Math.round(saved.session) : null;
        return {
          workspace: w ? Math.max(176, Math.min(420, w)) : null,
          session: s ? Math.max(196, Math.min(440, s)) : null,
        };
      }
    } catch {}
    return { workspace: null, session: null };
  });

  const [resizingSidebar, setResizingSidebar] = useState<"workspace" | "session" | null>(null);

  const persistSidebarWidths = useCallback((next: SidebarWidthsState) => {
    try {
      localStorage.setItem("ai-gent.sidebar-widths.v1", JSON.stringify(next));
    } catch {}
  }, []);

  const startSidebarResize = useCallback(
    (which: "workspace" | "session", event: ReactMouseEvent<HTMLDivElement>) => {
      event.preventDefault();
      const sidebarEl = event.currentTarget.parentElement;
      if (!sidebarEl) return;
      const startX = event.clientX;
      const startWidth = sidebarEl.getBoundingClientRect().width;
      const minWidth = which === "workspace" ? 176 : 196;
      const baseMax = which === "workspace" ? 420 : 440;
      const otherWidth = which === "workspace"
        ? (document.querySelector(".group-session-sidebar")?.getBoundingClientRect().width ?? 0)
        : (document.querySelector(".conversation-sidebar")?.getBoundingClientRect().width ?? 0);
      const railWidth = document.querySelector(".agent-rail")?.getBoundingClientRect().width ?? 64;
      const dynamicMax = Math.max(minWidth, Math.min(baseMax, window.innerWidth - railWidth - otherWidth - 340));

      setResizingSidebar(which);
      const prevCursor = document.body.style.cursor;
      const prevSelect = document.body.style.userSelect;
      document.body.style.cursor = "col-resize";
      document.body.style.userSelect = "none";

      let latestWidth = Math.round(startWidth);
      const onMove = (moveEvent: MouseEvent) => {
        const nextWidth = Math.round(Math.max(minWidth, Math.min(dynamicMax, startWidth + (moveEvent.clientX - startX))));
        latestWidth = nextWidth;
        setSidebarWidths(prev => ({ ...prev, [which]: nextWidth }));
      };
      const onUp = () => {
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
        document.body.style.cursor = prevCursor;
        document.body.style.userSelect = prevSelect;
        setResizingSidebar(null);
        setSidebarWidths(prev => {
          const next = { ...prev, [which]: latestWidth };
          persistSidebarWidths(next);
          return next;
        });
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    },
    [persistSidebarWidths],
  );

  const resetSidebarWidth = useCallback(
    (which: "workspace" | "session") => {
      setSidebarWidths(prev => {
        const next = { ...prev, [which]: null };
        persistSidebarWidths(next);
        return next;
      });
    },
    [persistSidebarWidths],
  );

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "b") {
        if (disabled) return;
        e.preventDefault();
        updateSidebarLayout(prev => {
          const allCollapsed = prev.workspaceCollapsed && (!hasSessions || prev.sessionsCollapsed);
          if (allCollapsed) {
            const restored = savedBeforeCollapseRef.current;
            savedBeforeCollapseRef.current = null;
            if (restored && (!restored.workspaceCollapsed || (hasSessions && !restored.sessionsCollapsed))) {
              return restored;
            }
            return { workspaceCollapsed: false, sessionsCollapsed: false };
          }
          savedBeforeCollapseRef.current = prev;
          return { workspaceCollapsed: true, sessionsCollapsed: hasSessions ? true : prev.sessionsCollapsed };
        });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [disabled, hasSessions, updateSidebarLayout]);

  useEffect(() => {
    let lastWidth = window.innerWidth;
    const onResize = () => {
      const width = window.innerWidth;
      if (lastWidth >= 980 && width < 980) {
        updateSidebarLayout(prev => (prev.workspaceCollapsed ? prev : { ...prev, workspaceCollapsed: true }), false);
      }
      if (lastWidth >= 720 && width < 720) {
        updateSidebarLayout(prev => (prev.sessionsCollapsed ? prev : { ...prev, sessionsCollapsed: true }), false);
      }
      lastWidth = width;
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [updateSidebarLayout]);

  return {
    sidebarLayout,
    sidebarWidths,
    resizingSidebar,
    toggleWorkspaceSidebar,
    toggleSessionsSidebar,
    startSidebarResize,
    resetSidebarWidth,
  };
}
