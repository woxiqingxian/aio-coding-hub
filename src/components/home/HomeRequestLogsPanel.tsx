// Usage:
// - Render as the right side column in `HomeOverviewPanel` to show realtime traces + request logs list.
// - Selection state is controlled by parent; the detail dialog is rendered outside the grid layout.

import { memo, useRef, useMemo } from "react";
import { useNavigate } from "react-router-dom";
import { useVirtualizer } from "@tanstack/react-virtual";
import { cliBadgeTone, cliShortLabel } from "../../constants/clis";
import type { RequestLogSummary } from "../../services/requestLogs";
import type { TraceSession } from "../../services/traceStore";
import { Button } from "../../ui/Button";
import { Card } from "../../ui/Card";
import { Spinner } from "../../ui/Spinner";
import { Tooltip } from "../../ui/Tooltip";
import { cn } from "../../utils/cn";
import {
  computeOutputTokensPerSecond,
  formatDurationMs,
  formatInteger,
  formatRelativeTimeFromUnixSeconds,
  formatTokensPerSecond,
  formatTokensPerSecondShort,
  formatUsdRaw,
  sanitizeTtfbMs,
} from "../../utils/formatters";
import {
  buildRequestRouteMeta,
  computeEffectiveInputTokens,
  computeStatusBadge,
  getErrorCodeLabel,
  SessionReuseBadge,
} from "./HomeLogShared";
import {
  Clock,
  CheckCircle2,
  XCircle,
  Server,
  Terminal,
  Cpu,
  RefreshCw,
  ArrowUpRight,
} from "lucide-react";
import { RealtimeTraceCards } from "./RealtimeTraceCards";

// Estimated height for each request log card (px): padding + 2 rows of content + margin
const ESTIMATED_LOG_CARD_HEIGHT = 90;

// Threshold below which we skip virtualization (overhead not worth it).
// Set to 30 so the default 50-item HomePage list benefits from virtualization.
const VIRTUALIZATION_THRESHOLD = 30;

// Module-level stable reference: pure function, no need to recreate per render.
const formatUnixSecondsStable = (ts: number) => formatRelativeTimeFromUnixSeconds(ts);

type RequestLogCardProps = {
  log: RequestLogSummary;
  isSelected: boolean;
  showCustomTooltip: boolean;
  onSelectLogId: (id: number | null) => void;
  formatUnixSeconds: (ts: number) => string;
};

