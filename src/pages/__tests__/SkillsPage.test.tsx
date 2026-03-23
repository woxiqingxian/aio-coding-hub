import { render, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { SkillsPage } from "../SkillsPage";
import { createTestQueryClient } from "../../test/utils/reactQuery";
import { setTauriRuntime } from "../../test/utils/tauriRuntime";
import { logToConsole } from "../../services/consoleLog";
import { useWorkspacesListQuery } from "../../query/workspaces";

vi.mock("sonner", () => {
  const toast = Object.assign(vi.fn(), {
    loading: vi.fn(),
    success: vi.fn(),
    error: vi.fn(),
  });
  return { toast };
});

vi.mock("../../services/consoleLog", async () => {
  const actual = await vi.importActual<typeof import("../../services/consoleLog")>(
    "../../services/consoleLog"
  );
  return { ...actual, logToConsole: vi.fn() };
});

const skillsViewMock = vi.fn((_: any) => <div data-testid="skills-view" />);

vi.mock("../skills/SkillsView", () => ({
  SkillsView: (props: unknown) => skillsViewMock(props),
}));

vi.mock("../../query/workspaces", async () => {
  const actual =
    await vi.importActual<typeof import("../../query/workspaces")>("../../query/workspaces");
  return { ...actual, useWorkspacesListQuery: vi.fn() };
});

function renderWithProviders(element: ReactElement) {
  const client = createTestQueryClient();
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>{element}</MemoryRouter>
    </QueryClientProvider>
  );
}

describe("pages/SkillsPage", () => {
  it("renders SkillsView with local import enabled for active workspace", () => {
    setTauriRuntime();

    vi.mocked(useWorkspacesListQuery).mockReturnValue({
      data: { active_id: 42, items: [] },
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<SkillsPage />);

    expect(skillsViewMock).toHaveBeenCalledWith(
      expect.objectContaining({
        workspaceId: 42,
        cliKey: "claude",
        isActiveWorkspace: true,
      })
    );
    expect(skillsViewMock.mock.calls[0]?.[0]).not.toHaveProperty("localImportMode");
  });

  it("reads active cli from localStorage", () => {
    setTauriRuntime();
    localStorage.setItem("skills.activeCli", "codex");

    vi.mocked(useWorkspacesListQuery).mockReturnValue({
      data: { active_id: null, items: [] },
      isFetching: false,
      error: null,
    } as any);

    renderWithProviders(<SkillsPage />);
    expect(useWorkspacesListQuery).toHaveBeenCalledWith("codex");
  });

  it("logs and toasts when workspaces query errors", async () => {
    setTauriRuntime();

    vi.mocked(useWorkspacesListQuery).mockReturnValue({
      data: { active_id: null, items: [] },
      isFetching: false,
      error: new Error("boom"),
    } as any);

    renderWithProviders(<SkillsPage />);

    await waitFor(() => {
      expect(logToConsole).toHaveBeenCalled();
      expect(toast).toHaveBeenCalledWith("加载失败：请查看控制台日志");
    });
  });
});
