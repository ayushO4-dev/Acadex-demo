"use client";

import { PanelLeft } from "lucide-react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { Fragment } from "react";

import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarHeader,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarSeparator,
  useSidebar,
} from "@/components/ui/sidebar";
import type { Me } from "@/lib/api";
import { navFor } from "@/lib/nav";

/**
 * The logo heads the sidebar, the navigation follows, and the collapse
 * button is at the bottom. Expanded: section titles with icon-and-label
 * items. Collapsed: titles give way to line dividers and items show icons
 * only, with the label as a tooltip (architecture.md §8.3). Below `md` it
 * becomes a slide-over sheet. No border: it's one surface with the topbar.
 */
export function AppSidebar({ me }: { me: Pick<Me, "display_name" | "role" | "permissions" | "email"> }) {
  const pathname = usePathname();
  const { toggleSidebar, state, isMobile, setOpenMobile } = useSidebar();
  // On phones the sidebar is a slide-over: close it once a page is chosen.
  const close = () => {
    if (isMobile) setOpenMobile(false);
  };

  return (
    <Sidebar collapsible="icon" className="group-data-[side=left]:border-r-0">
      {/* Every icon keeps its x position while collapsing (see SidebarMenuButton);
          labels fade and are clipped by the narrowing width. */}
      <SidebarHeader className="h-14 justify-center overflow-hidden">
        <Link href="/dashboard" className="flex items-center gap-2 px-1 whitespace-nowrap" onClick={close}>
          {/* eslint-disable-next-line @next/next/no-img-element -- a static 64px icon */}
          <img src="/icons/acadex-64.png" alt="" width={32} height={32} className="size-8 shrink-0 rounded-md" />
          <span className="font-semibold transition-opacity duration-200 group-data-[collapsible=icon]:opacity-0">Acadex</span>
        </Link>
      </SidebarHeader>
      <SidebarContent>
        {navFor(me.permissions).map((section, index) => (
          <Fragment key={section.title}>
            {index > 0 && <SidebarSeparator className="mx-auto hidden data-horizontal:w-6 group-data-[collapsible=icon]:block" />}
            <SidebarGroup className={section.title ? undefined : "pb-0 group-data-[collapsible=icon]:pb-2"}>
              {section.title && <SidebarGroupLabel className="overflow-hidden whitespace-nowrap uppercase tracking-wider group-data-[collapsible=icon]:pointer-events-none">{section.title}</SidebarGroupLabel>}
              <SidebarGroupContent>
                <SidebarMenu>
                  {section.items.map((item) => {
                    const active = pathname === item.href || pathname.startsWith(`${item.href}/`);
                    return (
                      <SidebarMenuItem key={item.href}>
                        <SidebarMenuButton
                          asChild
                          isActive={active}
                          tooltip={item.title}
                          className="data-[active=true]:bg-sidebar-active data-[active=true]:text-sidebar-active-foreground data-[active=true]:hover:bg-sidebar-active data-[active=true]:font-semibold"
                        >
                          <Link href={item.href} aria-current={active ? "page" : undefined} onClick={close}>
                            <item.icon aria-hidden />
                            <span>{item.title}</span>
                          </Link>
                        </SidebarMenuButton>
                      </SidebarMenuItem>
                    );
                  })}
                </SidebarMenu>
              </SidebarGroupContent>
            </SidebarGroup>
          </Fragment>
        ))}
      </SidebarContent>
      <SidebarFooter className="overflow-hidden">
        <div className="flex items-center gap-1 whitespace-nowrap">
          <SidebarMenuButton
            hidden={isMobile}
            onClick={toggleSidebar}
            tooltip={state === "expanded" ? "Collapse sidebar" : "Expand sidebar"}
            aria-label={state === "expanded" ? "Collapse sidebar" : "Expand sidebar"}
            className="w-auto shrink-0"
          >
            <PanelLeft aria-hidden />
          </SidebarMenuButton>
          <span
            title={me.email ?? undefined}
            className="bg-sidebar-accent text-sidebar-accent-foreground min-w-0 truncate rounded-full px-3 py-1 text-xs transition-opacity duration-200 group-data-[collapsible=icon]:opacity-0"
          >
            {me.email?.split("@")[0] ?? me.display_name}
          </span>
        </div>
      </SidebarFooter>
    </Sidebar>
  );
}