const RequestLogCard = memo(function RequestLogCard({
  log,
  isSelected,
  showCustomTooltip,
  onSelectLogId,
  formatUnixSeconds,
}: RequestLogCardProps) {
  const statusBadge = computeStatusBadge({
    status: log.status,
    errorCode: log.error_code,
    hasFailover: log.has_failover,
  });

  const providerText =
    log.final_provider_id === 0 ||
    !log.final_provider_name ||
    log.final_provider_name.trim().length === 0 ||
    log.final_provider_name === "Unknown"
      ? "未知"
      : log.final_provider_name;

  const routeMeta = buildRequestRouteMeta({
    route: log.route,
    status: log.status,
    hasFailover: log.has_failover,
    attemptCount: log.attempt_count,
  });

  const providerTitle = providerText;

  const modelText =
    log.requested_model && log.requested_model.trim() ? log.requested_model.trim() : "未知";

  const cliLabel = cliShortLabel(log.cli_key);
  const cliTone = cliBadgeTone(log.cli_key);

  const ttfbMs = sanitizeTtfbMs(log.ttfb_ms, log.duration_ms);
  const outputTokensPerSecond = computeOutputTokensPerSecond(
    log.output_tokens,
    log.duration_ms,
    ttfbMs
  );

  const costMultiplier = log.cost_multiplier;
  const showCostMultiplier =
    Number.isFinite(costMultiplier) && costMultiplier >= 0 && Math.abs(costMultiplier - 1) > 0.0001;
  const rawCostUsdText = formatUsdRaw(log.cost_usd);

  const cacheWrite = (() => {
    // 优先 5m，其次 1h，最后用 cache_creation_input_tokens 汇总
    if (log.cache_creation_5m_input_tokens != null && log.cache_creation_5m_input_tokens > 0) {
      return {
        tokens: log.cache_creation_5m_input_tokens,
        ttl: "5m" as const,
      };
    }
    if (log.cache_creation_1h_input_tokens != null && log.cache_creation_1h_input_tokens > 0) {
      return {
        tokens: log.cache_creation_1h_input_tokens,
        ttl: "1h" as const,
      };
    }
    if (log.cache_creation_input_tokens != null && log.cache_creation_input_tokens > 0) {
      return {
        tokens: log.cache_creation_input_tokens,
        ttl: null,
      };
    }
    return { tokens: null, ttl: null };
  })();

  const effectiveInputTokens = computeEffectiveInputTokens(
    log.cli_key,
    log.input_tokens,
    log.cache_read_input_tokens
  );

  return (
    <button type="button" onClick={() => onSelectLogId(log.id)} className="w-full text-left group">
      <div
        className={cn(
          "relative transition-all duration-200 group/item mx-2 my-1.5 rounded-lg border",
          isSelected
            ? "bg-indigo-50/40 border-indigo-200 shadow-sm dark:bg-indigo-900/20 dark:border-indigo-700"
            : "bg-white border-slate-100 hover:bg-slate-50/60 hover:border-slate-200 hover:shadow-sm dark:bg-slate-800 dark:border-slate-700 dark:hover:bg-slate-700/60 dark:hover:border-slate-600"
        )}
      >
        {/* Selection indicator */}
        <div
          className={cn(
            "absolute left-0 top-2 bottom-2 w-1 rounded-r-full transition-all duration-200",
            isSelected
              ? "bg-indigo-500 opacity-100"
              : "bg-slate-300 opacity-0 group-hover/item:opacity-40"
          )}
        />

        <div className="flex flex-col gap-1.5 px-3 py-2.5">
          {/* Row 1: Status + CLI + Model + Time + Badges */}
          <div className="flex items-center gap-2 min-w-0">
            <span
              className={cn(
                "inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium shrink-0",
                statusBadge.tone
              )}
              title={statusBadge.title}
            >
              {statusBadge.isError ? (
                <XCircle className="h-3 w-3" />
              ) : (
                <CheckCircle2 className="h-3 w-3" />
              )}
              {statusBadge.text}
            </span>

            <span
              className={cn(
                "inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium shrink-0",
                cliTone
              )}
            >
              {log.cli_key === "claude" ? (
                <Terminal className="h-3 w-3" />
              ) : (
                <Cpu className="h-3 w-3" />
              )}
              {cliLabel}
            </span>

            <span
              className="text-xs font-medium text-slate-800 dark:text-slate-200 truncate"
              title={modelText}
            >
              {modelText}
            </span>

            {log.error_code && (
              <span className="rounded bg-amber-50 dark:bg-amber-900/30 px-1 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-400 shrink-0">
                {getErrorCodeLabel(log.error_code)}
              </span>
            )}

            <span className="flex items-center gap-1.5 text-[11px] text-slate-400 dark:text-slate-500 ml-auto shrink-0">
              {log.session_reuse && <SessionReuseBadge showCustomTooltip={showCustomTooltip} />}
              <Clock className="h-3 w-3" />
              {formatUnixSeconds(log.created_at)}
            </span>
          </div>

          {/* Row 2: Provider + Stats Grid (2 rows x 4 cols for alignment) */}
          <div className="flex items-start gap-3 text-[11px]">
            {/* Provider - left side (2 rows: name + multiplier) */}
            <div className="flex flex-col gap-y-0.5 w-[110px] shrink-0" title={providerTitle}>
              <div className="flex items-center gap-1 h-4">
                <Server className="h-3 w-3 text-slate-400 dark:text-slate-500 shrink-0" />
                <span className="truncate font-medium text-slate-600 dark:text-slate-400">
                  {providerText}
                </span>
              </div>
              <div className="flex items-center h-4">
                <div className="flex items-center gap-1 min-w-0 w-full">
                  {routeMeta.hasRoute && routeMeta.tooltipText ? (
                    showCustomTooltip ? (
                      <Tooltip
                        content={routeMeta.tooltipContent}
                        contentClassName="max-w-[400px] break-words"
                        placement="top"
                      >
                        <span className="text-[10px] text-slate-400 dark:text-slate-500 hover:text-indigo-600 dark:hover:text-indigo-400 cursor-help">
                          {routeMeta.label}
                        </span>
                      </Tooltip>
                    ) : (
                      <span
                        className="text-[10px] text-slate-400 dark:text-slate-500 cursor-help"
                        title={routeMeta.tooltipText}
                      >
                        {routeMeta.label}
                      </span>
                    )
                  ) : null}

                  {showCostMultiplier ? (
                    <span className="inline-flex items-center rounded bg-indigo-50 dark:bg-indigo-900/30 px-1 text-[10px] font-medium text-indigo-600 dark:text-indigo-400 shrink-0">
                      x{costMultiplier.toFixed(2)}
                    </span>
                  ) : null}
                </div>
              </div>
            </div>

            {/* Stats Grid: 2 rows x 4 cols */}
            <div className="grid grid-cols-4 gap-x-3 gap-y-0.5 flex-1 text-slate-500 dark:text-slate-400">
              {/* Row 1 */}
              <div className="flex items-center gap-1 h-4" title="Input Tokens">
                <span className="text-slate-400 dark:text-slate-500 shrink-0">输入</span>
                <span className="font-mono tabular-nums text-slate-600 dark:text-slate-300 truncate">
                  {formatInteger(effectiveInputTokens)}
                </span>
              </div>
              <div className="flex items-center gap-1 h-4" title="Cache Write">
                <span className="text-slate-400 dark:text-slate-500 shrink-0">缓存创建</span>
                {cacheWrite.tokens ? (
                  <>
                    <span className="font-mono tabular-nums text-slate-600 dark:text-slate-300 truncate">
                      {formatInteger(cacheWrite.tokens)}
                    </span>
                    {cacheWrite.ttl && (
                      <span className="text-slate-400 dark:text-slate-500 text-[10px]">
                        ({cacheWrite.ttl})
                      </span>
                    )}
                  </>
                ) : (
                  <span className="text-slate-300 dark:text-slate-600">—</span>
                )}
              </div>
              <div className="flex items-center gap-1 h-4" title="TTFB">
                <span className="text-slate-400 dark:text-slate-500 shrink-0">首字</span>
                <span className="font-mono tabular-nums text-slate-600 dark:text-slate-300 truncate">
                  {ttfbMs != null ? formatDurationMs(ttfbMs) : "—"}
                </span>
              </div>
              <div
                className="flex items-center gap-1 h-4"
                title={rawCostUsdText === "—" ? undefined : rawCostUsdText}
              >
                <span className="text-slate-400 dark:text-slate-500 shrink-0">花费</span>
                <span className="font-mono tabular-nums text-slate-600 dark:text-slate-300 truncate">
                  {rawCostUsdText}
                </span>
              </div>

              {/* Row 2 */}
              <div className="flex items-center gap-1 h-4" title="Output Tokens">
                <span className="text-slate-400 dark:text-slate-500 shrink-0">输出</span>
                <span className="font-mono tabular-nums text-slate-600 dark:text-slate-300 truncate">
                  {formatInteger(log.output_tokens)}
                </span>
              </div>
              <div className="flex items-center gap-1 h-4" title="Cache Read">
                <span className="text-slate-400 dark:text-slate-500 shrink-0">缓存读取</span>
                {log.cache_read_input_tokens ? (
                  <span className="font-mono tabular-nums text-slate-600 dark:text-slate-300 truncate">
                    {formatInteger(log.cache_read_input_tokens)}
                  </span>
                ) : (
                  <span className="text-slate-300 dark:text-slate-600">—</span>
                )}
              </div>
              <div className="flex items-center gap-1 h-4" title="Duration">
                <span className="text-slate-400 dark:text-slate-500 shrink-0">耗时</span>
                <span className="font-mono tabular-nums text-slate-600 dark:text-slate-300 truncate">
                  {formatDurationMs(log.duration_ms)}
                </span>
              </div>
              <div
                className="flex items-center gap-1 h-4"
                title={
                  outputTokensPerSecond ? formatTokensPerSecond(outputTokensPerSecond) : undefined
                }
              >
                <span className="text-slate-400 dark:text-slate-500 shrink-0">速率</span>
                {outputTokensPerSecond ? (
                  <span className="font-mono tabular-nums text-slate-600 dark:text-slate-300 truncate">
                    {formatTokensPerSecondShort(outputTokensPerSecond)}
                  </span>
                ) : (
                  <span className="text-slate-300 dark:text-slate-600">—</span>
                )}
              </div>
            </div>
          </div>
        </div>
      </div>
    </button>
  );
});

