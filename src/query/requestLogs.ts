import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  requestAttemptLogsByTraceId,
  requestLogGet,
  requestLogsListAfterIdAll,
  requestLogsListAll,
  type RequestLogSummary,
} from "../services/requestLogs";
import { requestLogsKeys } from "./keys";

const CLAUDE_INTERNAL_COUNT_TOKENS_PATH = "/v1/messages/count_tokens";
type RequestLogsListQueryResult = RequestLogSummary[] | null;
type RequestLogsIncrementalPollResult = number | null;
type RequestLogsIncrementalRefreshResult = {
  mode: "full" | "incremental";
  items: RequestLogSummary[] | null;
};

function isRequestLogsQueryEnabled(enabled: boolean | undefined) {
  return enabled ?? true;
}

function shouldHideRequestLog(log: Pick<RequestLogSummary, "cli_key" | "path">) {
  // Claude CLI 首次对话前会发内部 count_tokens 预请求；它失败时不该伪装成真实对话失败。
  return log.cli_key === "claude" && log.path === CLAUDE_INTERNAL_COUNT_TOKENS_PATH;
}

function filterVisibleRequestLogs(rows: RequestLogSummary[]) {
  return rows.filter((row) => !shouldHideRequestLog(row));
}

function requestLogCreatedAtMs(log: Pick<RequestLogSummary, "created_at" | "created_at_ms">) {
  const ms = log.created_at_ms ?? 0;
  if (Number.isFinite(ms) && ms > 0) return ms;
  return log.created_at * 1000;
}

function sortRequestLogsDesc(a: RequestLogSummary, b: RequestLogSummary) {
  const aTsMs = requestLogCreatedAtMs(a);
  const bTsMs = requestLogCreatedAtMs(b);
  if (aTsMs !== bTsMs) return bTsMs - aTsMs;
  return b.id - a.id;
}

function computeRequestLogsCursorId(rows: RequestLogSummary[]) {
  let maxId = 0;
  for (const row of rows) {
    if (Number.isFinite(row.id) && row.id > maxId) maxId = row.id;
  }
  return maxId;
}

function mergeRequestLogs(prev: RequestLogSummary[], incoming: RequestLogSummary[], limit: number) {
  const byId = new Map<number, RequestLogSummary>();
  for (const row of incoming) byId.set(row.id, row);
  for (const row of prev) {
    if (!byId.has(row.id)) byId.set(row.id, row);
  }
  const merged = Array.from(byId.values());
  merged.sort(sortRequestLogsDesc);
  return merged.slice(0, limit);
}

export function useRequestLogsListAllQuery(
  limit: number,
  options?: { enabled?: boolean; refetchIntervalMs?: number | false }
) {
  const enabled = isRequestLogsQueryEnabled(options?.enabled);
  return useQuery<RequestLogsListQueryResult>({
    queryKey: requestLogsKeys.listAll(limit),
    queryFn: async () => {
      const rows = await requestLogsListAll(limit);
      return rows == null ? null : filterVisibleRequestLogs(rows);
    },
    enabled,
    placeholderData: keepPreviousData,
    refetchInterval: options?.refetchIntervalMs ?? false,
  });
}

export function useRequestLogsIncrementalPollQuery(
  limit: number,
  options?: { enabled?: boolean; refetchIntervalMs?: number | false }
) {
  const queryClient = useQueryClient();
  const enabled = isRequestLogsQueryEnabled(options?.enabled);

  return useQuery<RequestLogsIncrementalPollResult>({
    queryKey: requestLogsKeys.pollAfterIdAll(limit),
    queryFn: async () => {
      const prev = queryClient.getQueryData<RequestLogSummary[] | null>(
        requestLogsKeys.listAll(limit)
      );

      // Wait until the primary list query has loaded at least once.
      if (prev === undefined) return 0;

      const cursorId = prev?.length ? computeRequestLogsCursorId(prev) : 0;

      const items = cursorId
        ? await requestLogsListAfterIdAll(cursorId, limit)
        : await requestLogsListAll(limit);

      if (items == null) {
        queryClient.setQueryData(requestLogsKeys.listAll(limit), null);
        return null;
      }

      const visibleItems = filterVisibleRequestLogs(items);

      if (!cursorId) {
        queryClient.setQueryData(
          requestLogsKeys.listAll(limit),
          visibleItems.slice().sort(sortRequestLogsDesc)
        );
        return visibleItems.length;
      }

      if (visibleItems.length === 0) return 0;

      queryClient.setQueryData<RequestLogSummary[]>(requestLogsKeys.listAll(limit), (cur) =>
        mergeRequestLogs(cur ?? [], visibleItems, limit)
      );

      return visibleItems.length;
    },
    enabled,
    refetchInterval: options?.refetchIntervalMs ?? false,
    refetchIntervalInBackground: false,
  });
}

export function useRequestLogsIncrementalRefreshMutation(limit: number) {
  const queryClient = useQueryClient();

  return useMutation<RequestLogsIncrementalRefreshResult>({
    mutationFn: async () => {
      const prev = queryClient.getQueryData<RequestLogSummary[] | null>(
        requestLogsKeys.listAll(limit)
      );
      const cursorId = prev?.length ? computeRequestLogsCursorId(prev) : 0;

      if (!cursorId) {
        const rawItems = await requestLogsListAll(limit);
        const items = rawItems == null ? null : filterVisibleRequestLogs(rawItems);
        return { mode: "full" as const, items };
      }

      const rawItems = await requestLogsListAfterIdAll(cursorId, limit);
      const items = rawItems == null ? null : filterVisibleRequestLogs(rawItems);
      return { mode: "incremental" as const, items };
    },
    onSuccess: (result) => {
      if (!result) return;
      if (!result.items) return;

      if (result.mode === "full") {
        queryClient.setQueryData(
          requestLogsKeys.listAll(limit),
          result.items.slice().sort(sortRequestLogsDesc)
        );
        return;
      }

      if (result.items.length === 0) return;

      queryClient.setQueryData<RequestLogSummary[]>(requestLogsKeys.listAll(limit), (cur) =>
        mergeRequestLogs(cur ?? [], result.items ?? [], limit)
      );
    },
  });
}

export function useRequestLogDetailQuery(logId: number | null) {
  return useQuery({
    queryKey: requestLogsKeys.detail(logId),
    queryFn: () => {
      if (logId == null) return null;
      return requestLogGet(logId);
    },
    enabled: logId != null,
    placeholderData: keepPreviousData,
  });
}

export function useRequestAttemptLogsByTraceIdQuery(traceId: string | null, limit: number) {
  return useQuery({
    queryKey: requestLogsKeys.attemptsByTrace(traceId, limit),
    queryFn: () => {
      if (!traceId) return null;
      return requestAttemptLogsByTraceId(traceId, limit);
    },
    enabled: Boolean(traceId),
    placeholderData: keepPreviousData,
  });
}
