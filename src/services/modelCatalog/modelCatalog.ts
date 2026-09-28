import {
  commands,
  type CliModelExportResult,
  type ModelCatalogApplyDecision,
  type ModelCatalogApplyInput,
  type ModelCatalogItem,
  type ModelCatalogProviderDiff,
  type ModelCatalogRefreshPreview,
  type ModelCatalogResult,
  type ModelCatalogSetEnabledInput,
  type ModelCatalogUpdateInput,
} from "../../generated/bindings";
import { invokeGeneratedIpc, mapGeneratedCommandResponse } from "../generatedIpc";
import type { CliKey } from "../providers/providers";

export type {
  CliModelExportResult,
  ModelCatalogApplyDecision,
  ModelCatalogApplyInput,
  ModelCatalogItem,
  ModelCatalogProviderDiff,
  ModelCatalogRefreshPreview,
  ModelCatalogResult,
  ModelCatalogSetEnabledInput,
  ModelCatalogUpdateInput,
};

export async function modelCatalogList(cliKey: CliKey): Promise<ModelCatalogResult> {
  return invokeGeneratedIpc({
    title: "Load model catalog",
    cmd: "model_catalog_list",
    args: { cliKey },
    invoke: async () => commands.modelCatalogList(cliKey),
  });
}

export async function modelCatalogUpdate(
  input: ModelCatalogUpdateInput
): Promise<ModelCatalogItem> {
  return invokeGeneratedIpc({
    title: "Update model catalog metadata",
    cmd: "model_catalog_update",
    args: { input },
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.modelCatalogUpdate(input), (item) => item),
  });
}

export async function modelCatalogSetEnabled(
  input: ModelCatalogSetEnabledInput
): Promise<ModelCatalogItem> {
  return invokeGeneratedIpc({
    title: "Update model catalog enabled state",
    cmd: "model_catalog_set_enabled",
    args: { input },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.modelCatalogSetEnabled(input),
        (item) => item
      ),
  });
}

export async function modelCatalogRefreshPreview(
  cliKey: CliKey
): Promise<ModelCatalogRefreshPreview> {
  return invokeGeneratedIpc({
    title: "拉取远程模型目录差异",
    cmd: "model_catalog_refresh_preview",
    args: { cliKey },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.modelCatalogRefreshPreview(cliKey),
        (preview) => preview
      ),
  });
}

export async function modelCatalogRefreshApply(input: ModelCatalogApplyInput): Promise<void> {
  return invokeGeneratedIpc({
    title: "应用模型目录更新",
    cmd: "model_catalog_refresh_apply",
    args: { input },
    nullResultBehavior: "return_fallback",
    invoke: async () =>
      mapGeneratedCommandResponse(await commands.modelCatalogRefreshApply(input), () => undefined),
  });
}

export async function modelCatalogExportDefaultProvider(cliKey: CliKey): Promise<string> {
  return invokeGeneratedIpc({
    title: "获取默认供应商名",
    cmd: "model_catalog_export_default_provider",
    args: { cliKey },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.modelCatalogExportDefaultProvider(cliKey),
        (name) => name
      ),
  });
}

export async function modelCatalogExportPi(
  cliKey: CliKey,
  providerName: string
): Promise<CliModelExportResult> {
  return invokeGeneratedIpc({
    title: "刷新 Pi 模型列表",
    cmd: "model_catalog_export_pi",
    args: { cliKey, providerName },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.modelCatalogExportPi(cliKey, providerName),
        (result) => result
      ),
  });
}

export async function modelCatalogExportOpencode(
  cliKey: CliKey,
  providerName: string
): Promise<CliModelExportResult> {
  return invokeGeneratedIpc({
    title: "刷新 OpenCode 模型列表",
    cmd: "model_catalog_export_opencode",
    args: { cliKey, providerName },
    invoke: async () =>
      mapGeneratedCommandResponse(
        await commands.modelCatalogExportOpencode(cliKey, providerName),
        (result) => result
      ),
  });
}
