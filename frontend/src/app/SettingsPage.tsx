import { useState } from "react";
import { core, chooseBackupExportPath, chooseBackupToInspect } from "../lib/core";
import { languageLocale, useI18n } from "../i18n";
import {
  defaultAppearancePreferences,
  isValidHexColor,
  PRESET_DEFAULTS,
  type AppearanceMode,
  type AppearancePreferences,
  type AppearancePreset,
  type FontScale,
} from "../theme";
import type { AppSnapshot } from "../types";
import { SectionHeader } from "../components/UIComponents";
import { useToast } from "../components/Toast";
import { useConfirm } from "../components/ConfirmDialog";

export function SettingsPage({
  provider,
  onChanged,
  appearance,
  onAppearanceChange,
  workspacePath,
}: {
  provider?: AppSnapshot["provider"];
  onChanged: () => void;
  appearance: AppearancePreferences;
  onAppearanceChange: (next: AppearancePreferences) => void;
  workspacePath?: string;
}) {
  const { language, setLanguage, t } = useI18n();
  const { showToast } = useToast();
  const confirm = useConfirm();

  const [baseUrl, setBaseUrl] = useState("https://api.openai.com/v1");
  const [model, setModel] = useState("gpt-4o-mini");
  const [apiKey, setApiKey] = useState("");
  const [busy, setBusy] = useState(false);

  const [showAdvancedColors, setShowAdvancedColors] = useState(false);

  // Appearance handlers
  const handleModeChange = (mode: AppearanceMode) => {
    onAppearanceChange({ ...appearance, mode });
  };

  const handlePresetChange = (preset: AppearancePreset) => {
    onAppearanceChange({
      ...appearance,
      preset,
      light: { accent: null, background: null, text: null },
      dark: { accent: null, background: null, text: null },
    });
  };

  const handleFontScaleChange = (scale: FontScale) => {
    onAppearanceChange({ ...appearance, fontScale: scale });
  };

  const handleCustomColorChange = (
    mode: "light" | "dark",
    field: "accent" | "background" | "text",
    value: string
  ) => {
    const trimmed = value.trim();
    const sanitized = isValidHexColor(trimmed) ? trimmed.toLowerCase() : null;
    onAppearanceChange({
      ...appearance,
      [mode]: {
        ...appearance[mode],
        [field]: sanitized,
      },
    });
  };

  const handleResetAppearance = async () => {
    const ok = await confirm({
      title: t("settings.resetDefaults"),
      message: t("settings.resetConfirm"),
    });
    if (ok) {
      onAppearanceChange(defaultAppearancePreferences());
      showToast(t("common.saved"), "success");
    }
  };

  // Provider handlers
  async function signIn() {
    setBusy(true);
    try {
      const url = await core.loginUrl();
      await core.openExternal(url);
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setBusy(false);
    }
  }

  async function saveByok() {
    setBusy(true);
    try {
      await core.saveByok({ apiKey, baseUrl, model });
      setApiKey("");
      onChanged();
      showToast(t("common.saved"), "success");
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setBusy(false);
    }
  }

  async function disconnect() {
    const ok = await confirm({
      title: t("settings.disconnect"),
      message: t("settings.disconnectConfirm"),
      isDestructive: true,
    });
    if (!ok) return;

    setBusy(true);
    try {
      await core.disconnectAi();
      onChanged();
      showToast(t("common.saved"), "success");
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    } finally {
      setBusy(false);
    }
  }

  async function handleImportBackup() {
    try {
      const path = await chooseBackupToInspect(t("dialog.inspectBackup"));
      if (!path) return;
      const inspection = await core.inspectBackup(path);
      const conflictText = inspection.conflicts.length
        ? `\n${t("backup.conflicts", { count: inspection.conflicts.length })}`
        : "";
      const shouldApply = await confirm({
        title: t("top.importBackup"),
        message: `${t("backup.ready", {
          schema: inspection.schema_version,
          records: inspection.added_records,
        })}${conflictText}\n\n${t("backup.applyReplace")}`,
        confirmText: t("common.confirm"),
      });
      if (shouldApply) {
        await core.applyBackup(inspection.id, "Replace");
        onChanged();
        showToast(t("common.saved"), "success");
      }
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  async function handleExportBackup() {
    try {
      const path = await chooseBackupExportPath(t("dialog.exportBackup"));
      if (!path) return;
      await core.exportBackup(path, languageLocale(language));
      showToast(t("backup.exported"), "success");
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  const activeMode = appearance.mode;
  const currentDefaults = PRESET_DEFAULTS[appearance.preset][activeMode];
  const customValues = appearance[activeMode];

  return (
    <div className="page-content settings-page">
      <SectionHeader
        title={t("settings.title")}
        description={t("settings.description")}
      />

      {/* Appearance Section */}
      <section className="settings-card panel">
        <div className="settings-heading">
          <div className="heading-text">
            <span className="eyebrow">{t("settings.appearance")}</span>
            <h3>{t("settings.appearanceTitle")}</h3>
          </div>
          <button
            className="button subtle small"
            onClick={handleResetAppearance}
          >
            {t("settings.resetDefaults")}
          </button>
        </div>

        {/* Mode Toggle */}
        <div className="setting-row">
          <div className="setting-info">
            <strong>{t("settings.theme")}</strong>
            <span className="setting-description">{t("settings.themeDescription")}</span>
          </div>
          <div className="segmented">
            <button
              className={appearance.mode === "light" ? "active" : ""}
              onClick={() => handleModeChange("light")}
            >
              {t("theme.light")}
            </button>
            <button
              className={appearance.mode === "dark" ? "active" : ""}
              onClick={() => handleModeChange("dark")}
            >
              {t("theme.dark")}
            </button>
          </div>
        </div>

        {/* Theme Preset Choices */}
        <div className="setting-row preset-setting-row">
          <div className="setting-info">
            <strong>{t("settings.preset")}</strong>
            <span className="setting-description">{t("settings.presetDescription")}</span>
          </div>
          <div className="preset-choice-group" role="group" aria-label={t("settings.preset")}>
            <button
              className={`preset-choice ${appearance.preset === "openai" ? "active" : ""}`}
              onClick={() => handlePresetChange("openai")}
            >
              <div className="preset-preview openai-preview">
                <span className="dot accent" />
                <span className="dot surface" />
                <span className="dot text" />
              </div>
              <div className="preset-info">
                <strong>{t("settings.presetOpenai")}</strong>
                <small>{t("settings.presetOpenaiHint")}</small>
              </div>
            </button>

            <button
              className={`preset-choice ${appearance.preset === "ocean" ? "active" : ""}`}
              onClick={() => handlePresetChange("ocean")}
            >
              <div className="preset-preview ocean-preview">
                <span className="dot accent" />
                <span className="dot surface" />
                <span className="dot text" />
              </div>
              <div className="preset-info">
                <strong>{t("settings.presetOcean")}</strong>
                <small>{t("settings.presetOceanHint")}</small>
              </div>
            </button>

            <button
              className={`preset-choice ${appearance.preset === "violet" ? "active" : ""}`}
              onClick={() => handlePresetChange("violet")}
            >
              <div className="preset-preview violet-preview">
                <span className="dot accent" />
                <span className="dot surface" />
                <span className="dot text" />
              </div>
              <div className="preset-info">
                <strong>{t("settings.presetViolet")}</strong>
                <small>{t("settings.presetVioletHint")}</small>
              </div>
            </button>
          </div>
        </div>

        {/* Font Scale Selector */}
        <div className="setting-row">
          <div className="setting-info">
            <strong>{t("settings.fontScale")}</strong>
            <span className="setting-description">{t("settings.fontScaleDescription")}</span>
          </div>
          <div className="segmented">
            <button
              className={appearance.fontScale === 0.9 ? "active" : ""}
              onClick={() => handleFontScaleChange(0.9)}
            >
              {t("settings.scaleCompact")}
            </button>
            <button
              className={appearance.fontScale === 1 ? "active" : ""}
              onClick={() => handleFontScaleChange(1)}
            >
              {t("settings.scaleDefault")}
            </button>
            <button
              className={appearance.fontScale === 1.1 ? "active" : ""}
              onClick={() => handleFontScaleChange(1.1)}
            >
              {t("settings.scaleLarge")}
            </button>
            <button
              className={appearance.fontScale === 1.2 ? "active" : ""}
              onClick={() => handleFontScaleChange(1.2)}
            >
              {t("settings.scaleExtra")}
            </button>
          </div>
        </div>

        {/* Advanced Colors Toggle */}
        <div className="setting-row">
          <div className="setting-info">
            <strong>{t("settings.customColors")}</strong>
            <span className="setting-description">{t("settings.customColorsDescription")}</span>
          </div>
          <button
            className="button subtle small"
            onClick={() => setShowAdvancedColors((v) => !v)}
          >
            {showAdvancedColors ? t("settings.hideAdvanced") : t("settings.customize")}
          </button>
        </div>

        {showAdvancedColors && (
          <div className="advanced-colors-panel">
            <div className="color-field-row">
              <label>{t("settings.accentColor")}</label>
              <div className="color-input-wrap">
                <input
                  type="color"
                  value={customValues.accent || currentDefaults.accent}
                  onChange={(e) => handleCustomColorChange(activeMode, "accent", e.target.value)}
                />
                <input
                  type="text"
                  placeholder={currentDefaults.accent}
                  value={customValues.accent || ""}
                  onChange={(e) => handleCustomColorChange(activeMode, "accent", e.target.value)}
                />
              </div>
            </div>

            <div className="color-field-row">
              <label>{t("settings.backgroundColor")}</label>
              <div className="color-input-wrap">
                <input
                  type="color"
                  value={customValues.background || currentDefaults.background}
                  onChange={(e) =>
                    handleCustomColorChange(activeMode, "background", e.target.value)
                  }
                />
                <input
                  type="text"
                  placeholder={currentDefaults.background}
                  value={customValues.background || ""}
                  onChange={(e) =>
                    handleCustomColorChange(activeMode, "background", e.target.value)
                  }
                />
              </div>
            </div>

            <div className="color-field-row">
              <label>{t("settings.textColor")}</label>
              <div className="color-input-wrap">
                <input
                  type="color"
                  value={customValues.text || currentDefaults.text}
                  onChange={(e) => handleCustomColorChange(activeMode, "text", e.target.value)}
                />
                <input
                  type="text"
                  placeholder={currentDefaults.text}
                  value={customValues.text || ""}
                  onChange={(e) => handleCustomColorChange(activeMode, "text", e.target.value)}
                />
              </div>
            </div>
          </div>
        )}

        {/* Language Selection */}
        <div className="setting-row">
          <div className="setting-info">
            <strong>{t("settings.language")}</strong>
            <span className="setting-description">{t("settings.languageDescription")}</span>
          </div>
          <select
            value={language}
            onChange={(e) => setLanguage(e.target.value as typeof language)}
            aria-label={t("language.label")}
          >
            <option value="zh-CN">简体中文</option>
            <option value="zh-TW">繁體中文</option>
            <option value="en">English</option>
            <option value="ja">日本語</option>
            <option value="ko">한국어</option>
          </select>
        </div>
      </section>

      {/* Provider Section */}
      <section className="settings-card panel">
        <div className="settings-heading">
          <div className="heading-text">
            <span className="eyebrow">{t("settings.provider")}</span>
            <h3>{t("settings.providerTitle")}</h3>
            <p className="muted">{t("settings.providerDescription")}</p>
          </div>
          <span
            className={`connection-badge ${
              provider?.cloud_account || provider?.byok_config || provider?.has_saved_byok ? "connected" : ""
            }`}
          >
            {provider?.cloud_account
              ? t("settings.cloudConnected")
              : (provider?.byok_config || provider?.has_saved_byok)
              ? t("settings.byokConnected")
              : t("settings.notConnected")}
          </span>
        </div>

        <div className="provider-option">
          <div className="provider-info">
            <strong>{t("settings.cloudName")}</strong>
            <span className="setting-description">{t("settings.cloudCopy")}</span>
          </div>
          <button className="button secondary" onClick={() => void signIn()} disabled={busy}>
            {busy ? t("settings.working") : t("settings.signIn")}
          </button>
        </div>

        <div className="provider-option">
          <div className="provider-info">
            <strong>{t("settings.byokName")}</strong>
            <span className="setting-description">{t("settings.byokCopy")}</span>
          </div>
          <div className="byok-form">
            <input
              value={baseUrl}
              onChange={(e) => setBaseUrl(e.target.value)}
              placeholder={t("settings.baseUrl")}
            />
            <input
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={t("settings.model")}
            />
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder={
                provider?.has_saved_byok ? t("settings.savedKey") : t("settings.apiKey")
              }
            />
            <button className="button primary" onClick={() => void saveByok()} disabled={busy}>
              {busy ? t("settings.saving") : t("settings.saveByok")}
            </button>
          </div>
        </div>

        {(provider?.cloud_account || provider?.byok_config || provider?.has_saved_byok) && (
          <div className="setting-footer-actions">
            <button className="button danger outline" onClick={() => void disconnect()} disabled={busy}>
              {t("settings.disconnect")}
            </button>
          </div>
        )}
      </section>

      {/* Workspace & Data Management */}
      <section className="settings-card panel">
        <div className="settings-heading">
          <div className="heading-text">
            <span className="eyebrow">{t("settings.workspaceActions")}</span>
            <h3>{t("brand.localWorkspace")}</h3>
            {workspacePath && <p className="muted">{t("settings.pathLabel", { path: workspacePath })}</p>}
          </div>
        </div>
        <div className="setting-row">
          <div className="setting-info">
            <strong>{t("settings.backupTitle")}</strong>
            <span className="setting-description">{t("settings.backupDescription")}</span>
          </div>
          <div className="button-group">
            <button className="button secondary" onClick={() => void handleImportBackup()}>
              {t("top.importBackup")}
            </button>
            <button className="button secondary" onClick={() => void handleExportBackup()}>
              {t("top.exportBackup")}
            </button>
          </div>
        </div>
      </section>

      {/* Privacy Section */}
      <section className="settings-card panel">
        <div className="settings-heading">
          <div className="heading-text">
            <span className="eyebrow">{t("settings.privacy")}</span>
            <h3>{t("settings.privacyTitle")}</h3>
            <p className="muted">{t("settings.privacyCopy")}</p>
          </div>
        </div>
      </section>
    </div>
  );
}
