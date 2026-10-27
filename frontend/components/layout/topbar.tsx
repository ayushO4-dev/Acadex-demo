"use client";

import { LogOut, Menu } from "lucide-react";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import { NotificationsBell } from "@/components/layout/notifications-bell";
import { ThemeToggle } from "@/components/theme-toggle";
import { Button } from "@/components/ui/button";
import { useSidebar } from "@/components/ui/sidebar";

/**
 * Left to right: the demo label · notifications · theme · sign out
 * (architecture.md §8.2). It shares the sidebar's surface; the logo and the
 * collapse button live in the sidebar, except on phones, where the sidebar
 * is a sheet and this bar opens it.
 */
export function Topbar({ unread, role }: { unread: number; role: string }) {
  return (
    <header className="bg-sidebar grid h-14 shrink-0 grid-cols-[auto_1fr_auto_auto_auto] items-center gap-2 px-3 md:grid-cols-[1fr_auto_auto_auto]">
      <MenuButton />

      <div className="mx-auto w-full max-w-xl px-2 text-center">
        <span className="text-muted-foreground bg-sidebar-accent rounded-full px-3 py-1 text-xs">
          Demo · every person and record here is made up
        </span>
      </div>

      {role === "platform_admin" ? <span /> : <NotificationsBell initialUnread={unread} />}

      <ThemeToggle />

      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button variant="ghost" size="icon" aria-label="Sign out">
            <LogOut />
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Sign out of Acadex?</AlertDialogTitle>
            <AlertDialogDescription>You&apos;ll need to sign in again to continue.</AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction asChild>
              {/* A plain link: /auth/logout is a route handler that clears the session
                  cookie, so it needs a full navigation, not <Link>. */}
              {/* eslint-disable-next-line @next/next/no-html-link-for-pages */}
              <a href="/auth/logout">Sign out</a>
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </header>
  );
}

/** Phones only: the sidebar is a slide-over there, opened from here. */
function MenuButton() {
  const { setOpenMobile } = useSidebar();
  return (
    <Button variant="ghost" size="icon" className="md:hidden" aria-label="Open the menu" onClick={() => setOpenMobile(true)}>
      <Menu />
    </Button>
  );
}
