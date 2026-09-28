// Usage:
// - Renders request logs as a compact data table for the dedicated /logs page.
// - Reuses the same projection helpers as HomeRequestLogsPanel but presents
//   data in aligned columns with a sticky header for fast comparison.
import { memo, useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { cliBadgeToneStatic, cliShortLabel } from "../../constants/clis";
import { useNowMs } from "../../hooks/useNowMs";
import { useCliSessionsFolderLookupByIdsQuery } from "../../query/cliSessions";
import type {
  CliSessionsFolderLookupEntry,
  CliSessionsFolderLookupInput,
  CliSessionsSource,
} from "../../services/cli/cliSessions";
import {
  buildRequestActivityProjection,
  shouldTickRequestActivityClock,
  type ActiveRequestSnapshotItem,
  type ProjectedRequestLogRow,
} from "../../services/gateway/requestActivityProjection";
import type { RequestLogSummary } from "../../services/gateway/requestLogs";
import { hasCodexSystemRequestSpecialSetting } from "../../services/gateway/requestLogSpecialSettings";
import type { TraceSession } from "../../services/gateway/traceStore";
import { computeCacheHitRate } from "../../utils/cacheRateMetrics";
import { cn } from "../../utils/cn";
import {
  computeOutputTokensPerSecond,
  formatDurationMs,
  formatInteger,
  formatPercent,
  formatTokensPerSecondShort,
  formatUsd,
  sanitizeTtfbMs,
} from "../../utils/formatters";
import {
  buildRequestLogAuditMeta,
  buildRequestRouteMeta,
  computeStatusBadge,
  resolveCacheCreationDisplay,
} from "./requestLogPresentation";
import {
  ClientIdentityBadge,
  FastModeBadge,
  FolderBadge,
  FreeBadge,
  ReasoningEffortBadge,
  SessionReuseBadge,
} from "./LogBadges";
import {
  formatModelRedirectText,
  hasPriorityServiceTierSpecialSetting,
  resolveModelRedirectFromSpecialSettings,
} from "./requestLogSpecialSettings";
import { getErrorCodeLabel } from "./requestLogErrorLabels";
import { Clock, CheckCircle2, XCircle, Server } from "lucide-react";
import { RealtimeTraceCards } from "./RealtimeTraceCards";
import { CliBrandIcon } from "./CliBrandIcon";
import { Tooltip } from "../../ui/Tooltip";
import { EmptyState } from "../../ui/EmptyState";
import { Spinner } from "../../ui/Spinner";

// Threshold below which we skip virtualization (overhead not worth it).
const VIRTUALIZATION_THRESHOLD = 30;
// Estimated row height for virtualization (px). Each row carries 2-3 stacked
// sub-lines inside several cells; ~64px matches the typical compact row.
const ESTIMATED_ROW_HEIGHT = 64;

const TABLE_MIN_WIDTH_PX = 1380;

const TABLE_COLUMN_WIDTHS = [120, 110, 280, 190, 170, 190, 190, 130] as const;

const TH_CLASS =
  "border-b border-border bg-secondary/70 dark:bg-secondary/70 px-3 py-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground backdrop-blur-sm";
const TD_CLASS = "border-b border-border/60 px-3 py-2 align-top text-[13px]";
const MONO_TD_CLASS = `${TD_CLASS} font-mono tabular-nums text-xs text-secondary-foreground`;
const CELL_VALUE = "block font-mono tabular-nums text-xs font-semibold text-foreground/90 leading-4";

function isFolderLookupCliKey(cliKey: string): cliKey is CliSessionsSource {
  return cliKey === "claude" || cliKey === "codex";
}

function sessionFolderLookupKey(cliKey: string, sessionId: string | null | undefined) {
  const normalized = sessionId?.trim();
  if (!normalized) return null;
  return `${cliKey}:${normalized}`;
}

function formatUnixSecondsStable(ts: number) {
  if (ts == null || !Number.isFinite(ts)) return "—";
  try {
    return new Date(ts * 1000).toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    });
  } catch {
    return String(ts);
  }
}

function formatUnixSecondsFull(ts: number) {
  if (ts == null || !Number.isFinite(ts)) return "—";
  try {
    return new Date(ts * 1000).toLocaleString();
  } catch {
    return String(ts);
  }
}

