// Usage:
// - Entry: Home "代理记录" button -> `/#/logs`.
// - Backend commands: `request_logs_list_all`, `request_logs_list_after_id_all`, `request_log_get`, `request_attempt_logs_by_trace_id`.

import { useMemo, useReducer } from "react";
import { RefreshCw } from "lucide-react";
import { cn } from "../utils/cn";
import { RequestLogsTable } from "../components/home/RequestLogsTable";
import { RequestLogDetailDialog } from "../components/home/RequestLogDetailDialog";
import { cliFilterItemsWith, type CliFilterKey } from "../constants/clis";
import { GatewayErrorCodes } from "../constants/gatewayErrorCodes";
import { useRequestLogsFeed } from "../hooks/useRequestLogsFeed";
import { Button } from "../ui/Button";
import { Card } from "../ui/Card";
import { Input } from "../ui/Input";
import { PageHeader } from "../ui/PageHeader";
import { Select } from "../ui/Select";
import { Switch } from "../ui/Switch";
import { TabList } from "../ui/TabList";
import { useTraceStore } from "../services/gateway/traceStore";

const LOGS_PAGE_LIMIT = 200;
const AUTO_REFRESH_INTERVAL_MS = 2000;
const LOG_CLI_FILTER_ITEMS = cliFilterItemsWith("logs");

// Keep the selected model as its original ID; only Select option values are encoded.
const MODEL_FILTER_ALL = null;
const MODEL_FILTER_UNKNOWN = "";
type StatusPredicate = (status: number | null) => boolean;

type LogsPageState = {
  cliKey: CliFilterKey;
  statusFilter: string;
  errorCodeFilter: string;
  pathFilter: string;
  modelFilter: string | null;
  autoRefresh: boolean;
  selectedLogId: number | null;
};

type LogsPageAction =
  | { type: "setCliKey"; cliKey: CliFilterKey }
  | { type: "setStatusFilter"; statusFilter: string }
  | { type: "setErrorCodeFilter"; errorCodeFilter: string }
  | { type: "setPathFilter"; pathFilter: string }
  | { type: "setModelFilter"; modelFilter: string | null }
  | { type: "setAutoRefresh"; autoRefresh: boolean }
  | { type: "setSelectedLogId"; selectedLogId: number | null }
  | { type: "resetFilters" };

const initialLogsPageState: LogsPageState = {
  cliKey: "all",
  statusFilter: "",
  errorCodeFilter: "",
  pathFilter: "",
  modelFilter: MODEL_FILTER_ALL,
  autoRefresh: true,
  selectedLogId: null,
};

function logsPageReducer(state: LogsPageState, action: LogsPageAction): LogsPageState {
  switch (action.type) {
    case "setCliKey":
      return { ...state, cliKey: action.cliKey };
    case "setStatusFilter":
      return { ...state, statusFilter: action.statusFilter };
    case "setErrorCodeFilter":
      return { ...state, errorCodeFilter: action.errorCodeFilter };
    case "setPathFilter":
      return { ...state, pathFilter: action.pathFilter };
    case "setModelFilter":
      return { ...state, modelFilter: action.modelFilter };
    case "setAutoRefresh":
      return { ...state, autoRefresh: action.autoRefresh };
    case "setSelectedLogId":
      return { ...state, selectedLogId: action.selectedLogId };
    case "resetFilters":
      return {
        ...state,
        cliKey: "all",
        statusFilter: "",
        errorCodeFilter: "",
        pathFilter: "",
        modelFilter: MODEL_FILTER_ALL,
      };
  }
}

function buildStatusPredicate(query: string): StatusPredicate | null {
  const raw = query.trim();
  if (!raw) return null;

  const exact = raw.match(/^(\d{3})$/);
  if (exact) {
    const target = Number(exact[1]);
    return (status) => status === target;
  }

  const not = raw.match(/^!\s*(\d{3})$/);
  if (not) {
    const target = Number(not[1]);
    return (status) => status == null || status !== target;
  }

  const gte = raw.match(/^>=\s*(\d{3})$/);
  if (gte) {
    const target = Number(gte[1]);
    return (status) => status != null && status >= target;
  }

  const lte = raw.match(/^<=\s*(\d{3})$/);
  if (lte) {
    const target = Number(lte[1]);
    return (status) => status != null && status <= target;
  }

  return null;
}

type ModelFilterableLog = { requested_model?: string | null };

function matchesModelFilter(log: ModelFilterableLog, filter: string | null): boolean {
  if (filter === MODEL_FILTER_ALL) return true;
  const value = log.requested_model?.trim() ?? "";
  if (filter === MODEL_FILTER_UNKNOWN) return value.length === 0;
  return value === filter;
}

/**
 * Model filter options are derived from the currently loaded rows, so the
 * control never issues an extra request and always matches what is on screen.
 */
