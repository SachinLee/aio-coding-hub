import { useState } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { MemoryRouter } from "react-router-dom";
import { afterEach, expect, it, vi } from "vitest";
import { gatewayEventNames } from "../../../constants/gatewayEvents";
import unavailable from "../../../services/gateway/__fixtures__/gatewayEvents/request_unavailable.json";
import { listenGatewayEvents } from "../../../services/gateway/gatewayEvents";
import { useTraceStore } from "../../../services/gateway/traceStore";
import type { ActiveRequestSnapshotItem } from "../../../services/gateway/requestActivityProjection";
import type { RequestLogDetail, RequestLogSummary } from "../../../services/gateway/requestLogs";
import {
  createRequestLogDetail,
  createRequestLogSummary,
} from "../../../services/gateway/requestLogFixtures";
import { clearTauriEventListeners, emitTauriEvent } from "../../../test/mocks/tauri";
import { setTauriRuntime } from "../../../test/utils/tauriRuntime";
import { createTestQueryClient } from "../../../test/utils/reactQuery";
import { HomeRequestLogsPanel } from "../HomeRequestLogsPanel";
import { RequestLogDetailDialog } from "../RequestLogDetailDialog";

const queryState = vi.hoisted(() => ({ detail: null as RequestLogDetail | null }));
vi.mock("../../../query/cliSessions", () => ({
  useCliSessionsFolderLookupByIdsQuery: () => ({ data: [], isLoading: false }),
}));
vi.mock("../../../query/plugins", () => ({
  usePluginActiveContributionsQuery: () => ({ data: { ui: [] }, isLoading: false }),
}));
vi.mock("../../../query/requestLogs", () => ({
  useRequestLogDetailQuery: () => ({
    data: queryState.detail,
    isFetching: false,
    refetch: vi.fn(),
  }),
  useRequestAttemptLogsByTraceIdQuery: () => ({ data: [], isFetching: false, refetch: vi.fn() }),
}));

function FailurePanel({
  logs,
  active,
}: {
  logs: RequestLogSummary[];
  active: ActiveRequestSnapshotItem[];
}) {
  const { traces } = useTraceStore();
  const [selected, setSelected] = useState<number | null>(null);
  return (
    <>
      <output data-testid="trace-count" data-trace-id={traces[0]?.trace_id}>
        {traces.length}
      </output>
      <HomeRequestLogsPanel
        traces={traces}
        activeRequests={active}
        requestLogs={logs}
        requestLogsLoading={false}
        requestLogsRefreshing={false}
        requestLogsAvailable
        onRefreshRequestLogs={vi.fn()}
        selectedLogId={selected}
        onSelectLogId={setSelected}
      />
      <RequestLogDetailDialog selectedLogId={selected} onSelectLogId={setSelected} />
    </>
  );
}

afterEach(() => {
  clearTauriEventListeners();
  vi.useRealTimers();
  queryState.detail = null;
});

