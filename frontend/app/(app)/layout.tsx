import { cookies } from "next/headers";
import { redirect } from "next/navigation";

import { ApiProblem } from "@/components/api-problem";
import { AppSidebar } from "@/components/layout/app-sidebar";
import { AcknowledgeGate } from "@/components/notices/acknowledge-gate";
import { Topbar } from "@/components/layout/topbar";
import { SidebarProvider } from "@/components/ui/sidebar";
import { asUser, getMe } from "@/lib/acadex";
import { ApiError, type Me, type Notice } from "@/lib/api";
import { getSession } from "@/lib/session";

// The signed-in shell: every page under (app) gets the topbar and sidebar.
export default async function AppLayout({ children }: { children: React.ReactNode }) {
  if (!(await getSession())) redirect("/");

  let me: Me;
  try {
    me = await getMe();
  } catch (err) {
    if (err instanceof ApiError && err.kind === "no-account") redirect("/?signin=no-account");
    return (
      <main className="mx-auto max-w-xl p-6 py-12">
        <ApiProblem error={err} />
      </main>
    );
  }

  // Critical notices waiting for acknowledgement come before anything else.
  const awaiting =
    me.role === "platform_admin"
      ? []
      : await asUser<Notice[]>("/notices/awaiting-acknowledgement").catch(() => [] as Notice[]);

  // Read on the server so the first render has the right width — no flash.
  const sidebarOpen = (await cookies()).get("sidebar_state")?.value !== "false";

  return (
    // Sidebar and topbar are one surface (the sidebar colour); the page sits in a
    // panel whose top-left corner curves where the two meet it. The panel
    // scrolls on its own, so the curve stays put.
    // Collapsed, the sidebar is as wide as the topbar is tall (3.5rem).
    <SidebarProvider
      defaultOpen={sidebarOpen}
      className="bg-sidebar"
      style={{ "--sidebar-width": "13rem", "--sidebar-width-icon": "3.5rem" } as React.CSSProperties}
    >
      {/* First thing a keyboard reaches: past the sidebar and topbar (WCAG 2.4.1). */}
      <a
        href="#main"
        className="bg-background focus-visible:ring-ring sr-only z-50 rounded-md px-3 py-2 text-sm font-medium focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus-visible:ring-2"
      >
        Skip to content
      </a>
      <AppSidebar me={me} />
      <div className="flex h-svh min-w-0 flex-1 flex-col">
        <Topbar unread={me.unread_notices} role={me.role} />
        {/* The scrollbar's space is always reserved, so pages don't reflow when it appears. */}
        <main
          id="main"
          tabIndex={-1}
          className="bg-background flex-1 overflow-y-auto outline-none [scrollbar-gutter:stable] md:rounded-tl-2xl md:border-t md:border-l"
        >
          {/* Pages use the panel's full width. */}
          <div className="w-full p-4 md:p-6 lg:px-8">{children}</div>
        </main>
        <AcknowledgeGate notices={awaiting} />
      </div>
    </SidebarProvider>
  );
}