function buildModelFilterOptions(
  logs: readonly ModelFilterableLog[],
  activeRequests: readonly ModelFilterableLog[],
  currentFilter: string | null
): string[] {
  const models = new Set<string>();
  for (const row of [...logs, ...activeRequests]) {
    const value = row.requested_model?.trim();
    if (value) models.add(value);
  }

  // A selected model remains visible even after leaving the rolling window.
  if (currentFilter) models.add(currentFilter);
  return [...models].sort((a, b) => a.localeCompare(b));
}
export function LogsPage() {
  const { traces } = useTraceStore();
  const showCustomTooltip = true;

  const [state, dispatch] = useReducer(logsPageReducer, initialLogsPageState);
  const {
    cliKey,
    statusFilter,
    errorCodeFilter,
    pathFilter,
    modelFilter,
    autoRefresh,
    selectedLogId,
  } = state;
  const setSelectedLogId = (selectedLogId: number | null) =>
    dispatch({ type: "setSelectedLogId", selectedLogId });
  const {
    requestLogs,
    activeRequests,
    requestLogsLoading,
    requestLogsRefreshing,
    requestLogsAvailable,
    refreshRequestLogs,
  } = useRequestLogsFeed({
    limit: LOGS_PAGE_LIMIT,
    liveUpdatesEnabled: autoRefresh,
    liveUpdateIntervalMs: AUTO_REFRESH_INTERVAL_MS,
    refreshOnForeground: autoRefresh,
  });

  const statusPredicate = useMemo(() => buildStatusPredicate(statusFilter), [statusFilter]);
  const statusFilterValid = statusFilter.trim().length === 0 || statusPredicate != null;
  const activeFilterCount = [
    cliKey !== "all",
    statusFilter.trim().length > 0,
    errorCodeFilter.trim().length > 0,
    pathFilter.trim().length > 0,
    modelFilter !== MODEL_FILTER_ALL,
  ].filter(Boolean).length;
  const filteredLogs = useMemo(() => {
    const errorNeedle = errorCodeFilter.trim().toLowerCase();
    const pathNeedle = pathFilter.trim().toLowerCase();

    return requestLogs.filter((log) => {
      if (cliKey !== "all" && log.cli_key !== cliKey) return false;
      if (statusPredicate && !statusPredicate(log.status)) return false;
      if (errorNeedle) {
        const raw = (log.error_code ?? "").toLowerCase();
        if (!raw.includes(errorNeedle)) return false;
      }
      if (pathNeedle) {
        const haystack = `${log.method} ${log.path}`.toLowerCase();
        if (!haystack.includes(pathNeedle)) return false;
      }
      if (!matchesModelFilter(log, modelFilter)) return false;
      return true;
    });
  }, [cliKey, errorCodeFilter, modelFilter, pathFilter, requestLogs, statusPredicate]);
  const filteredActiveRequests = useMemo(() => {
    const errorNeedle = errorCodeFilter.trim().toLowerCase();
    const pathNeedle = pathFilter.trim().toLowerCase();

    return activeRequests.filter((request) => {
      if (cliKey !== "all" && request.cli_key !== cliKey) return false;
      if (statusPredicate) return false;
      if (errorNeedle) return false;
      if (pathNeedle) {
        const haystack = `${request.method} ${request.path}`.toLowerCase();
        if (!haystack.includes(pathNeedle)) return false;
      }
      if (!matchesModelFilter(request, modelFilter)) return false;
      return true;
    });
  }, [activeRequests, cliKey, errorCodeFilter, modelFilter, pathFilter, statusPredicate]);
  const modelFilterOptions = useMemo(
    () => buildModelFilterOptions(requestLogs, activeRequests, modelFilter),
    [activeRequests, modelFilter, requestLogs]
  );

  const logsSummaryText =
    requestLogsAvailable === false
      ? undefined
      : requestLogs.length === 0 && requestLogsLoading
        ? "加载中…"
        : requestLogsRefreshing
          ? `更新中… · 共 ${filteredLogs.length} / ${requestLogs.length} 条`
          : `共 ${filteredLogs.length} / ${requestLogs.length} 条`;
  function resetFilters() {
    dispatch({ type: "resetFilters" });
  }

  return (
    <div className="flex h-full flex-col gap-6 overflow-hidden">
      <PageHeader title="代理记录" />

      <Card padding="sm" className="overflow-visible flex flex-col gap-4">
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="text-sm font-semibold text-foreground">筛选条件</div>

          <div className="flex flex-wrap items-center gap-3">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <span>自动刷新</span>
              <Switch
                checked={autoRefresh}
                onCheckedChange={(autoRefresh) => dispatch({ type: "setAutoRefresh", autoRefresh })}
                size="sm"
                disabled={requestLogsAvailable === false}
              />
            </div>
            <Button
              variant="secondary"
              size="sm"
              onClick={resetFilters}
              disabled={activeFilterCount === 0}
            >
              清空筛选
            </Button>
          </div>
        </div>

        <div className="grid items-start gap-4 md:grid-cols-2 xl:grid-cols-[1.35fr_1fr_1fr_1fr]">
          <div className="space-y-2">
            <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              CLI
            </div>
            <TabList
              ariaLabel="CLI 过滤"
              items={LOG_CLI_FILTER_ITEMS}
              value={cliKey}
              onChange={(cliKey) => dispatch({ type: "setCliKey", cliKey })}
              size="sm"
              className="w-full"
              buttonClassName="shrink-0 px-3 py-1.5 whitespace-nowrap"
            />
          </div>

          <div className="space-y-2">
            <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              Status
            </div>
            <Input
              value={statusFilter}
              onChange={(e) => dispatch({ type: "setStatusFilter", statusFilter: e.target.value })}
              placeholder="例：499 / 524 / !200 / >=400"
              mono
              disabled={requestLogsAvailable === false}
            />
            <div className="text-[11px] leading-4 text-muted-foreground">
              支持 `499`、`!200`、`&gt;=400`、`&lt;=399`
            </div>
            {!statusFilterValid ? (
              <div className="text-[11px] leading-4 text-rose-600 dark:text-rose-400">
                表达式不合法：支持 499 / !200 / &gt;=400 / &lt;=399
              </div>
            ) : null}
          </div>

          <div className="space-y-2">
            <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              error_code
            </div>
            <Input
              value={errorCodeFilter}
              onChange={(e) =>
                dispatch({ type: "setErrorCodeFilter", errorCodeFilter: e.target.value })
              }
              placeholder={`例：${GatewayErrorCodes.UPSTREAM_TIMEOUT}`}
              mono
              disabled={requestLogsAvailable === false}
            />
            <div className="text-[11px] leading-4 text-muted-foreground">
              支持按错误码关键字模糊匹配
            </div>
          </div>

          <div className="space-y-2">
            <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              Path
            </div>
            <Input
              value={pathFilter}
              onChange={(e) => dispatch({ type: "setPathFilter", pathFilter: e.target.value })}
              placeholder="例：/v1/messages"
              mono
              disabled={requestLogsAvailable === false}
            />
            <div className="text-[11px] leading-4 text-muted-foreground">
              按请求路径或方法路径组合模糊匹配
            </div>
          </div>

          <div className="space-y-2">
            <div className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              模型
            </div>
            <Select
              aria-label="模型过滤"
              value={modelFilter == null ? "all" : modelFilter === "" ? "unknown" : `model:${modelFilter}`}
              onChange={(e) => {
                const value = e.currentTarget.value;
                dispatch({
                  type: "setModelFilter",
                  modelFilter: value === "all" ? null : value === "unknown" ? "" : value.slice(6),
                });
              }}
              className="w-full"
              disabled={requestLogsAvailable === false}
            >
              <option value="all">全部模型</option>
              <option value="unknown">未识别模型</option>
              {modelFilterOptions.map((value) => (
                <option key={value} value={`model:${value}`}>
                  {value}
                </option>
              ))}
            </Select>
            <div className="text-[11px] leading-4 text-muted-foreground">
              按当前已加载日志的请求模型精确筛选
            </div>
          </div>
        </div>
      </Card>

      <Card padding="sm" className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden">
        <div className="flex flex-wrap items-center justify-between gap-3 shrink-0">
          <div className="text-sm font-semibold">代理记录列表</div>
          <div className="flex items-center gap-2">
            <div className="text-xs text-muted-foreground">{logsSummaryText ?? ""}</div>
            <Button
              onClick={() => void refreshRequestLogs()}
              variant="ghost"
              size="sm"
              className="h-8 gap-1 px-2 text-muted-foreground hover:text-indigo-600 dark:hover:text-indigo-400"
              disabled={
                requestLogsAvailable === false || requestLogsLoading || requestLogsRefreshing
              }
            >
              刷新
              <RefreshCw
                className={cn(
                  "h-3.5 w-3.5",
                  (requestLogsLoading || requestLogsRefreshing) && "animate-spin"
                )}
              />
            </Button>
          </div>
        </div>
        <RequestLogsTable
          traces={traces}
          activeRequests={filteredActiveRequests}
          requestLogs={filteredLogs}
          requestLogsLoading={requestLogsLoading}
          requestLogsRefreshing={requestLogsRefreshing}
          requestLogsAvailable={requestLogsAvailable}
          emptyStateTitle={
            activeFilterCount > 0 ? "没有符合筛选条件的代理记录" : "当前没有代理记录"
          }
          selectedLogId={selectedLogId}
          onSelectLogId={setSelectedLogId}
          showCustomTooltip={showCustomTooltip}
        />
      </Card>
      <RequestLogDetailDialog selectedLogId={selectedLogId} onSelectLogId={setSelectedLogId} />
    </div>
  );
}