it("consumes real failure events through the store and projection, exits activity, and keeps trace details for retries", async () => {
  setTauriRuntime();
  vi.useFakeTimers();
  vi.setSystemTime(1_750_000_000_123);
  const unlisten = await listenGatewayEvents();
  const client = createTestQueryClient();
  const ui = (logs: RequestLogSummary[], active: ActiveRequestSnapshotItem[]) => (
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <FailurePanel logs={logs} active={active} />
      </MemoryRouter>
    </QueryClientProvider>
  );
  const view = render(ui([], []));
  // Prewarm and cached-result reads emit no formal request events.
  expect(screen.getByTestId("trace-count")).toHaveTextContent("0");

  const active: ActiveRequestSnapshotItem = {
    trace_id: unavailable.trace_id,
    cli_key: "codex",
    session_id: unavailable.session_id,
    method: "POST",
    path: "/v1/responses",
    query: null,
    requested_model: "gpt-test",
    created_at_ms: Date.now(),
    last_activity_ms: Date.now(),
    current_attempt: null,
  };
  act(() => emitTauriEvent(gatewayEventNames.requestStart, { ...unavailable, ts: 1_750_000_000 }));
  await act(() => vi.advanceTimersByTimeAsync(250));
  view.rerender(ui([], [active]));
  expect(screen.getByText(/进行中/)).toBeInTheDocument();
  for (const [index, attempt] of unavailable.attempts.entries()) {
    act(() =>
      emitTauriEvent(gatewayEventNames.attempt, {
        ...unavailable,
        ...attempt,
        attempt_index: index + 1,
        claude_model_mapping: null,
      })
    );
  }
  act(() => emitTauriEvent(gatewayEventNames.request, unavailable));
  await act(() => vi.advanceTimersByTimeAsync(250));
  const log = createRequestLogSummary({
    id: 1,
    trace_id: unavailable.trace_id,
    cli_key: "codex",
    path: unavailable.path,
    status: 503,
    error_code: unavailable.error_code,
    requested_model: "gpt-test",
    session_id: unavailable.session_id,
    created_at: 1_750_000_000,
    final_provider_id: 0,
    final_provider_name: "Unknown",
    attempt_count: 2,
    has_failover: false,
    route: unavailable.attempts.map((attempt) => ({
      ...attempt,
      ok: false,
      skipped: true,
      attempts: 1,
    })),
  });
  queryState.detail = createRequestLogDetail({
    ...log,
    attempts_json: JSON.stringify(unavailable.attempts),
    error_details_json: JSON.stringify({ gateway_error_code: unavailable.error_code }),
  });
  view.rerender(ui([log], []));
  await act(() => vi.advanceTimersByTimeAsync(1500));
  expect(screen.queryByText(/进行中/)).not.toBeInTheDocument();
  expect(screen.getByTestId("trace-count")).toHaveTextContent("1");
  expect(screen.queryByText("503 成功")).not.toBeInTheDocument();
  fireEvent.click(screen.getByText(/503/));
  await act(() => vi.advanceTimersByTimeAsync(250));
  expect(screen.getByRole("dialog")).toHaveTextContent("GW_ALL_PROVIDERS_UNAVAILABLE");
  const group = screen.getByText(/供应商熔断 ×2/);
  expect(group).toHaveTextContent("本组最后预计恢复");
  expect(group).not.toHaveTextContent("触发：");
  // A retry references the same saved trace; no new event/history entry is added.
  view.rerender(ui([{ ...log }], []));
  expect(screen.getByTestId("trace-count")).toHaveTextContent("1");
  fireEvent.click(screen.getByRole("tab", { name: "原始数据" }));
  expect(screen.getByRole("dialog")).toHaveTextContent(unavailable.error_code);
  expect(screen.getByTestId("trace-count")).toHaveAttribute("data-trace-id", unavailable.trace_id);
  fireEvent.click(screen.getByRole("button", { name: "attempts_json" }));
  expect(screen.getByRole("dialog")).toHaveTextContent('"upstream_sent": false');

  // An independent early rejection has only a terminal event, with no start/attempt signal.
  act(() =>
    emitTauriEvent(gatewayEventNames.request, {
      ...unavailable,
      trace_id: "trace-early-rejection",
      status: 400,
      error_code: "GW_REQUEST_REJECTED",
      attempts: [],
    })
  );
  await act(() => vi.advanceTimersByTimeAsync(1500));
  expect(screen.getByTestId("trace-count")).toHaveTextContent("2");
  fireEvent.click(screen.getByRole("button", { name: "关闭" }));
  const earlyLog = createRequestLogSummary({
    ...log,
    id: 2,
    trace_id: "trace-early-rejection",
    status: 400,
    error_code: "GW_REQUEST_REJECTED",
    attempt_count: 0,
    route: [],
  });
  view.rerender(ui([earlyLog, log], []));
  expect(screen.getByText(/400/)).toBeInTheDocument();
  expect(screen.queryByText(/进行中/)).not.toBeInTheDocument();
  unlisten();
  client.clear();
});