function cacheHitRateToneClass(rate: number): string {
  if (!Number.isFinite(rate)) return "text-muted-foreground";
  if (rate >= 0.6) return "text-emerald-600 dark:text-emerald-400";
  if (rate >= 0.3) return "text-amber-600 dark:text-amber-400";
  return "text-rose-600 dark:text-rose-400";
}
function ttfbToneClass(ttfbMs: number | null): string {
  if (ttfbMs == null || !Number.isFinite(ttfbMs)) return "text-muted-foreground";
  // 将毫秒转换为秒
  const ttfbSeconds = ttfbMs / 1000;
  if (ttfbSeconds < 10) return "text-emerald-600 dark:text-emerald-400";
  if (ttfbSeconds < 30) return "text-amber-600 dark:text-amber-400";
  return "text-rose-600 dark:text-rose-400";
}

const CACHE_HIT_RATE_TOOLTIP =
  "缓存命中率 = 缓存读取 /（有效输入 + 缓存创建 + 缓存读取）";

type RequestLogRowProps = {
  row: ProjectedRequestLogRow;
  isSelected: boolean;
  sessionFolder: CliSessionsFolderLookupEntry | null;
  showCustomTooltip: boolean;
  onSelectLogId: (id: number | null) => void;
  // Virtualization measurement passthrough (@tanstack/react-virtual)
  "data-index"?: number;
  ref?: React.Ref<HTMLTableRowElement>;
};

