import { useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw, Save } from "lucide-react";
import { toast } from "sonner";
import { CLI_REGISTRY, type CliKey } from "../constants/clis";
import { cliManagerCodexModelCatalogRefresh } from "../services/cli/cliManager";
import {
  modelCatalogList,
  modelCatalogRefreshPreview,
  modelCatalogSetEnabled,
  modelCatalogUpdate,
  type ModelCatalogItem,
  type ModelCatalogRefreshPreview,
  type ModelCatalogResult,
} from "../services/modelCatalog/modelCatalog";
import { ModelCatalogDiffDialog } from "./model-catalog/ModelCatalogDiffDialog";
import { ModelExportDialog, type ExportTarget } from "./model-catalog/ModelExportDialog";
import { PageHeader } from "../ui/PageHeader";
import { TabList } from "../ui/TabList";
import { Button } from "../ui/Button";
import { Switch } from "../ui/Switch";
import { cn } from "../utils/cn";

const REASONING_EFFORTS = ["low", "medium", "high", "xhigh", "max"] as const;
const MODEL_CLI_ORDER: readonly CliKey[] = ["codex", "claude", "grok", "gemini"];

function parseContextWindow(value: string) {
  const match = value.trim().match(/^(\d+(?:\.\d+)?)\s*([km]?)$/i);
  if (!match) return NaN;
  const suffix = match[2].toLowerCase();
  const multiplier = suffix === "m" ? 1_000_000 : suffix === "k" ? 1_000 : 1;
  const tokens = Number(match[1]) * multiplier;
  return Number.isSafeInteger(tokens) ? tokens : NaN;
}

function formatContextWindow(value: number) {
  if (value >= 1_000_000 && value % 1_000_000 === 0) return `${value / 1_000_000}M`;
  if (value >= 1_000 && value % 1_000 === 0) return `${value / 1_000}K`;
  return value.toLocaleString();
}

