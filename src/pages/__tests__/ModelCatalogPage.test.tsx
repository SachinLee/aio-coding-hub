import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import { createTestQueryClient } from "../../test/utils/reactQuery";
import { ModelCatalogPage } from "../ModelCatalogPage";
import {
  modelCatalogList,
  modelCatalogSetEnabled,
  modelCatalogUpdate,
} from "../../services/modelCatalog/modelCatalog";
import { cliManagerCodexModelCatalogRefresh } from "../../services/cli/cliManager";

vi.mock("../../services/modelCatalog/modelCatalog", () => ({
  modelCatalogList: vi.fn(),
  modelCatalogUpdate: vi.fn(),
  modelCatalogSetEnabled: vi.fn(),
}));

vi.mock("../../services/cli/cliManager", () => ({
  cliManagerCodexModelCatalogRefresh: vi.fn(),
}));

function renderPage() {
  return render(
    <QueryClientProvider client={createTestQueryClient()}>
      <ModelCatalogPage />
    </QueryClientProvider>
  );
}

const catalog = {
  cliKey: "codex" as const,
  defaultContextWindow: 1_000_000,
  defaultReasoningEffort: "high",
  supportedReasoningEfforts: ["low", "medium", "high", "xhigh", "max"],
  items: [
    {
      modelId: "gpt-5-test",
      contextWindow: 1_000_000,
      reasoningEffort: "high",
      metadataSource: "default" as const,
      enabled: true,
      routes: [
        {
          providerId: 7,
          providerName: "Primary",
          routeOrder: 0,
          upstreamModelId: "gpt-5-upstream",
          snapshotStatus: "fresh" as const,
          isMapping: true,
        },
      ],
    },
  ],
};

describe("ModelCatalogPage", () => {
  it("shows the four CLIs, route details, and saves context and reasoning metadata", async () => {
    vi.mocked(modelCatalogList).mockResolvedValue(catalog);
    vi.mocked(modelCatalogUpdate).mockResolvedValue({
      ...catalog.items[0],
      contextWindow: 2_000_000,
      metadataSource: "user",
    });

    renderPage();

    expect((await screen.findAllByRole("tab")).map((tab) => tab.textContent)).toEqual([
      "Codex",
      "Claude",
      "Grok",
      "Gemini",
    ]);
    expect(await screen.findByDisplayValue("1M")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /1 条路由/ }));
    expect(screen.getByText(/Primary/)).toBeInTheDocument();
    expect(screen.getByText(/gpt-5-test → gpt-5-upstream/)).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("上下文长度：gpt-5-test"), {
      target: { value: "2M" },
    });
    fireEvent.change(screen.getByLabelText("默认推理强度：gpt-5-test"), {
      target: { value: "xhigh" },
    });
    fireEvent.click(screen.getByRole("button", { name: "保存 gpt-5-test" }));

    await waitFor(() => expect(modelCatalogUpdate).toHaveBeenCalled());
    expect(vi.mocked(modelCatalogUpdate).mock.calls[0]?.[0]).toEqual({
      cliKey: "codex",
      modelId: "gpt-5-test",
      contextWindow: 2_000_000,
      reasoningEffort: "xhigh",
    });
  });

  it("toggles a model enabled flag through the catalog switch", async () => {
    vi.mocked(modelCatalogList).mockResolvedValue(catalog);
    vi.mocked(modelCatalogSetEnabled).mockResolvedValue({
      ...catalog.items[0],
      enabled: false,
      metadataSource: "user",
    });

    renderPage();

    const toggle = await screen.findByRole("switch", { name: "启用 gpt-5-test" });
    expect(toggle).toHaveAttribute("data-state", "checked");

    fireEvent.click(toggle);

    await waitFor(() => expect(modelCatalogSetEnabled).toHaveBeenCalled());
    expect(vi.mocked(modelCatalogSetEnabled).mock.calls[0]?.[0]).toEqual({
      cliKey: "codex",
      modelId: "gpt-5-test",
      enabled: false,
    });
  });

  it("refreshes the Codex catalog only on the Codex tab", async () => {
    vi.mocked(modelCatalogList).mockResolvedValue(catalog);
    vi.mocked(cliManagerCodexModelCatalogRefresh).mockResolvedValue(undefined);

    renderPage();

    const refreshButton = await screen.findByRole("button", { name: "更新 Codex 目录" });
    fireEvent.click(refreshButton);

    await waitFor(() => expect(cliManagerCodexModelCatalogRefresh).toHaveBeenCalled());

    fireEvent.click(screen.getByRole("tab", { name: "Claude" }));
    vi.mocked(modelCatalogList).mockResolvedValue({ ...catalog, cliKey: "claude", items: [] });

    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "更新 Codex 目录" })).not.toBeInTheDocument()
    );
  });
});
