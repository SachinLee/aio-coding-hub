import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import type { ReactElement } from "react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it, vi } from "vitest";
import { LogsPage } from "../LogsPage";
import { createTestQueryClient } from "../../test/utils/reactQuery";
import { clearTauriRuntime, setTauriRuntime } from "../../test/utils/tauriRuntime";
import {
  useRequestAttemptLogsByTraceIdQuery,
  useRequestLogDetailQuery,
  useRequestLogsListAllQuery,
  useActiveRequestLogsSnapshotQuery,
} from "../../query/requestLogs";
import { createRequestLogSummary } from "../../services/gateway/requestLogFixtures";
import type { TraceSession } from "../../services/gateway/traceStore";

const traceStoreState = vi.hoisted(() => ({
  traces: [] as TraceSession[],
}));

const detailDialogState = vi.hoisted(() => ({
  selectedLogId: null as number | null,
}));

vi.mock("../../query/requestLogs", async () => {
  const actual =
    await vi.importActual<typeof import("../../query/requestLogs")>("../../query/requestLogs");
  return {
    ...actual,
    useRequestLogsListAllQuery: vi.fn(),
    useActiveRequestLogsSnapshotQuery: vi.fn(),
    useRequestLogDetailQuery: vi.fn(),
    useRequestAttemptLogsByTraceIdQuery: vi.fn(),
    useRequestLogsRefreshMutation: vi.fn(() => ({
      mutateAsync: vi.fn().mockResolvedValue(undefined),
      isPending: false,
    })),
  };
});

vi.mock("../../services/gateway/traceStore", () => ({
  useTraceStore: () => ({
    traces: traceStoreState.traces,
  }),
}));

vi.mock("../../query/cliSessions", () => ({
  useCliSessionsFolderLookupByIdsQuery: vi.fn(() => ({ data: [], isLoading: false })),
}));

vi.mock("../../components/home/RequestLogDetailDialog", () => ({
  RequestLogDetailDialog: ({
    selectedLogId,
  }: {
    selectedLogId: number | null;
    onSelectLogId: (id: number | null) => void;
  }) => {
    detailDialogState.selectedLogId = selectedLogId;
    return <div data-testid="request-log-detail-dialog">{selectedLogId ?? ""}</div>;
  },
}));

function renderWithProviders(element: ReactElement) {
  const client = createTestQueryClient();
  return render(
    <QueryClientProvider client={client}>
      <MemoryRouter>{element}</MemoryRouter>
    </QueryClientProvider>
  );
}

function mockFeed({
  logs = [],
  activeRequests = [],
  isLoading = false,
  isFetching = false,
}: {
  logs?: Array<Parameters<typeof createRequestLogSummary>[0]>;
  activeRequests?: unknown[];
  isLoading?: boolean;
  isFetching?: boolean;
} = {}) {
  vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
    data: logs.map((item) => createRequestLogSummary(item)),
    isLoading,
    isFetching,
    refetch: vi.fn(),
  } as any);
  vi.mocked(useActiveRequestLogsSnapshotQuery).mockReturnValue({
    data: activeRequests,
    isLoading: false,
    isFetching: false,
    refetch: vi.fn(),
  } as any);
  vi.mocked(useRequestLogDetailQuery).mockReturnValue({
    data: null,
    isFetching: false,
    refetch: vi.fn(),
  } as any);
  vi.mocked(useRequestAttemptLogsByTraceIdQuery).mockReturnValue({
    data: [],
    isFetching: false,
    refetch: vi.fn(),
  } as any);
}

function getTable() {
  return screen.getByRole("table", { name: "请求日志表格" });
}

function getDataRows(table: HTMLElement): HTMLElement[] {
  return Array.from(table.querySelectorAll<HTMLElement>('[data-testid="request-log-row"]'));
}