export function ModelCatalogPage() {
  const queryClient = useQueryClient();
  const [activeCli, setActiveCli] = useState<CliKey>("codex");
  const [expandedModel, setExpandedModel] = useState<string | null>(null);
  const [codexCatalogRefreshing, setCodexCatalogRefreshing] = useState(false);
  const [refreshPreview, setRefreshPreview] = useState<ModelCatalogRefreshPreview | null>(null);
  const [diffDialogOpen, setDiffDialogOpen] = useState(false);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [exportTarget, setExportTarget] = useState<ExportTarget | null>(null);
  const [drafts, setDrafts] = useState<
    Record<string, { contextWindow: string; reasoningEffort: string }>
  >({});
  const queryKey = useMemo(() => ["model-catalog", activeCli] as const, [activeCli]);
  const catalogQuery = useQuery({ queryKey, queryFn: () => modelCatalogList(activeCli) });
  const updateMutation = useMutation({
    mutationFn: modelCatalogUpdate,
    onSuccess: async () => {
      setDrafts({});
      await queryClient.invalidateQueries({ queryKey });
    },
    onError: (error) => {
      toast(`模型元数据保存失败：${error.message}`);
    },
  });
  const setEnabledMutation = useMutation({
    mutationFn: modelCatalogSetEnabled,
    onMutate: async (input) => {
      await queryClient.cancelQueries({ queryKey });
      const previous = queryClient.getQueryData<ModelCatalogResult>(queryKey);
      if (previous) {
        queryClient.setQueryData<ModelCatalogResult>(queryKey, {
          ...previous,
          items: previous.items.map((item) =>
            item.modelId === input.modelId ? { ...item, enabled: input.enabled } : item
          ),
        });
      }
      return { previous };
    },
    onError: (error, _input, context) => {
      if (context?.previous) {
        queryClient.setQueryData(queryKey, context.previous);
      }
      toast(`模型启停更新失败：${error.message}`);
    },
    onSettled: async () => {
      await queryClient.invalidateQueries({ queryKey });
    },
  });
  const tabs = MODEL_CLI_ORDER.map((key) => ({
    key,
    label: CLI_REGISTRY.find((cli) => cli.key === key)?.name ?? key,
  }));
  const result = catalogQuery.data;

  function draftFor(item: ModelCatalogItem) {
    return (
      drafts[item.modelId] ?? {
        contextWindow: formatContextWindow(item.contextWindow),
        reasoningEffort: item.reasoningEffort,
      }
    );
  }

  async function refreshCodexCatalog() {
    if (codexCatalogRefreshing) return;
    setCodexCatalogRefreshing(true);
    try {
      await cliManagerCodexModelCatalogRefresh();
      toast("Codex 模型目录已更新");
      await queryClient.invalidateQueries({ queryKey });
    } catch (error) {
      toast(`Codex 模型目录更新失败：${String(error)}`);
    } finally {
      setCodexCatalogRefreshing(false);
    }
  }

  async function openRefreshDiff() {
    if (previewLoading) return;
    setPreviewLoading(true);
    try {
      const preview = await modelCatalogRefreshPreview(activeCli);
      setRefreshPreview(preview);
      setDiffDialogOpen(true);
    } catch (error) {
      toast(`拉取远程模型列表失败：${String(error)}`);
    } finally {
      setPreviewLoading(false);
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-5 overflow-hidden">
      <PageHeader
        title="模型目录"
        actions={
          <div className="flex items-center gap-3">
            <TabList ariaLabel="CLI 切换" items={tabs} value={activeCli} onChange={setActiveCli} />
            {activeCli === "codex" ? (
              <Button
                type="button"
                variant="secondary"
                size="sm"
                className="h-9"
                title="重新生成 Codex 本地模型目录"
                onClick={() => void refreshCodexCatalog()}
                disabled={codexCatalogRefreshing}
              >
                {codexCatalogRefreshing ? "更新中…" : "更新 Codex 目录"}
              </Button>
            ) : null}
            <Button
              type="button"
              variant="ghost"
              size="icon"
              aria-label="刷新模型目录"
              title="从远程刷新模型目录（差异预览）"
              onClick={() => void openRefreshDiff()}
              disabled={previewLoading}
            >
              <RefreshCw className={previewLoading ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="h-9"
              title="把当前目录中该供应商的模型写入 ~/.pi/agent/models.json"
              onClick={() => setExportTarget("pi")}
            >
              刷新 Pi
            </Button>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="h-9"
              title="把当前目录中该供应商的模型写入 ~/.config/opencode/opencode.json"
              onClick={() => setExportTarget("opencode")}
            >
              刷新 OpenCode
            </Button>
          </div>
        }
      />
      {catalogQuery.isPending ? (
        <div role="status" className="py-10 text-center text-sm text-muted-foreground">
          正在加载模型目录
        </div>
      ) : catalogQuery.isError ? (
        <div role="alert" className="py-10 text-center text-sm text-destructive">
          模型目录加载失败：{catalogQuery.error.message}
        </div>
      ) : !result?.items.length ? (
        <div className="py-10 text-center text-sm text-muted-foreground">
          当前 CLI 暂无可路由模型
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto rounded-lg border border-border/70 bg-card/60">
          <table className="w-full min-w-[980px] border-collapse text-sm">
            <thead className="sticky top-0 z-10">
              <tr className="bg-muted/95 text-left text-xs text-muted-foreground backdrop-blur">
                <th className="border-b border-border/70 px-4 py-3 font-medium">模型</th>
                <th className="w-36 border-b border-border/70 px-4 py-3 font-medium">上下文长度</th>
                <th className="w-36 border-b border-border/70 px-4 py-3 font-medium">
                  默认推理强度
                </th>
                <th className="w-28 border-b border-border/70 px-4 py-3 font-medium">路由</th>
                <th className="w-20 border-b border-border/70 px-4 py-3 font-medium">启用</th>
                <th className="w-24 border-b border-border/70 px-4 py-3 font-medium">操作</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {result.items.map((item) => {
                const draft = draftFor(item);
                const saving =
                  updateMutation.isPending && updateMutation.variables?.modelId === item.modelId;
                const toggling =
                  setEnabledMutation.isPending &&
                  setEnabledMutation.variables?.modelId === item.modelId;
                return (
                  <ModelRow
                    key={item.modelId}
                    cliKey={activeCli}
                    item={item}
                    draft={draft}
                    expanded={expandedModel === item.modelId}
                    saving={saving}
                    toggling={toggling}
                    error={
                      updateMutation.isError && updateMutation.variables?.modelId === item.modelId
                        ? updateMutation.error.message
                        : null
                    }
                    onExpand={() =>
                      setExpandedModel((current) =>
                        current === item.modelId ? null : item.modelId
                      )
                    }
                    onDraftChange={(next) =>
                      setDrafts((current) => ({ ...current, [item.modelId]: next }))
                    }
                    onToggleEnabled={(enabled) =>
                      setEnabledMutation.mutate({
                        cliKey: activeCli,
                        modelId: item.modelId,
                        enabled,
                      })
                    }
                    onSave={() => {
                      const contextWindow = parseContextWindow(draft.contextWindow);
                      if (!Number.isSafeInteger(contextWindow) || contextWindow <= 0) return;
                      updateMutation.mutate({
                        cliKey: activeCli,
                        modelId: item.modelId,
                        contextWindow,
                        reasoningEffort: draft.reasoningEffort,
                      });
                    }}
                  />
                );
              })}
            </tbody>
          </table>
        </div>
      )}
      <ModelCatalogDiffDialog
        open={diffDialogOpen}
        cliKey={activeCli}
        preview={refreshPreview}
        onClose={() => setDiffDialogOpen(false)}
        onApplied={() => void queryClient.invalidateQueries({ queryKey })}
      />
      {exportTarget ? (
        <ModelExportDialog
          open={exportTarget != null}
          cliKey={activeCli}
          target={exportTarget}
          onClose={() => setExportTarget(null)}
        />
      ) : null}
    </div>
  );
}

function ModelRow({
  cliKey,
  item,
  draft,
  expanded,
  saving,
  toggling,
  error,
  onExpand,
  onDraftChange,
  onToggleEnabled,
  onSave,
}: {
  cliKey: CliKey;
  item: ModelCatalogItem;
  draft: { contextWindow: string; reasoningEffort: string };
  expanded: boolean;
  saving: boolean;
  toggling: boolean;
  error: string | null;
  onExpand: () => void;
  onDraftChange: (draft: { contextWindow: string; reasoningEffort: string }) => void;
  onToggleEnabled: (enabled: boolean) => void;
  onSave: () => void;
}) {
  const parsedContextWindow = parseContextWindow(draft.contextWindow);
  const validContext = Number.isSafeInteger(parsedContextWindow) && parsedContextWindow > 0;
  return (
    <>
      <tr
        className={cn(
          "align-middle transition-colors hover:bg-muted/25",
          !item.enabled && "bg-muted/10"
        )}
      >
        <td className="px-4 py-3">
          <button
            className={cn(
              "max-w-full truncate text-left font-medium hover:underline",
              item.enabled ? "text-foreground" : "text-muted-foreground"
            )}
            onClick={onExpand}
            aria-expanded={expanded}
          >
            {item.modelId}
          </button>
          <div className="mt-1 text-xs text-muted-foreground">
            {item.enabled ? null : "已停用 · "}
            {item.metadataSource === "user" ? "已维护" : "默认值"}
            {item.routes.some((route) => route.snapshotStatus === "stale")
              ? " · 含过期路由快照"
              : ""}
          </div>
        </td>
        <td className="px-4 py-3">
          <label className="sr-only" htmlFor={`${cliKey}-${item.modelId}-context`}>
            上下文长度：{item.modelId}
          </label>
          <input
            id={`${cliKey}-${item.modelId}-context`}
            className="h-9 w-28 rounded border border-input bg-background px-2 text-sm"
            inputMode="text"
            value={draft.contextWindow}
            aria-invalid={!validContext}
            onChange={(event) =>
              onDraftChange({ ...draft, contextWindow: event.currentTarget.value })
            }
            title={`当前值 ${formatContextWindow(item.contextWindow)} tokens`}
          />
          <span className="ml-2 text-xs text-muted-foreground">tokens</span>
        </td>
        <td className="px-4 py-3">
          <label className="sr-only" htmlFor={`${cliKey}-${item.modelId}-effort`}>
            默认推理强度：{item.modelId}
          </label>
          <select
            id={`${cliKey}-${item.modelId}-effort`}
            className="h-9 w-28 rounded border border-input bg-background px-2 text-sm"
            value={draft.reasoningEffort}
            onChange={(event) =>
              onDraftChange({ ...draft, reasoningEffort: event.currentTarget.value })
            }
          >
            {REASONING_EFFORTS.map((effort) => (
              <option key={effort} value={effort}>
                {effort}
              </option>
            ))}
          </select>
        </td>
        <td className="px-4 py-3">
          <button
            className="text-left hover:underline"
            onClick={onExpand}
            aria-label={`${item.modelId} 的 ${item.routes.length} 条路由`}
          >
            {item.routes.length} 家供应商
          </button>
        </td>
        <td className="px-4 py-3">
          <Switch
            size="sm"
            checked={item.enabled}
            disabled={toggling}
            onCheckedChange={onToggleEnabled}
            aria-label={`启用 ${item.modelId}`}
          />
        </td>
        <td className="px-4 py-3">
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={onSave}
            disabled={!validContext || saving}
            aria-label={`保存 ${item.modelId}`}
          >
            <Save className="mr-1 h-3.5 w-3.5" />
            保存
          </Button>
          {error && (
            <div role="alert" className="mt-2 max-w-40 text-xs text-destructive">
              {error}
            </div>
          )}
        </td>
      </tr>
      {expanded && (
        <tr className="bg-muted/15">
          <td colSpan={6} className="px-6 py-3">
            <div className="grid gap-2 text-xs sm:grid-cols-2 xl:grid-cols-3">
              {item.routes.map((route) => (
                <div
                  key={`${route.providerId}-${route.upstreamModelId}`}
                  className="flex min-w-0 items-center justify-between gap-3 border-l-2 border-primary/50 pl-3"
                >
                  <span className="truncate font-medium">
                    {route.routeOrder + 1}. {route.providerName}
                  </span>
                  <span className="truncate text-muted-foreground" title={route.upstreamModelId}>
                    {route.isMapping ? `${item.modelId} → ` : ""}
                    {route.upstreamModelId} · {route.snapshotStatus}
                  </span>
                </div>
              ))}
            </div>
          </td>
        </tr>
      )}
    </>
  );
}