const RequestLogTableRow = memo(function RequestLogTableRow({
  row,
  isSelected,
  sessionFolder,
  showCustomTooltip,
  onSelectLogId,
  ...rest
}: RequestLogRowProps) {
  const { log, activityState } = row;
  const auditMeta = buildRequestLogAuditMeta(log);
  const isInterrupted = activityState === "interrupted";
  const statusBadge = isInterrupted
    ? {
        text: "未完成",
        tone: "bg-amber-50 text-amber-600 ring-1 ring-inset ring-amber-500/15 dark:bg-amber-500/15 dark:text-amber-400 dark:ring-amber-400/25",
        title: "请求未完成：历史日志缺少终态，当前网关没有对应的进行中请求",
        isError: false,
      }
    : computeStatusBadge({
        status: log.status,
        errorCode: log.error_code,
        hasFailover: log.has_failover,
      });

  const providerText =
    auditMeta.providerFallbackText ??
    (log.final_provider_id === 0 ||
    !log.final_provider_name ||
    log.final_provider_name.trim().length === 0 ||
    log.final_provider_name === "Unknown"
      ? "未知"
      : log.final_provider_name);

  const routeMeta = buildRequestRouteMeta({
    route: log.route,
    status: log.status,
    hasFailover: log.has_failover,
    attemptCount: log.attempt_count,
  });

  const modelText = formatModelRedirectText(
    log.requested_model,
    resolveModelRedirectFromSpecialSettings(log.special_settings_json, log.final_provider_id)
  );
  const cliLabel = cliShortLabel(log.cli_key);
  const cliTone = cliBadgeToneStatic(log.cli_key);
  const isCodexSystemRequest =
    log.cli_key === "codex" && hasCodexSystemRequestSpecialSetting(log.special_settings_json);

  const ttfbMs = sanitizeTtfbMs(log.ttfb_ms, log.duration_ms);
  const outputTokensPerSecond = computeOutputTokensPerSecond(
    log.output_tokens,
    log.duration_ms,
    ttfbMs
  );

  const costMultiplier = log.cost_multiplier ?? 1;
  const isFree = Number.isFinite(costMultiplier) && costMultiplier === 0;
  const showCostMultiplier =
    Number.isFinite(costMultiplier) && costMultiplier >= 0 && Math.abs(costMultiplier - 1) > 0.0001;
  const costMultiplierText = isFree ? "免费" : `x${costMultiplier.toFixed(2)}`;

  const cacheWrite = resolveCacheCreationDisplay(log);
  const effectiveInputTokens = log.effective_input_tokens ?? null;
  const hitRate = computeCacheHitRate(
    effectiveInputTokens,
    cacheWrite?.tokens ?? null,
    log.cache_read_input_tokens
  );
  const hitRateText = Number.isFinite(hitRate) && hitRate === 0 ? "0%" : formatPercent(hitRate);
  const hitRateTone = cacheHitRateToneClass(hitRate);

  const isPriorityServiceTier =
    log.cli_key === "codex" && hasPriorityServiceTierSpecialSetting(log.special_settings_json);

  const routeSummary = routeMeta.hasRoute ? routeMeta.summary : "直连";
  const errorCodeLabel = log.error_code ? getErrorCodeLabel(log.error_code) : null;

  return (
    <tr
      {...rest}
      data-testid="request-log-row"
      onClick={() => onSelectLogId(log.id > 0 ? log.id : null)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onSelectLogId(log.id > 0 ? log.id : null);
        }
      }}
      tabIndex={0}
      aria-selected={isSelected}
      className={cn(
        "cursor-pointer outline-none transition-colors hover:bg-secondary/50 focus-visible:bg-secondary/70 focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-page-accent/60",
        isSelected && "bg-state-selected/60",
        auditMeta.muted && !isSelected && "opacity-70"
      )}
    >
      {/* 状态 */}
      <td className={TD_CLASS}>
        <span
          className={cn(
            "inline-flex items-center gap-1 whitespace-nowrap rounded-md px-1.5 py-0.5 text-[11px] font-medium",
            statusBadge.tone
          )}
          title={statusBadge.title}
        >
          {isInterrupted ? (
            <Clock className="h-3 w-3 shrink-0" />
          ) : statusBadge.isError ? (
            <XCircle className="h-3 w-3 shrink-0" />
          ) : (
            <CheckCircle2 className="h-3 w-3 shrink-0" />
          )}
          <span>{statusBadge.text}</span>
        </span>
        {errorCodeLabel ? (
          <div className="mt-1 max-w-[120px] truncate text-[10px] text-muted-foreground/80">
            {errorCodeLabel}
          </div>
        ) : null}
      </td>

      {/* 时间 */}
      <td className={MONO_TD_CLASS}>
        <span className="whitespace-nowrap" title={formatUnixSecondsFull(log.created_at)}>
          {formatUnixSecondsStable(log.created_at)}
        </span>
      </td>

      {/* 请求 */}
      <td className={TD_CLASS}>
        <div className="flex min-w-0 flex-col gap-0.5">
          <div className="flex min-w-0 items-center gap-1">
            <span
              className={cn(
                "inline-flex min-w-0 items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-medium",
                cliTone
              )}
              title={`${cliLabel} / ${modelText}`}
            >
              <CliBrandIcon
                cliKey={log.cli_key}
                className="h-2.5 w-2.5 shrink-0 rounded-[3px] object-contain"
              />
              <span className="shrink-0">{cliLabel} /</span>
              <span className="truncate">{modelText}</span>
            </span>
            <ClientIdentityBadge value={log.client_identity} />
            <ReasoningEffortBadge value={log.reasoning_effort} />
            {isPriorityServiceTier ? (
              <FastModeBadge showCustomTooltip={showCustomTooltip} />
            ) : null}
            {isFree ? <FreeBadge /> : null}
          </div>
          <div className="truncate font-mono text-[11px] text-muted-foreground">
            {log.method} {log.path}
          </div>
          <div className="flex min-w-0 flex-wrap items-center gap-1">
            {log.session_reuse ? (
              <SessionReuseBadge showCustomTooltip={showCustomTooltip} />
            ) : null}
            {isCodexSystemRequest ? (
              <span className="shrink-0 whitespace-nowrap rounded-md border border-border/60 bg-muted px-1.5 py-0.5 text-[10px] font-semibold text-foreground">
                系统请求
              </span>
            ) : null}
            {sessionFolder ? (
              <FolderBadge
                folderName={sessionFolder.folder_name}
                folderPath={sessionFolder.folder_path}
              />
            ) : null}
            {auditMeta.tags.map((tag) => (
              <span
                key={tag.label}
                className={cn(
                  "shrink-0 whitespace-nowrap rounded-md px-1.5 py-0.5 text-[10px] font-semibold",
                  tag.className
                )}
                title={tag.title}
              >
                {tag.label}
              </span>
            ))}
          </div>
        </div>
      </td>

      {/* 供应商 */}
      <td className={TD_CLASS}>
        <div className="flex min-w-0 flex-col gap-0.5">
          <div className="flex items-center gap-1">
            <Server className="h-3 w-3 shrink-0 text-muted-foreground/60" />
            <span className="truncate font-semibold text-foreground/85" title={providerText}>
              {providerText}
            </span>
          </div>
          {routeMeta.hasRoute && routeMeta.tooltipText ? (
            showCustomTooltip ? (
              <Tooltip
                content={routeMeta.tooltipContent}
                contentClassName="max-w-[400px] break-words"
                placement="top"
              >
                <span className="text-[11px] text-muted-foreground cursor-help hover:text-indigo-600 dark:hover:text-indigo-400">
                  {routeSummary}
                </span>
              </Tooltip>
            ) : (
              <span
                className="text-[11px] text-muted-foreground cursor-help"
                title={routeMeta.tooltipText}
              >
                {routeSummary}
              </span>
            )
          ) : (
            <span className="text-[11px] text-muted-foreground">{routeSummary}</span>
          )}
        </div>
      </td>

      {/* Token */}
      <td className={MONO_TD_CLASS}>
        <div className="flex flex-col gap-0.5">
          <span className="text-[11px] leading-4">
            <span className="text-muted-foreground/75">输入：</span>
            <span className="font-semibold text-foreground/90">
              {effectiveInputTokens != null ? formatInteger(effectiveInputTokens) : "—"}
            </span>
          </span>
          <span className="text-[11px] leading-4">
            <span className="text-muted-foreground/75">输出：</span>
            <span className="font-semibold text-foreground/90">
              {log.output_tokens != null ? formatInteger(log.output_tokens) : "—"}
            </span>
          </span>
        </div>
      </td>

      {/* 缓存命中率 */}
      <td className={MONO_TD_CLASS} title={CACHE_HIT_RATE_TOOLTIP}>
        <div className="flex flex-col gap-0.5">
          <span className={cn("font-mono tabular-nums text-sm font-bold leading-5", hitRateTone)}>
            {hitRateText}
          </span>
          <span className="text-[10px] text-muted-foreground/80">
            读 {log.cache_read_input_tokens != null ? formatInteger(log.cache_read_input_tokens) : "—"}
            {" / "}写 {formatInteger(cacheWrite?.tokens ?? 0)}
            {cacheWrite?.ttl && cacheWrite.tokens > 0 ? ` (${cacheWrite.ttl})` : ""}
          </span>
        </div>
      </td>

      {/* 性能 */}
      <td className={MONO_TD_CLASS}>
        <div className="flex flex-col gap-0.5">
          <span className="text-[11px] leading-4">
            <span className="text-muted-foreground/75">首字：</span>
            <span className={cn("font-semibold", ttfbToneClass(ttfbMs))}>
              {ttfbMs != null ? formatDurationMs(ttfbMs) : "—"}
            </span>
          </span>
          <span className="text-[11px] leading-4">
            <span className="text-muted-foreground/75">耗时：</span>
            <span className="font-semibold text-foreground/90">{formatDurationMs(log.duration_ms)}</span>
          </span>
          <span className="text-[11px] leading-4">
            <span className="text-muted-foreground/75">速率：</span>
            <span className="font-semibold text-foreground/90">
              {outputTokensPerSecond != null
                ? formatTokensPerSecondShort(outputTokensPerSecond)
                : "—"}
            </span>
          </span>
        </div>
      </td>

      {/* 费用 */}
      <td className={MONO_TD_CLASS}>
        <div className="flex flex-col gap-0.5">
          <span className={CELL_VALUE}>{formatUsd(log.cost_usd)}</span>
          {showCostMultiplier ? (
            <span className="text-[10px] text-muted-foreground/80">{costMultiplierText}</span>
          ) : null}
        </div>
      </td>
    </tr>
  );
});

