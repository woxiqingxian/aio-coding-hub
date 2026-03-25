// Usage:
// - Manages circuit breaker queries, open-circuit derivation, auto-refresh timer,
//   and provider reset logic for the HomePage.

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { logToConsole } from "../../../services/consoleLog";
import type { OpenCircuitRow } from "../../../components/ProviderCircuitBadge";
import { gatewayKeys } from "../../../query/keys";
import {
  summarizeGatewayCircuitRows,
  useGatewayCircuitResetProviderMutation,
  useGatewayCircuitStatusQuery,
} from "../../../query/gateway";
import { useProvidersListQuery } from "../../../query/providers";

export type HomeCircuitState = {
  openCircuits: OpenCircuitRow[];
  resettingProviderIds: Set<number>;
  handleResetProvider: (providerId: number) => void;
};

export function useHomeCircuitState(): HomeCircuitState {
  const queryClient = useQueryClient();

  const [resettingProviderIds, setResettingProviderIds] = useState<Set<number>>(new Set());
  const resettingProviderIdsRef = useRef(resettingProviderIds);
  resettingProviderIdsRef.current = resettingProviderIds;
  const openCircuitsAutoRefreshTimerRef = useRef<number | null>(null);

  const resetCircuitProviderMutation = useGatewayCircuitResetProviderMutation();
  const claudeCircuitsQuery = useGatewayCircuitStatusQuery("claude");
  const codexCircuitsQuery = useGatewayCircuitStatusQuery("codex");
  const geminiCircuitsQuery = useGatewayCircuitStatusQuery("gemini");
  const claudeProvidersQuery = useProvidersListQuery("claude");
  const codexProvidersQuery = useProvidersListQuery("codex");
  const geminiProvidersQuery = useProvidersListQuery("gemini");
  const claudeCircuitSummary = useMemo(
    () => summarizeGatewayCircuitRows(claudeCircuitsQuery.data),
    [claudeCircuitsQuery.data]
  );
  const codexCircuitSummary = useMemo(
    () => summarizeGatewayCircuitRows(codexCircuitsQuery.data),
    [codexCircuitsQuery.data]
  );
  const geminiCircuitSummary = useMemo(
    () => summarizeGatewayCircuitRows(geminiCircuitsQuery.data),
    [geminiCircuitsQuery.data]
  );

  const openCircuits = useMemo<OpenCircuitRow[]>(() => {
    const specs = [
      {
        cliKey: "claude" as const,
        unavailableRows: claudeCircuitSummary.unavailableRows,
        providers: claudeProvidersQuery.data ?? [],
      },
      {
        cliKey: "codex" as const,
        unavailableRows: codexCircuitSummary.unavailableRows,
        providers: codexProvidersQuery.data ?? [],
      },
      {
        cliKey: "gemini" as const,
        unavailableRows: geminiCircuitSummary.unavailableRows,
        providers: geminiProvidersQuery.data ?? [],
      },
    ];

    const rows: OpenCircuitRow[] = [];
    for (const spec of specs) {
      if (spec.unavailableRows.length === 0) continue;

      const providerNameById: Record<number, string> = {};
      for (const provider of spec.providers) {
        const name = provider.name?.trim();
        if (!name) continue;
        providerNameById[provider.id] = name;
      }

      for (const unavailable of spec.unavailableRows) {
        const { row, unavailableUntil } = unavailable;
        rows.push({
          cli_key: spec.cliKey,
          provider_id: row.provider_id,
          provider_name: providerNameById[row.provider_id] ?? "未知",
          open_until: unavailableUntil,
        });
      }
    }
    rows.sort((a, b) => {
      const aUntil = a.open_until ?? Number.POSITIVE_INFINITY;
      const bUntil = b.open_until ?? Number.POSITIVE_INFINITY;
      if (aUntil !== bUntil) return aUntil - bUntil;
      if (a.cli_key !== b.cli_key) return a.cli_key.localeCompare(b.cli_key);
      return a.provider_name.localeCompare(b.provider_name);
    });

    return rows;
  }, [
    claudeCircuitSummary.unavailableRows,
    claudeProvidersQuery.data,
    codexCircuitSummary.unavailableRows,
    codexProvidersQuery.data,
    geminiCircuitSummary.unavailableRows,
    geminiProvidersQuery.data,
  ]);

  const hasUnavailableCircuit =
    claudeCircuitSummary.hasUnavailable ||
    codexCircuitSummary.hasUnavailable ||
    geminiCircuitSummary.hasUnavailable;
  const earliestUnavailableUntil = useMemo(() => {
    const untils = [
      claudeCircuitSummary.earliestUnavailableUntil,
      codexCircuitSummary.earliestUnavailableUntil,
      geminiCircuitSummary.earliestUnavailableUntil,
    ].filter((value): value is number => value != null);

    if (untils.length === 0) return null;
    return Math.min(...untils);
  }, [
    claudeCircuitSummary.earliestUnavailableUntil,
    codexCircuitSummary.earliestUnavailableUntil,
    geminiCircuitSummary.earliestUnavailableUntil,
  ]);

  const handleResetProvider = useCallback(
    async (providerId: number) => {
      if (resettingProviderIdsRef.current.has(providerId)) return;

      setResettingProviderIds((prev) => new Set(prev).add(providerId));
      try {
        const result = await resetCircuitProviderMutation.mutateAsync({ providerId });
        if (result) {
          toast.success("已解除熔断");
        } else {
          toast.error("解除熔断失败");
        }
      } catch (err) {
        logToConsole("error", "解除熔断失败", { providerId, error: String(err) });
        toast.error("解除熔断失败");
      } finally {
        setResettingProviderIds((prev) => {
          const next = new Set(prev);
          next.delete(providerId);
          return next;
        });
      }
    },
    [resetCircuitProviderMutation]
  );

  // Auto-refresh circuits when the earliest open_until expires
  useEffect(() => {
    if (openCircuitsAutoRefreshTimerRef.current != null) {
      window.clearTimeout(openCircuitsAutoRefreshTimerRef.current);
      openCircuitsAutoRefreshTimerRef.current = null;
    }

    if (!hasUnavailableCircuit) return;

    const nowUnix = Math.floor(Date.now() / 1000);
    const delayMs =
      earliestUnavailableUntil != null
        ? Math.max(200, (earliestUnavailableUntil - nowUnix) * 1000 + 250)
        : 30_000;

    openCircuitsAutoRefreshTimerRef.current = window.setTimeout(() => {
      openCircuitsAutoRefreshTimerRef.current = null;
      queryClient.invalidateQueries({ queryKey: gatewayKeys.circuits() });
    }, delayMs);

    return () => {
      if (openCircuitsAutoRefreshTimerRef.current != null) {
        window.clearTimeout(openCircuitsAutoRefreshTimerRef.current);
        openCircuitsAutoRefreshTimerRef.current = null;
      }
    };
  }, [earliestUnavailableUntil, hasUnavailableCircuit, queryClient]);

  return {
    openCircuits,
    resettingProviderIds,
    handleResetProvider,
  };
}