describe("pages/LogsPage", () => {
  afterEach(() => {
    traceStoreState.traces = [];
    detailDialogState.selectedLogId = null;
  });

  it("disables filters when not running in tauri runtime", () => {
    clearTauriRuntime();
    mockFeed({ logs: [] });
    vi.mocked(useRequestLogsListAllQuery).mockReturnValue({
      data: null,
      isLoading: false,
      isFetching: false,
      refetch: vi.fn(),
    } as any);

    renderWithProviders(<LogsPage />);

    expect(screen.getByRole("switch")).toBeDisabled();
    expect(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400")).toBeDisabled();
    expect(screen.getByPlaceholderText("例：GW_UPSTREAM_TIMEOUT")).toBeDisabled();
    expect(screen.getByPlaceholderText("例：/v1/messages")).toBeDisabled();
  });

  it("shows validation error when status filter expression is invalid", () => {
    setTauriRuntime();
    mockFeed({ logs: [] });

    renderWithProviders(<LogsPage />);

    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: "nope" },
    });
    expect(screen.getByText(/表达式不合法/)).toBeInTheDocument();
  });

  it("renders a table with headers, request rows, and cache hit rate", () => {
    setTauriRuntime();
    mockFeed({
      logs: [
        {
          id: 1,
          trace_id: "t1",
          cli_key: "claude",
          method: "POST",
          path: "/v1/messages",
          requested_model: "claude-3-7-sonnet",
          status: 200,
          duration_ms: 1234,
          ttfb_ms: 120,
          final_provider_name: "Provider A",
          input_tokens: 70,
          output_tokens: 20,
          cache_read_input_tokens: 30,
          cache_creation_input_tokens: 10,
          effective_input_tokens: 70,
          cost_usd: 0.01,
        },
        {
          id: 2,
          trace_id: "t2",
          cli_key: "codex",
          method: "POST",
          path: "/v1/responses",
          requested_model: "gpt-5",
          status: 500,
          error_code: "GW_UPSTREAM_TIMEOUT",
          duration_ms: 500,
          final_provider_name: "Provider B",
        },
      ],
    });

    renderWithProviders(<LogsPage />);

    const table = getTable();
    const headers = within(table)
      .getAllByRole("columnheader")
      .map((h) => h.textContent?.trim());
    expect(headers).toEqual([
      "状态",
      "时间",
      "请求",
      "供应商",
      "Token",
      "缓存命中率",
      "性能",
      "费用",
    ]);

    const rows = getDataRows(table);
    expect(rows).toHaveLength(2);

    // Rows sorted by created_at desc: row0 = Codex 500 error, row1 = Claude 200 success
    const row0 = rows[0];
    expect(row0.textContent).toMatch(/500.*失败/);
    expect(row0.textContent).toContain("上游超时");
    expect(row0.textContent).toContain("—");

    const row1 = rows[1];
    expect(row1.textContent).toMatch(/200.*成功/);
    expect(row1.textContent).toContain("claude-3-7-sonnet");
    expect(row1.textContent).toContain("Provider A");
    expect(row1.textContent).toContain("27.3%");
    expect(row1.textContent).toContain("读 30");
    expect(row1.textContent).toContain("写 10");
    expect(within(row1).getByText("120ms")).toHaveClass("text-emerald-600");
  });

  it("shows 0% hit rate when cache read is zero but denominator is positive", () => {
    setTauriRuntime();
    mockFeed({
      logs: [
        {
          id: 1,
          trace_id: "t1",
          cli_key: "claude",
          status: 200,
          effective_input_tokens: 100,
          cache_read_input_tokens: 0,
          cache_creation_input_tokens: 0,
        },
      ],
    });

    renderWithProviders(<LogsPage />);
    const table = getTable();
    const row = getDataRows(table)[0];
    expect(row.textContent).toContain("0%");
  });

  it("opens the detail dialog when a row is clicked", () => {
    setTauriRuntime();
    mockFeed({
      logs: [
        {
          id: 42,
          trace_id: "t1",
          cli_key: "claude",
          status: 200,
          final_provider_name: "Provider A",
        },
      ],
    });

    renderWithProviders(<LogsPage />);

    const row = getDataRows(getTable())[0];
    fireEvent.click(row);
    expect(detailDialogState.selectedLogId).toBe(42);
  });

  it("filters logs by status expression", () => {
    setTauriRuntime();
    mockFeed({
      logs: [
        { id: 1, cli_key: "claude", status: 200, method: "GET", path: "/" },
        { id: 2, cli_key: "claude", status: 499, error_code: "GW_ABORTED", method: "POST", path: "/v1" },
        { id: 3, cli_key: "codex", status: 524, error_code: "GW_TIMEOUT", method: "POST", path: "/v1/messages" },
      ],
    });

    renderWithProviders(<LogsPage />);

    expect(screen.getByText(/共 3 \/ 3 条/)).toBeInTheDocument();
    expect(getDataRows(getTable())).toHaveLength(3);

    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: "499" },
    });
    expect(screen.getByText(/共 1 \/ 3 条/)).toBeInTheDocument();
    expect(getDataRows(getTable())).toHaveLength(1);
  });

  it("filters logs by negated status expression (!200)", () => {
    setTauriRuntime();
    mockFeed({
      logs: [
        { id: 1, cli_key: "claude", status: 200 },
        { id: 2, cli_key: "claude", status: 499 },
        { id: 3, cli_key: "claude", status: 524 },
      ],
    });
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: "!200" },
    });
    expect(getDataRows(getTable())).toHaveLength(2);
  });

  it("filters logs by >=400 status expression", () => {
    setTauriRuntime();
    mockFeed({
      logs: [
        { id: 1, cli_key: "claude", status: 200 },
        { id: 2, cli_key: "claude", status: 400 },
        { id: 3, cli_key: "claude", status: 524 },
      ],
    });
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: ">=400" },
    });
    expect(getDataRows(getTable())).toHaveLength(2);
  });

  it("filters logs by <=399 status expression", () => {
    setTauriRuntime();
    mockFeed({
      logs: [
        { id: 1, cli_key: "claude", status: 200 },
        { id: 2, cli_key: "claude", status: 400 },
      ],
    });
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：499 / 524 / !200 / >=400"), {
      target: { value: "<=399" },
    });
    expect(getDataRows(getTable())).toHaveLength(1);
  });

  it("filters logs by error_code", () => {
    setTauriRuntime();
    mockFeed({
      logs: [
        { id: 1, cli_key: "claude", status: 200, error_code: null },
        { id: 2, cli_key: "claude", status: 499, error_code: "GW_ABORTED" },
      ],
    });
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：GW_UPSTREAM_TIMEOUT"), {
      target: { value: "ABORTED" },
    });
    expect(getDataRows(getTable())).toHaveLength(1);
  });

  it("filters logs by path", () => {
    setTauriRuntime();
    mockFeed({
      logs: [
        { id: 1, cli_key: "claude", status: 200, method: "GET", path: "/" },
        { id: 2, cli_key: "claude", status: 200, method: "POST", path: "/v1/messages" },
      ],
    });
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：/v1/messages"), {
      target: { value: "messages" },
    });
    expect(getDataRows(getTable())).toHaveLength(1);
  });

  it("renders live traces above the table", () => {
    setTauriRuntime();
    traceStoreState.traces = [
      {
        trace_id: "trace-live",
        cli_key: "claude",
        session_id: null,
        method: "POST",
        path: "/v1/messages",
        query: null,
        requested_model: "claude-3-7-sonnet",
        first_seen_ms: Date.now() - 1000,
        last_seen_ms: Date.now(),
        attempts: [
          {
            trace_id: "trace-live",
            cli_key: "claude",
            method: "POST",
            path: "/v1/messages",
            query: null,
            attempt_index: 1,
            provider_id: 1,
            provider_name: "P1",
            base_url: "https://p1",
            outcome: "started",
            status: null,
            attempt_started_ms: 0,
            attempt_duration_ms: 0,
            session_reuse: false,
          } as any,
        ],
      },
    ];
    mockFeed({
      logs: [{ id: 1, cli_key: "claude", status: 200 }],
      activeRequests: [
        {
          trace_id: "trace-live",
          cli_key: "claude",
          session_id: null,
          method: "POST",
          path: "/v1/messages",
          query: null,
          requested_model: "claude-3-7-sonnet",
          created_at_ms: Date.now() - 1000,
          last_activity_ms: Date.now(),
          current_attempt: null,
        },
      ],
    });

    renderWithProviders(<LogsPage />);

    expect(screen.getByText("进行中")).toBeInTheDocument();
    expect(getDataRows(getTable())).toHaveLength(1);
  });

  it("shows empty state when no logs match filters", () => {
    setTauriRuntime();
    mockFeed({
      logs: [{ id: 1, cli_key: "claude", status: 200, error_code: "OK" }],
    });
    renderWithProviders(<LogsPage />);
    fireEvent.change(screen.getByPlaceholderText("例：GW_UPSTREAM_TIMEOUT"), {
      target: { value: "TIMEOUT" },
    });
    expect(screen.getByText("没有符合筛选条件的代理记录")).toBeInTheDocument();
  });
});