export type RequestLogsTableProps = {
  traces: TraceSession[];
  activeRequests?: ActiveRequestSnapshotItem[];
  requestLogs: RequestLogSummary[];
  requestLogsLoading: boolean;
  requestLogsRefreshing: boolean;
  requestLogsAvailable: boolean | null;
  emptyStateTitle: string;
  selectedLogId: number | null;
  onSelectLogId: (id: number | null) => void;
  showCustomTooltip: boolean;
  realtimeCardLimit?: number;
};

export function RequestLogsTable({
  traces,
  activeRequests = [],
  requestLogs,
  requestLogsLoading,
  requestLogsRefreshing: _requestLogsRefreshing,
  requestLogsAvailable,
  emptyStateTitle,
  selectedLogId,
  onSelectLogId,
  showCustomTooltip,
  realtimeCardLimit = 5,
}: RequestLogsTableProps) {
  const wallClockNowMs = Date.now();
  const clockEnabled = shouldTickRequestActivityClock({
    requestLogs,
    activeRequests,
    traces,
    nowMs: wallClockNowMs,
  });
  const tickingNowMs = useNowMs(clockEnabled, 250);
  const nowMs = clockEnabled ? tickingNowMs : wallClockNowMs;

  const activityProjection = useMemo(
    () =>
      buildRequestActivityProjection({
        requestLogs,
        activeRequests,
        traces,
        nowMs,
        realtimeCardLimit,
      }),
    [activeRequests, requestLogs, traces, nowMs, realtimeCardLimit]
  );

  const sessionFolderLookupItems = useMemo(() => {
    const seen = new Set<string>();
    const out: CliSessionsFolderLookupInput[] = [];
    const pushIfNeeded = (cliKey: string, sessionId: string | null | undefined) => {
      if (!isFolderLookupCliKey(cliKey)) return;
      const normalized = sessionId?.trim();
      if (!normalized) return;
      const key = `${cliKey}:${normalized}`;
      if (seen.has(key)) return;
      seen.add(key);
      out.push({ source: cliKey, session_id: normalized });
    };
    for (const row of activityProjection.requestRows) {
      pushIfNeeded(row.log.cli_key, row.log.session_id ?? row.liveTrace?.session_id);
    }
    for (const card of activityProjection.realtimeCards) {
      pushIfNeeded(card.trace.cli_key, card.trace.session_id);
    }
    return out;
  }, [activityProjection]);

  const sessionFolderLookupQuery = useCliSessionsFolderLookupByIdsQuery(sessionFolderLookupItems);
  const sessionFolderLookupBySessionKey = useMemo(() => {
    const map = new Map<string, CliSessionsFolderLookupEntry>();
    for (const item of sessionFolderLookupQuery.data ?? []) {
      const key = sessionFolderLookupKey(item.source, item.session_id);
      if (key) map.set(key, item);
    }
    return map;
  }, [sessionFolderLookupQuery.data]);

  const requestRows = activityProjection.requestRows;
  const realtimeCards = activityProjection.realtimeCards;
  const useVirtual = requestRows.length >= VIRTUALIZATION_THRESHOLD;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* Realtime traces section (kept separate from the historical table) */}
      {realtimeCards.length > 0 ? (
        <div className="shrink-0 border-b border-border/40 pb-2">
          <RealtimeTraceCards
            folderLookupBySessionKey={sessionFolderLookupBySessionKey}
            cards={realtimeCards}
            nowMs={nowMs}
            formatUnixSeconds={formatUnixSecondsStable}
            showCustomTooltip={showCustomTooltip}
          />
        </div>
      ) : null}

      {/* Table section */}
      <div className="min-h-0 flex-1 overflow-auto scrollbar-overlay">
        {requestLogsAvailable === false ? (
          <div className="p-4 text-sm text-muted-foreground">数据不可用</div>
        ) : requestRows.length === 0 ? (
          requestLogsLoading ? (
            <div className="flex items-center justify-center gap-2 p-4 text-sm text-muted-foreground">
              <Spinner size="sm" />
              加载中…
            </div>
          ) : (
            <EmptyState title={emptyStateTitle} />
          )
        ) : (
          <table
            className="w-full border-separate border-spacing-0 table-fixed text-left text-sm"
            style={{ minWidth: TABLE_MIN_WIDTH_PX }}
            aria-label="请求日志表格"
          >
            <caption className="sr-only">请求日志表格</caption>
            <colgroup>
              {TABLE_COLUMN_WIDTHS.map((width, index) => (
                <col key={index} style={{ width }} />
              ))}
            </colgroup>
            <thead className="sticky top-0 z-10">
              <tr>
                <th scope="col" className={cn(TH_CLASS, "w-[88px]")}>
                  状态
                </th>
                <th scope="col" className={cn(TH_CLASS, "w-[84px]")}>
                  时间
                </th>
                <th scope="col" className={TH_CLASS}>
                  请求
                </th>
                <th scope="col" className={cn(TH_CLASS, "w-[150px]")}>
                  供应商
                </th>
                <th scope="col" className={cn(TH_CLASS, "w-[110px]")}>
                  Token
                </th>
                <th scope="col" className={cn(TH_CLASS, "w-[120px]")}>
                  缓存命中率
                </th>
                <th scope="col" className={cn(TH_CLASS, "w-[120px]")}>
                  性能
                </th>
                <th scope="col" className={cn(TH_CLASS, "w-[90px]")}>
                  费用
                </th>
              </tr>
            </thead>
            {useVirtual ? (
              <VirtualTableBody
                requestRows={requestRows}
                selectedLogId={selectedLogId}
                folderLookupBySessionKey={sessionFolderLookupBySessionKey}
                showCustomTooltip={showCustomTooltip}
                onSelectLogId={onSelectLogId}
              />
            ) : (
              <tbody>
                {requestRows.map((row) => {
                  const sessionFolder = resolveSessionFolder(
                    row,
                    sessionFolderLookupBySessionKey
                  );
                  return (
                    <RequestLogTableRow
                      key={row.log.id}
                      row={row}
                      isSelected={selectedLogId === row.log.id}
                      sessionFolder={sessionFolder}
                      showCustomTooltip={showCustomTooltip}
                      onSelectLogId={onSelectLogId}
                    />
                  );
                })}
              </tbody>
            )}
          </table>
        )}
      </div>
    </div>
  );
}

