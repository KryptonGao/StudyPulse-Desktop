import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import App from "./app/App";
import { I18nProvider } from "./i18n";
import "./styles.css";

// One QueryClient is shared by every page so query keys and invalidation calls
// observe the same cache instead of creating page-local resource copies.
const queryClient = new QueryClient({
  defaultOptions: {
    // Short-lived data may refresh after a mutation, but window focus should
    // not unexpectedly trigger desktop filesystem reads.
    queries: { staleTime: 10_000, refetchOnWindowFocus: false, retry: 1 },
  },
});

// StrictMode catches render/effect assumptions during development. The query
// and i18n providers wrap App so every page receives the shared boundaries.
ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <I18nProvider>
        <App />
      </I18nProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