export type HomeRequestLogsPanelProps = {
  showCustomTooltip: boolean;
  title?: string;
  showOpenLogsPageButton?: boolean;

  traces: TraceSession[];

  requestLogs: RequestLogSummary[];
  requestLogsLoading: boolean;
  requestLogsRefreshing: boolean;
  requestLogsAvailable: boolean | null;
  onRefreshRequestLogs: () => void;

  selectedLogId: number | null;
  onSelectLogId: (id: number | null) => void;
};

export function HomeRequestLogsPanel({
  showCustomTooltip,
  title,
  showOpenLogsPageButton = true,
  traces,
  requestLogs,
  requestLogsLoading,
  requestLogsRefreshing,
  requestLogsAvailable,
  onRefreshRequestLogs,
  selectedLogId,
  onSelectLogId,
}: HomeRequestLogsPanelProps) {
  const navigate = useNavigate();
  const realtimeTraceCandidates = useMemo(() => {
    const nowMs = Date.now();
    return traces
      .filter((t) => nowMs - t.first_seen_ms < 15 * 60 * 1000)
      .sort((a, b) => b.first_seen_ms - a.first_seen_ms)
      .slice(0, 20);
  }, [traces]);

  return (
    <Card padding="sm" className="flex flex-col gap-3 lg:col-span-7 h-full">
      <div className="flex flex-wrap items-center justify-between gap-3 shrink-0">
        <div className="flex flex-wrap items-center gap-2">
          <div className="text-sm font-semibold">{title ?? "使用记录（最近 50 条）"}</div>
        </div>

        <div className="flex items-center gap-2">
          <div className="text-xs text-slate-500 dark:text-slate-400">
            {requestLogsAvailable === false
              ? "仅在 Tauri Desktop 环境可用"
              : requestLogs.length === 0 && requestLogsLoading
                ? "加载中…"
                : requestLogsLoading || requestLogsRefreshing
                  ? `更新中… · 共 ${requestLogs.length} 条`
                  : `共 ${requestLogs.length} 条`}
          </div>
          {showOpenLogsPageButton && (
            <Button
              onClick={() => navigate("/logs")}
              variant="ghost"
              size="sm"
              className="h-8 px-2 text-slate-500 dark:text-slate-400 hover:text-indigo-600 dark:hover:text-indigo-400"
              disabled={requestLogsAvailable === false}
              title="打开日志页"
            >
              <ArrowUpRight className="h-4 w-4 mr-1.5" />
              日志
            </Button>
          )}
          <Button
            onClick={onRefreshRequestLogs}
            variant="ghost"
            size="sm"
            className="h-8 px-2 text-slate-500 dark:text-slate-400 hover:text-indigo-600 dark:hover:text-indigo-400"
            disabled={requestLogsAvailable === false || requestLogsLoading || requestLogsRefreshing}
          >
            <RefreshCw
              className={cn(
                "h-4 w-4 mr-1.5",
                (requestLogsLoading || requestLogsRefreshing) && "animate-spin"
              )}
            />
            刷新
          </Button>
        </div>
      </div>

      <div className="border rounded-lg border-slate-200 dark:border-slate-700 bg-slate-50/30 dark:bg-slate-800/30 shadow-sm overflow-hidden flex-1 min-h-0 flex flex-col">
        <RequestLogsList
          realtimeTraceCandidates={realtimeTraceCandidates}
          formatUnixSeconds={formatUnixSecondsStable}
          showCustomTooltip={showCustomTooltip}
          requestLogsAvailable={requestLogsAvailable}
          requestLogs={requestLogs}
          requestLogsLoading={requestLogsLoading}
          selectedLogId={selectedLogId}
          onSelectLogId={onSelectLogId}
        />
      </div>
    </Card>
  );
}