function resolveSessionFolder(
  row: ProjectedRequestLogRow,
  map: Map<string, CliSessionsFolderLookupEntry>
): CliSessionsFolderLookupEntry | null {
  const key = sessionFolderLookupKey(
    row.log.cli_key,
    row.log.session_id ?? row.liveTrace?.session_id
  );
  return key ? (map.get(key) ?? null) : null;
}

function VirtualTableBody({
  requestRows,
  selectedLogId,
  folderLookupBySessionKey,
  showCustomTooltip,
  onSelectLogId,
}: {
  requestRows: ProjectedRequestLogRow[];
  selectedLogId: number | null;
  folderLookupBySessionKey: Map<string, CliSessionsFolderLookupEntry>;
  showCustomTooltip: boolean;
  onSelectLogId: (id: number | null) => void;
}) {
  // For virtualization inside a native table we render a single tall spacer
  // row pair trick: top spacer + visible rows + bottom spacer keeps <tbody>
  // semantics while letting the scroll container size reflect all rows.
  const scrollRef = useRef<HTMLTableSectionElement>(null);
  // The virtualizer needs the actual scrolling element — the closest
  // overflow-auto ancestor. We walk up from the tbody to find it.
  const getScrollElement = () => scrollRef.current?.closest(".overflow-auto") ?? null;

  const virtualizer = useVirtualizer({
    count: requestRows.length,
    getScrollElement,
    estimateSize: () => ESTIMATED_ROW_HEIGHT,
    overscan: 10,
  });

  const virtualItems = virtualizer.getVirtualItems();
  const totalSize = virtualizer.getTotalSize();
  const paddingTop = virtualItems.length > 0 ? (virtualItems[0]?.start ?? 0) : 0;
  const paddingBottom =
    virtualItems.length > 0
      ? totalSize - (virtualItems[virtualItems.length - 1]?.end ?? 0)
      : 0;

  return (
    <tbody ref={scrollRef}>
      {paddingTop > 0 ? (
        <tr aria-hidden="true">
          <td colSpan={8} style={{ height: paddingTop, padding: 0, border: 0 }} />
        </tr>
      ) : null}
      {virtualItems.map((virtualRow) => {
        const row = requestRows[virtualRow.index];
        if (!row) return null;
        const sessionFolder = resolveSessionFolder(row, folderLookupBySessionKey);
        return (
          <RequestLogTableRow
            key={row.log.id}
            data-index={virtualRow.index}
            ref={virtualizer.measureElement}
            row={row}
            isSelected={selectedLogId === row.log.id}
            sessionFolder={sessionFolder}
            showCustomTooltip={showCustomTooltip}
            onSelectLogId={onSelectLogId}
          />
        );
      })}
      {paddingBottom > 0 ? (
        <tr aria-hidden="true">
          <td colSpan={8} style={{ height: paddingBottom, padding: 0, border: 0 }} />
        </tr>
      ) : null}
    </tbody>
  );
}
