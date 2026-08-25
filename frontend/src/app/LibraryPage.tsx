import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { chooseSourceFiles, core } from "../lib/core";
import { useI18n } from "../i18n";
import {
  EmptyState,
  ErrorCard,
  PageLoading,
  SectionHeader,
} from "../components/UIComponents";
import { useToast } from "../components/Toast";

export function LibraryPage() {
  const { t } = useI18n();
  const { showToast } = useToast();
  const queryClient = useQueryClient();

  const query = useQuery({ queryKey: ["library"], queryFn: core.library });
  const [search, setSearch] = useState("");

  const results = useQuery({
    queryKey: ["library-search", search],
    queryFn: () => core.searchLibrary(search),
    enabled: search.trim().length > 1,
  });

  async function importFiles() {
    try {
      const selected = await chooseSourceFiles(t("dialog.addSources"));
      for (const path of selected) {
        try {
          await core.importLibraryFile(path);
        } catch (error) {
          showToast(error instanceof Error ? error.message : String(error), "error");
        }
      }
      await queryClient.invalidateQueries({ queryKey: ["library"] });
      showToast(t("common.saved"), "success");
    } catch (error) {
      showToast(error instanceof Error ? error.message : String(error), "error");
    }
  }

  if (query.isLoading) return <PageLoading />;
  if (query.error) return <ErrorCard error={query.error} />;

  const files = query.data ?? [];

  return (
    <div className="page-content">
      <SectionHeader
        title={t("library.title")}
        description={t("library.description")}
        action={
          <button className="button primary" onClick={() => void importFiles()}>
            {t("library.add")}
          </button>
        }
      />

      <div className="search-bar">
        <span>⌕</span>
        <input
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder={t("library.searchPlaceholder")}
        />
      </div>

      {search.trim().length > 1 && (
        <section className="panel search-results">
          <SectionHeader title={t("library.results")} />
          {results.data?.length ? (
            results.data.map((match) => (
              <div
                className="search-result"
                key={`${match.relative_path}-${match.line_number}`}
              >
                <strong>{match.relative_path}</strong>
                <span>{t("library.line", { line: match.line_number ?? "—" })}</span>
                <p>{match.snippet}</p>
              </div>
            ))
          ) : (
            <p className="muted" style={{ padding: "0 23px 18px" }}>
              {t("library.noMatches")}
            </p>
          )}
        </section>
      )}

      <div className="file-grid">
        {files
          .filter((file) => !file.is_directory)
          .map((file) => (
            <div className="file-card" key={file.relative_path}>
              <span className="file-icon">
                {file.relative_path.endsWith(".md") ? "M↓" : "TXT"}
              </span>
              <div className="file-info">
                <strong>{file.relative_path.split("/").at(-1)}</strong>
                <span>
                  {file.relative_path} · {Math.ceil(file.size_bytes / 1024)} KB
                </span>
              </div>
            </div>
          ))}
      </div>

      {!files.length && (
        <div className="panel">
          <EmptyState title={t("library.empty")} copy={t("library.emptyCopy")} />
        </div>
      )}
    </div>
  );
}
