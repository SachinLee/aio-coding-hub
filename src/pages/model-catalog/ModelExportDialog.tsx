// Usage: Prompt for a provider name and export the current CLI's catalog models
// into Pi or OpenCode config.

import { useEffect, useState } from "react";
import { toast } from "sonner";
import {
  modelCatalogExportDefaultProvider,
  modelCatalogExportOpencode,
  modelCatalogExportPi,
  type CliModelExportResult,
} from "../../services/modelCatalog/modelCatalog";
import type { CliKey } from "../../constants/clis";
import { Dialog } from "../../ui/Dialog";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";

export type ExportTarget = "pi" | "opencode";

export type ModelExportDialogProps = {
  open: boolean;
  cliKey: CliKey;
  target: ExportTarget;
  onClose: () => void;
};

const TARGET_LABEL: Record<ExportTarget, string> = {
  pi: "Pi",
  opencode: "OpenCode",
};

export function ModelExportDialog({ open, cliKey, target, onClose }: ModelExportDialogProps) {
  const [providerName, setProviderName] = useState("");
  const [loadingDefault, setLoadingDefault] = useState(false);
  const [running, setRunning] = useState(false);

  // Prefill the conventional `aio-<cli>` provider name when the dialog opens.
  useEffect(() => {
    if (!open) return;
    setProviderName("");
    setLoadingDefault(true);
    modelCatalogExportDefaultProvider(cliKey)
      .then((name) => setProviderName(name))
      .catch(() => setProviderName(`aio-${cliKey}`))
      .finally(() => setLoadingDefault(false));
  }, [open, cliKey]);

  async function run() {
    const name = providerName.trim();
    if (!name || running) return;
    setRunning(true);
    try {
      const result: CliModelExportResult =
        target === "pi"
          ? await modelCatalogExportPi(cliKey, name)
          : await modelCatalogExportOpencode(cliKey, name);
      const action = result.created ? "已新建供应商并写入" : "已更新";
      const count = result.models.length;
      toast(
        result.unchanged
          ? `${TARGET_LABEL[target]} ${result.providerName} 模型列表无变化（${count} 个）`
          : `${TARGET_LABEL[target]} ${result.providerName} ${action} ${count} 个模型`
      );
      onClose();
    } catch (error) {
      toast(`刷新 ${TARGET_LABEL[target]} 模型列表失败：${String(error)}`);
    } finally {
      setRunning(false);
    }
  }

  return (
    <Dialog
      open={open}
      title={`刷新 ${TARGET_LABEL[target]} 模型列表`}
      description={`把当前 ${cliKey} 目录中属于该供应商的模型写入 ${TARGET_LABEL[target]} 配置`}
      className="max-w-md"
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
    >
      <div className="space-y-4">
        <label className="block space-y-1.5 text-sm">
          <span className="text-muted-foreground">供应商名称</span>
          <Input
            value={providerName}
            onChange={(event) => setProviderName(event.target.value)}
            placeholder={loadingDefault ? "加载默认值…" : `aio-${cliKey}`}
            mono
            autoFocus
            onKeyDown={(event) => {
              if (event.key === "Enter") void run();
            }}
          />
          <span className="block text-xs text-muted-foreground">
            若该供应商不存在将自动新建（指向本地网关代理）
          </span>
        </label>
        <div className="flex items-center justify-end gap-2">
          <Button variant="secondary" onClick={onClose}>
            取消
          </Button>
          <Button disabled={running || !providerName.trim()} onClick={() => void run()}>
            {running ? "刷新中…" : "刷新"}
          </Button>
        </div>
      </div>
    </Dialog>
  );
}
