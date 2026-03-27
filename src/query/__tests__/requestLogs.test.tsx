import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  requestAttemptLogsByTraceId,
  requestLogGet,
  requestLogsListAfterIdAll,
  requestLogsListAll,
  type RequestLogSummary,
} from "../../services/requestLogs";
import { createQueryWrapper, createTestQueryClient } from "../../test/utils/reactQuery";
import { setTauriRuntime } from "../../test/utils/tauriRuntime";
import {
  useRequestAttemptLogsByTraceIdQuery,
  useRequestLogDetailQuery,
  useRequestLogsIncrementalPollQuery,
  useRequestLogsIncrementalRefreshMutation,
  useRequestLogsListAllQuery,
} from "../requestLogs";

vi.mock("../../services/requestLogs", async () => {
  const actual = await vi.importActual<typeof import("../../services/requestLogs")>(
    "../../services/requestLogs"
  );
  return {
    ...actual,
    requestLogsListAll: vi.fn(),
    requestLogsListAfterIdAll: vi.fn(),
    requestLogGet: vi.fn(),
    requestAttemptLogsByTraceId: vi.fn(),
  };
});

function makeRequestLogSummary(overrides: Partial<RequestLogSummary> = {}): RequestLogSummary {
  return {
    id: 1,
    trace_id: "trace-1",
    cli_key: "claude",
    method: "POST",
    path: "/v1/messages",
    requested_model: "claude-3-7-sonnet",
    status: 200,
    error_code: null,
    duration_ms: 100,
    ttfb_ms: 50,
    attempt_count: 1,
    has_failover: false,
    start_provider_id: 1,
    start_provider_name: "Provider A",
    final_provider_id: 1,
    final_provider_name: "Provider A",
    final_provider_source_id: null,
    final_provider_source_name: null,
    route: [],
    session_reuse: false,
    input_tokens: null,
    output_tokens: null,
    total_tokens: null,
    cache_read_input_tokens: null,
    cache_creation_input_tokens: null,
    cache_creation_5m_input_tokens: null,
    cache_creation_1h_input_tokens: null,
    cost_usd: null,
    cost_multiplier: 1,
    created_at_ms: null,
    created_at: 10,
    ...overrides,
  };
}