// Inner list component that conditionally applies virtualization
type RequestLogsListProps = {
  realtimeTraceCandidates: TraceSession[];
  formatUnixSeconds: (ts: number) => string;
  showCustomTooltip: boolean;
  requestLogsAvailable: boolean | null;
  requestLogs: RequestLogSummary[];
  requestLogsLoading: boolean;
  selectedLogId: number | null;
  onSelectLogId: (id: number | null) => void;
};

const RequestLogsList = memo(function RequestLogsList({
  realtimeTraceCandidates,
  formatUnixSeconds,
  showCustomTooltip,
  requestLogsAvailable,
  requestLogs,
  requestLogsLoading,
  selectedLogId,
  onSelectLogId,
}: RequestLogsListProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const useVirtual = requestLogs.length >= VIRTUALIZATION_THRESHOLD;

  const virtualizer = useVirtualizer({
    count: requestLogs.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ESTIMATED_LOG_CARD_HEIGHT,
    overscan: 8,
    enabled: useVirtual,
  });

  const virtualItems = virtualizer.getVirtualItems();

  // Non-virtualized fallback for small lists
  const plainList = !useVirtual && requestLogs.length > 0 && (
    <>
      {requestLogs.map((log) => (
        <RequestLogCard
          key={log.id}
          log={log}
          isSelected={selectedLogId === log.id}
          showCustomTooltip={showCustomTooltip}
          onSelectLogId={onSelectLogId}
          formatUnixSeconds={formatUnixSeconds}
        />
      ))}
    </>
  );

  return (
    <div ref={scrollRef} className="scrollbar-overlay flex-1 overflow-auto pr-1 py-2">
      <RealtimeTraceCards
        traces={realtimeTraceCandidates}
        formatUnixSeconds={formatUnixSeconds}
        showCustomTooltip={showCustomTooltip}
      />

      {requestLogsAvailable === false ? (
        <div className="p-4 text-sm text-slate-600 dark:text-slate-400">
          仅在 Tauri Desktop 环境可用
        </div>
      ) : requestLogs.length === 0 ? (
        requestLogsLoading ? (
          <div className="flex items-center justify-center gap-2 p-4 text-sm text-slate-600 dark:text-slate-400">
            <Spinner size="sm" />
            加载中…
          </div>
        ) : null
      ) : useVirtual ? (
        <div
          style={{
            height: virtualizer.getTotalSize(),
            width: "100%",
            position: "relative",
          }}
        >
          <div
            style={{
              position: "absolute",
              top: 0,
              left: 0,
              width: "100%",
              transform: `translateY(${virtualItems[0]?.start ?? 0}px)`,
            }}
          >
            {virtualItems.map((virtualRow) => (
              <div
                key={requestLogs[virtualRow.index].id}
                data-index={virtualRow.index}
                ref={virtualizer.measureElement}
              >
                <RequestLogCard
                  log={requestLogs[virtualRow.index]}
                  isSelected={selectedLogId === requestLogs[virtualRow.index].id}
                  showCustomTooltip={showCustomTooltip}
                  onSelectLogId={onSelectLogId}
                  formatUnixSeconds={formatUnixSeconds}
                />
              </div>
            ))}
          </div>
        </div>
      ) : (
        plainList
      )}
    </div>
  );
});
