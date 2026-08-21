// Application route ownership, including route-level code splitting for heavy
// secondary surfaces so the main research workspace stays quick to start.
import { lazy, Suspense } from "react";
import { Loader2 } from "lucide-react";
import { createBrowserRouter, Navigate, type RouteObject } from "react-router-dom";
import { AppShell } from "./layout/AppShell";
import { SessionPage } from "./routes/SessionPage";
import { LiveSessionPage } from "./routes/LiveSessionPage";
import { SkillsPage } from "./routes/SkillsPage";
import { NotebooksPage } from "./routes/NotebooksPage";
import { FilesPage } from "./routes/FilesPage";
import { RunsPage } from "./routes/RunsPage";
import { ProjectsPage } from "./routes/ProjectsPage";
import { HistoryPage } from "./routes/HistoryPage";
import { NotFound } from "./routes/NotFound";

const SettingsPage = lazy(() =>
  import("./routes/SettingsPage").then((module) => ({ default: module.SettingsPage })),
);

function SettingsRoute() {
  return (
    <Suspense
      fallback={
        <div className="flex h-full items-center justify-center" role="status" aria-busy="true">
          <Loader2 className="animate-spin text-muted" size={18} aria-hidden />
        </div>
      }
    >
      <SettingsPage />
    </Suspense>
  );
}

export const routes: RouteObject[] = [
  {
    path: "/",
    element: <AppShell />,
    children: [
      { index: true, element: <Navigate to="/live" replace /> },
      { path: "live", element: <LiveSessionPage /> },
      { path: "live/:sessionId", element: <LiveSessionPage /> },
      { path: "example/:sessionId", element: <SessionPage /> },
      { path: "skills", element: <SkillsPage /> },
      { path: "notebooks", element: <NotebooksPage /> },
      { path: "files", element: <FilesPage /> },
      { path: "runs", element: <RunsPage /> },
      { path: "projects", element: <ProjectsPage /> },
      { path: "history", element: <HistoryPage /> },
      { path: "settings", element: <SettingsRoute /> },
      { path: "settings/:section", element: <SettingsRoute /> },
      { path: "*", element: <NotFound /> },
    ],
  },
];

export const router = createBrowserRouter(routes);