describe("query/requestLogs", () => {
  it("calls requestLogsListAll with tauri runtime", async () => {
    setTauriRuntime();

    vi.mocked(requestLogsListAll).mockResolvedValue([]);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useRequestLogsListAllQuery(10), { wrapper });

    await waitFor(() => {
      expect(requestLogsListAll).toHaveBeenCalledWith(10);
    });
  });

  it("filters Claude internal count_tokens rows from list-all results", async () => {
    setTauriRuntime();

    vi.mocked(requestLogsListAll).mockResolvedValue([
      makeRequestLogSummary({
        id: 1,
        path: "/v1/messages/count_tokens",
        status: 403,
        error_code: "upstream_403",
      }),
      makeRequestLogSummary({ id: 2, path: "/v1/messages", created_at: 11 }),
    ]);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useRequestLogsListAllQuery(10), { wrapper });

    await waitFor(() => {
      expect(result.current.data?.map((row) => row.id)).toEqual([2]);
    });
  });

  it("useRequestLogsListAllQuery enters error state when requestLogsListAll rejects", async () => {
    setTauriRuntime();

    vi.mocked(requestLogsListAll).mockRejectedValue(new Error("request logs query boom"));

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useRequestLogsListAllQuery(10), { wrapper });

    await waitFor(() => {
      expect(result.current.isError).toBe(true);
    });
  });

  it("respects options.enabled=false", async () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useRequestLogsListAllQuery(10, { enabled: false }), { wrapper });
    await Promise.resolve();

    expect(requestLogsListAll).not.toHaveBeenCalled();
  });

  it("does not call requestLogGet when logId is null (even on manual refetch)", async () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useRequestLogDetailQuery(null), { wrapper });
    await act(async () => {
      const res = await result.current.refetch();
      expect(res.data).toBeNull();
    });

    expect(requestLogGet).not.toHaveBeenCalled();
  });

  it("calls requestLogGet when logId is provided", async () => {
    setTauriRuntime();

    vi.mocked(requestLogGet).mockResolvedValue(null);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useRequestLogDetailQuery(1), { wrapper });

    await waitFor(() => {
      expect(requestLogGet).toHaveBeenCalledWith(1);
    });
  });

  it("does not call requestAttemptLogsByTraceId when traceId is null (even on manual refetch)", async () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const { result } = renderHook(() => useRequestAttemptLogsByTraceIdQuery(null, 10), { wrapper });
    await act(async () => {
      const res = await result.current.refetch();
      expect(res.data).toBeNull();
    });

    expect(requestAttemptLogsByTraceId).not.toHaveBeenCalled();
  });

  it("calls requestAttemptLogsByTraceId when traceId is provided", async () => {
    setTauriRuntime();

    vi.mocked(requestAttemptLogsByTraceId).mockResolvedValue([]);

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    renderHook(() => useRequestAttemptLogsByTraceIdQuery("trace-1", 10), { wrapper });

    await waitFor(() => {
      expect(requestAttemptLogsByTraceId).toHaveBeenCalledWith("trace-1", 10);
    });
  });

  it("incremental poll waits for base list and then merges after cursor", async () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    // base list not loaded yet (undefined) -> returns 0 and does not call services
    const { result: poll1 } = renderHook(() => useRequestLogsIncrementalPollQuery(10), { wrapper });
    await act(async () => {
      const res = await poll1.current.refetch();
      expect(res.data).toBe(0);
    });
    expect(requestLogsListAll).not.toHaveBeenCalled();

    // base list is loaded but empty -> full list fetch path, then sorts & sets list cache
    const listKey = ["requestLogs", "list", "all", 10] as any;
    client.setQueryData(listKey, []);

    vi.mocked(requestLogsListAll).mockResolvedValueOnce([
      makeRequestLogSummary({ id: 1, created_at: 10, created_at_ms: null }),
      makeRequestLogSummary({
        id: 99,
        path: "/v1/messages/count_tokens",
        status: 403,
        error_code: "upstream_403",
        created_at: 12,
      }),
      makeRequestLogSummary({ id: 2, created_at: 11, created_at_ms: 1 }),
    ] as any);

    await act(async () => {
      const res = await poll1.current.refetch();
      expect(res.data).toBe(2);
    });

    const sorted = client.getQueryData<any[]>(listKey) ?? [];
    expect(sorted[0]?.id).toBe(1); // created_at_ms fallback wins over created_at
    expect(sorted.some((row) => row.id === 99)).toBe(false);

    // cursor > 0 -> listAfterIdAll and merge path
    vi.mocked(requestLogsListAfterIdAll).mockResolvedValueOnce([
      makeRequestLogSummary({
        id: 4,
        path: "/v1/messages/count_tokens",
        status: 503,
        error_code: "upstream_503",
        created_at: 13,
        created_at_ms: 13000,
      }),
      makeRequestLogSummary({ id: 3, created_at: 12, created_at_ms: 12000 }),
    ] as any);

    await act(async () => {
      const res = await poll1.current.refetch();
      expect(res.data).toBe(1);
    });

    const merged = client.getQueryData<any[]>(listKey) ?? [];
    expect(merged.some((r) => r.id === 3)).toBe(true);
    expect(merged.some((r) => r.id === 4)).toBe(false);
  });

  it("incremental refresh mutation filters internal rows and keeps cache stable on null items", async () => {
    setTauriRuntime();

    const client = createTestQueryClient();
    const wrapper = createQueryWrapper(client);

    const listKey = ["requestLogs", "list", "all", 10] as any;

    vi.mocked(requestLogsListAll).mockResolvedValueOnce([
      makeRequestLogSummary({
        id: 1,
        path: "/v1/messages/count_tokens",
        status: 403,
        error_code: "upstream_403",
      }),
      makeRequestLogSummary({ id: 2, created_at: 10, created_at_ms: null }),
    ] as any);
    const { result } = renderHook(() => useRequestLogsIncrementalRefreshMutation(10), { wrapper });

    await act(async () => {
      const res = await result.current.mutateAsync();
      expect(res?.mode).toBe("full");
      expect(res?.items?.map((row) => row.id)).toEqual([2]);
    });
    expect((client.getQueryData<any[]>(listKey) ?? []).map((row) => row.id)).toEqual([2]);

    client.setQueryData(listKey, [makeRequestLogSummary({ id: 5, created_at: 10 })] as any);
    vi.mocked(requestLogsListAfterIdAll).mockResolvedValueOnce([
      makeRequestLogSummary({
        id: 6,
        path: "/v1/messages/count_tokens",
        status: 503,
        error_code: "upstream_503",
        created_at: 11,
      }),
      makeRequestLogSummary({ id: 7, created_at: 12 }),
    ] as any);
    await act(async () => {
      const res = await result.current.mutateAsync();
      expect(res?.mode).toBe("incremental");
      expect(res?.items?.map((row) => row.id)).toEqual([7]);
    });
    expect((client.getQueryData<any[]>(listKey) ?? []).some((row) => row.id === 6)).toBe(false);
    expect((client.getQueryData<any[]>(listKey) ?? []).some((row) => row.id === 7)).toBe(true);

    vi.mocked(requestLogsListAfterIdAll).mockResolvedValueOnce(null as any);
    await act(async () => {
      const res = await result.current.mutateAsync();
      expect(res?.mode).toBe("incremental");
      expect(res?.items).toBeNull();
    });
    expect((client.getQueryData<any[]>(listKey) ?? []).some((row) => row.id === 7)).toBe(true);
  });
});
