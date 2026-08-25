import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { core } from "../lib/core";
import { localizeEnum, useI18n, type Translate } from "../i18n";
import type { TimeInvestmentSubject } from "../types";
import {
  EmptyState,
  ErrorCard,
  formatDate,
  PageLoading,
  SectionHeader,
} from "../components/UIComponents";
import { useToast } from "../components/Toast";
import { useConfirm } from "../components/ConfirmDialog";

function themeLabel(t: Translate, value: string): string {
  return localizeEnum(t, "theme", value.toLowerCase());
}

export function InvestmentPage() {
  const { language, t } = useI18n();
  const { showToast } = useToast();
  const confirm = useConfirm();
  const queryClient = useQueryClient();

  const query = useQuery({ queryKey: ["investment"], queryFn: core.investmentSubjects });
  const [name, setName] = useState("");
  const [theme, setTheme] = useState("Ocean");

  const mutation = useMutation({
    mutationFn: core.upsertInvestmentSubject,
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ["investment"] });
      setName("");
      showToast(t("common.saved"), "success");
    },
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  const remove = useMutation({
    mutationFn: core.deleteInvestmentSubject,
    onSuccess: () => void queryClient.invalidateQueries({ queryKey: ["investment"] }),
    onError: (error) => showToast(error instanceof Error ? error.message : String(error), "error"),
  });

  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;

  const values = query.data ?? [];

  function saveSubject() {
    const trimmed = name.trim();
    if (!trimmed) {
      showToast(t("investment.validation"), "error");
      return;
    }
    const now = new Date().toISOString();
    const value: TimeInvestmentSubject = {
      id: crypto.randomUUID(),
      name: trimmed,
      symbol_name: "book.closed",
      theme,
      start_date: now,
      sort_order: values.length,
      created_at: now,
      is_archived: false,
      extra_json: "{}",
    };
    mutation.mutate(value);
  }

  const handleRemove = async (id: string, subjectName: string) => {
    try {
      const ok = await confirm({
        title: t("investment.remove"),
        message: `${subjectName}?`,
        isDestructive: true,
      });
      if (ok) remove.mutate(id);
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  };

  return (
    <div className="page-content">
      <SectionHeader
        title={t("investment.title")}
        description={t("investment.description")}
        action={
          <div className="inline-form">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") saveSubject();
              }}
              placeholder={t("investment.newSubject")}
            />
            <select value={theme} onChange={(e) => setTheme(e.target.value)}>
              <option value="Ocean">{t("theme.ocean")}</option>
              <option value="Coral">{t("theme.coral")}</option>
              <option value="Violet">{t("theme.violet")}</option>
              <option value="Sunshine">{t("theme.sunshine")}</option>
              <option value="Mint">{t("theme.mint")}</option>
            </select>
            <button
              className="button primary small"
              onClick={saveSubject}
              disabled={mutation.isPending}
            >
              {mutation.isPending ? t("tasks.saving") : t("investment.add")}
            </button>
          </div>
        }
      />

      {values.length ? (
        <div className="record-grid">
          {values.map((value) => (
            <div className="record-card" key={value.id}>
              <div className="record-index">#{value.sort_order + 1}</div>
              <h3>{value.name}</h3>
              <div className="record-field">
                <span>{t("investment.theme")}</span>
                <strong>{themeLabel(t, value.theme)}</strong>
              </div>
              <div className="record-field">
                <span>{t("investment.started")}</span>
                <strong>{formatDate(value.start_date, language)}</strong>
              </div>
              <button
                className="button subtle small"
                onClick={() => void handleRemove(value.id, value.name)}
                disabled={remove.isPending}
              >
                {t("investment.remove")}
              </button>
            </div>
          ))}
        </div>
      ) : (
        <div className="panel">
          <EmptyState title={t("investment.none")} copy={t("investment.noneCopy")} />
        </div>
      )}
    </div>
  );
}
