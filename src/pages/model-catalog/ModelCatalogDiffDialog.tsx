// Usage: Preview remote-vs-local model catalog diff and apply user-approved changes.

import { useMemo, useState } from "react";
import { toast } from "sonner";
import {
  modelCatalogRefreshApply,
  type ModelCatalogApplyDecision,
  type ModelCatalogProviderDiff,
  type ModelCatalogRefreshPreview,
} from "../../services/modelCatalog/modelCatalog";
import type { CliKey } from "../../constants/clis";
import { Dialog } from "../../ui/Dialog";
import { Button } from "../../ui/Button";
import { cn } from "../../utils/cn";

export type ModelCatalogDiffDialogProps = {
  open: boolean;
  cliKey: CliKey;
  preview: ModelCatalogRefreshPreview | null;
  onClose: () => void;
  onApplied: () => void;
};

export function ModelCatalogDiffDialog({
  open,
  cliKey,
  preview,
  onClose,
  onApplied,
}: ModelCatalogDiffDialogProps) {
  const [applying, setApplying] = useState(false);
  // Per-provider decision: whether to apply additions / removals.
  const [decisions, setDecisions] = useState<
    Record<number, { applyAdded: boolean; applyRemoved: boolean }>
  >({});

  const providers = useMemo(() => preview?.providers ?? [], [preview]);

  function decisionFor(providerId: number, diff: ModelCatalogProviderDiff) {
    return (
      decisions[providerId] ?? {
        // Default: apply additions, but never auto-apply removals — an empty or
        // erroring remote must not silently wipe the catalog.
        applyAdded: diff.added.length > 0,
        applyRemoved: false,
      }
    );
  }

  const anyChange = providers.some(
    (diff) =>
      !diff.error &&
      (diff.added.length > 0 || diff.removed.length > 0) &&
      (decisionFor(diff.providerId, diff).applyAdded ||
        decisionFor(diff.providerId, diff).applyRemoved)
  );

  async function apply() {
    if (!preview || applying) return;
    setApplying(true);
    try {
      const payload: ModelCatalogApplyDecision[] = providers
        .filter((diff) => !diff.error)
        .map((diff) => {
          const decision = decisionFor(diff.providerId, diff);
          return {
            providerId: diff.providerId,
            remoteModels: diff.remoteModels,
            applyAdded: decision.applyAdded,
            applyRemoved: decision.applyRemoved,
          };
        });
      await modelCatalogRefreshApply({ cliKey, decisions: payload });
      toast("模型目录已按所选差异更新");
      onApplied();
      onClose();
    } catch (error) {
      toast(`模型目录更新失败：${String(error)}`);
    } finally {
      setApplying(false);
    }
  }

  return (
    <Dialog
      open={open}
      title="刷新模型目录"
      description="对比远程模型列表与本地目录，勾选要应用的新增或删除"
      className="max-w-2xl"
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
    >
      <div className="space-y-4">
        {providers.length === 0 ? (
          <p className="py-6 text-center text-sm text-muted-foreground">
            当前 CLI 没有可刷新的供应商
          </p>
        ) : (
          providers.map((diff) => (
            <ProviderDiffSection
              key={diff.providerId}
              diff={diff}
              decision={decisionFor(diff.providerId, diff)}
              onChange={(next) =>
                setDecisions((current) => ({ ...current, [diff.providerId]: next }))
              }
            />
          ))
        )}
        <div className="flex items-center justify-end gap-2 border-t border-border/60 pt-3">
          <Button variant="secondary" onClick={onClose}>
            取消
          </Button>
          <Button disabled={applying || !anyChange} onClick={() => void apply()}>
            {applying ? "应用中…" : "应用所选变更"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

function ProviderDiffSection({
  diff,
  decision,
  onChange,
}: {
  diff: ModelCatalogProviderDiff;
  decision: { applyAdded: boolean; applyRemoved: boolean };
  onChange: (next: { applyAdded: boolean; applyRemoved: boolean }) => void;
}) {
  if (diff.error) {
    return (
      <section className="rounded-lg border border-border/60 bg-muted/30 px-3 py-2">
        <header className="text-sm font-medium text-foreground">{diff.providerName}</header>
        <p className="mt-1 text-xs text-destructive">拉取远程列表失败：{diff.error}</p>
      </section>
    );
  }
  if (diff.added.length === 0 && diff.removed.length === 0) {
    return (
      <section className="rounded-lg border border-border/60 px-3 py-2">
        <header className="text-sm font-medium text-foreground">{diff.providerName}</header>
        <p className="mt-1 text-xs text-muted-foreground">与远程一致，无变更</p>
      </section>
    );
  }
  return (
    <section className="rounded-lg border border-border/60 px-3 py-2">
      <header className="text-sm font-medium text-foreground">{diff.providerName}</header>
      {diff.added.length > 0 ? (
        <DiffGroup
          tone="add"
          label={`新增 ${diff.added.length} 个模型`}
          models={diff.added}
          checked={decision.applyAdded}
          onToggle={(applyAdded) => onChange({ ...decision, applyAdded })}
        />
      ) : null}
      {diff.removed.length > 0 ? (
        <DiffGroup
          tone="remove"
          label={`减少 ${diff.removed.length} 个模型`}
          models={diff.removed}
          checked={decision.applyRemoved}
          onToggle={(applyRemoved) => onChange({ ...decision, applyRemoved })}
        />
      ) : null}
    </section>
  );
}

function DiffGroup({
  tone,
  label,
  models,
  checked,
  onToggle,
}: {
  tone: "add" | "remove";
  label: string;
  models: string[];
  checked: boolean;
  onToggle: (checked: boolean) => void;
}) {
  return (
    <label className="mt-2 flex cursor-pointer items-start gap-2 text-xs">
      <input
        type="checkbox"
        className="mt-0.5 h-3.5 w-3.5 accent-current"
        checked={checked}
        onChange={(event) => onToggle(event.target.checked)}
      />
      <span className="min-w-0">
        <span
          className={cn(
            "font-medium",
            tone === "add" ? "text-emerald-600 dark:text-emerald-400" : "text-destructive"
          )}
        >
          {label}
        </span>
        <span className="mt-1 block break-all text-muted-foreground">{models.join("、")}</span>
      </span>
    </label>
  );
}
