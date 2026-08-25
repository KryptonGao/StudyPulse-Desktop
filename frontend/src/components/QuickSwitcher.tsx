import { useEffect, useMemo, useRef, useState } from "react";
import { getAllSwitcherItems, type Page, type SwitcherItem } from "../app/navigation";
import { useI18n } from "../i18n";

const sidebarIconSprite = new URL("../assets/sidebar-icons.svg", import.meta.url).href;

function Icon({ name }: { name: string }) {
  return (
    <svg className="switcher-icon" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
      <use href={`${sidebarIconSprite}#${name}`} />
    </svg>
  );
}

export function QuickSwitcher({
  isOpen,
  onClose,
  onSelectPage,
}: {
  isOpen: boolean;
  onClose: () => void;
  onSelectPage: (page: Page) => void;
}) {
  if (!isOpen) return null;
  return <QuickSwitcherModal onClose={onClose} onSelectPage={onSelectPage} />;
}

function QuickSwitcherModal({
  onClose,
  onSelectPage,
}: {
  onClose: () => void;
  onSelectPage: (page: Page) => void;
}) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const allItems = useMemo(() => getAllSwitcherItems(), []);

  const filteredItems: SwitcherItem[] = useMemo(() => {
    if (!query.trim()) return allItems;
    const q = query.trim().toLowerCase();
    return allItems.filter((item) => {
      const label = t(item.labelKey).toLowerCase();
      const wsLabel = t(item.workspaceLabelKey).toLowerCase();
      const id = item.id.toLowerCase();
      return label.includes(q) || wsLabel.includes(q) || id.includes(q);
    });
  }, [allItems, query, t]);

  const activeIndex = Math.min(selectedIndex, Math.max(0, filteredItems.length - 1));

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // Keep active item in view
  useEffect(() => {
    if (!listRef.current) return;
    const activeEl = listRef.current.children[activeIndex] as HTMLElement | undefined;
    if (activeEl) {
      activeEl.scrollIntoView({ block: "nearest" });
    }
  }, [activeIndex]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((idx) => (filteredItems.length ? (idx + 1) % filteredItems.length : 0));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((idx) => (filteredItems.length ? (idx - 1 + filteredItems.length) % filteredItems.length : 0));
    } else if (e.key === "Enter" && filteredItems.length > 0) {
      e.preventDefault();
      const selected = filteredItems[activeIndex];
      if (selected) {
        onSelectPage(selected.page);
        onClose();
      }
    }
  };

  return (
    <div className="modal-backdrop switcher-backdrop" onClick={onClose} role="presentation">
      <div
        className="quick-switcher-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={t("switcher.placeholder")}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={handleKeyDown}
      >
        <div className="switcher-search-box">
          <svg className="switcher-search-icon" viewBox="0 0 24 24" aria-hidden="true">
            <use href={`${sidebarIconSprite}#search`} />
          </svg>
          <input
            ref={inputRef}
            type="text"
            className="switcher-input"
            placeholder={t("switcher.placeholder")}
            value={query}
            onChange={(e) => {
              setQuery(e.target.value);
              setSelectedIndex(0);
            }}
            aria-autocomplete="list"
            aria-controls="switcher-results"
          />
          <kbd className="switcher-kbd">ESC</kbd>
        </div>

        <div className="switcher-results" id="switcher-results" ref={listRef} role="listbox">
          {filteredItems.length > 0 ? (
            filteredItems.map((item, index) => {
              const isSelected = index === activeIndex;
              return (
                <div
                  key={item.id}
                  className={`switcher-item ${isSelected ? "selected" : ""}`}
                  role="option"
                  aria-selected={isSelected}
                  onClick={() => {
                    onSelectPage(item.page);
                    onClose();
                  }}
                  onMouseEnter={() => setSelectedIndex(index)}
                >
                  <span className="switcher-item-icon">
                    <Icon name={item.icon} />
                  </span>
                  <div className="switcher-item-info">
                    <strong className="switcher-item-title">{t(item.labelKey)}</strong>
                    <span className="switcher-item-badge">{t(item.workspaceLabelKey)}</span>
                  </div>
                  {isSelected && <span className="switcher-item-arrow">↵</span>}
                </div>
              );
            })
          ) : (
            <div className="switcher-empty">{t("switcher.noResults")}</div>
          )}
        </div>

        <div className="switcher-footer">
          <span>{t("switcher.hint")}</span>
        </div>
      </div>
    </div>
  );
}
