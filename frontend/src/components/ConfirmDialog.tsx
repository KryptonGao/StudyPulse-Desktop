/* eslint-disable react-refresh/only-export-components */
import { createContext, useContext, useState, useCallback, useEffect, useRef, type ReactNode } from "react";
import { useI18n } from "../i18n";

export interface ConfirmOptions {
  title?: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  isDestructive?: boolean;
}

interface ConfirmContextValue {
  confirm: (options: ConfirmOptions | string) => Promise<boolean>;
}

const ConfirmContext = createContext<ConfirmContextValue | null>(null);

export function ConfirmDialogProvider({ children }: { children: ReactNode }) {
  const { t } = useI18n();
  const [current, setCurrent] = useState<{
    options: ConfirmOptions;
    resolve: (value: boolean) => void;
  } | null>(null);

  const confirmButtonRef = useRef<HTMLButtonElement>(null);

  const confirm = useCallback((opts: ConfirmOptions | string): Promise<boolean> => {
    const options: ConfirmOptions = typeof opts === "string" ? { message: opts } : opts;
    return new Promise((resolve) => {
      setCurrent({ options, resolve });
    });
  }, []);

  useEffect(() => {
    if (current) {
      confirmButtonRef.current?.focus();
    }
  }, [current]);

  const handleClose = useCallback((result: boolean) => {
    if (current) {
      current.resolve(result);
      setCurrent(null);
    }
  }, [current]);

  useEffect(() => {
    if (!current) return;
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        handleClose(false);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [current, handleClose]);

  return (
    <ConfirmContext.Provider value={{ confirm }}>
      {children}
      {current && (
        <div className="modal-backdrop" onClick={() => handleClose(false)} role="presentation">
          <div
            className="modal-dialog"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="confirm-dialog-title"
            aria-describedby="confirm-dialog-message"
            onClick={(e) => e.stopPropagation()}
          >
            {current.options.title && (
              <h3 id="confirm-dialog-title" className="modal-title">
                {current.options.title}
              </h3>
            )}
            <p id="confirm-dialog-message" className="modal-message">
              {current.options.message}
            </p>
            <div className="modal-actions">
              <button
                className="button subtle"
                onClick={() => handleClose(false)}
              >
                {current.options.cancelText ?? t("common.cancel")}
              </button>
              <button
                ref={confirmButtonRef}
                className={`button ${current.options.isDestructive ? "danger" : "primary"}`}
                onClick={() => handleClose(true)}
              >
                {current.options.confirmText ?? t("common.confirm")}
              </button>
            </div>
          </div>
        </div>
      )}
    </ConfirmContext.Provider>
  );
}

export function useConfirm(): (options: ConfirmOptions | string) => Promise<boolean> {
  const context = useContext(ConfirmContext);
  if (!context) {
    return (options: ConfirmOptions | string) => {
      const msg = typeof options === "string" ? options : `${options.title ? options.title + "\n\n" : ""}${options.message}`;
      return Promise.resolve(window.confirm(msg));
    };
  }
  return context.confirm;
}
