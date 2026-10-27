import {
  Award,
  CalendarCheck,
  CalendarDays,
  History,
  House,
  IndianRupee,
  type LucideIcon,
  Megaphone,
  Receipt,
} from "lucide-react";

export type NavItem = {
  title: string;
  href: string;
  icon: LucideIcon;
  /** Has its own page. The rest show a "not built yet" page until Phase 3B builds them. */
  built: boolean;
  /** Shown only to users with this permission (or any of these). */
  permission?: string | string[];
};

export type NavSection = { title: string; items: NavItem[] };

// The demo's sidebar: the core student journey. The full product's other
// modules (activities, exam forms, the accounts office, support, profile and
// more) aren't part of this demo; see the README.
export const NAV: NavSection[] = [
  // Untitled: Home sits above the sections.
  { title: "", items: [{ title: "Home", href: "/dashboard", icon: House, built: true }] },
  {
    title: "Academic",
    items: [
      { title: "Attendance", href: "/academic/attendance", icon: CalendarCheck, built: true, permission: ["attendance:read", "attendance:mark"] },
      { title: "Calendar", href: "/academic/calendar", icon: CalendarDays, built: true },
    ],
  },
  {
    title: "Examination",
    items: [
      { title: "Results", href: "/examination/results", icon: Award, built: true, permission: ["results:read", "marks:enter", "results:publish"] },
    ],
  },
  {
    title: "Fees",
    items: [
      { title: "Payable", href: "/fees/payable", icon: IndianRupee, built: true, permission: "fees:read" },
      { title: "Receipts", href: "/fees/receipts", icon: Receipt, built: true, permission: "fees:read" },
    ],
  },
  {
    title: "Account",
    items: [{ title: "Notices", href: "/notices", icon: Megaphone, built: true }],
  },
  {
    title: "Administration",
    items: [{ title: "Logs", href: "/logs", icon: History, built: true, permission: "audit:read_all" }],
  },
];

/** The sidebar for someone with these permissions: empty sections disappear. */
export function navFor(permissions: string[]): NavSection[] {
  const visible = (item: NavItem) => !item.permission || [item.permission].flat().some((p) => permissions.includes(p));
  return NAV.map((section) => ({ ...section, items: section.items.filter(visible) })).filter(
    (section) => section.items.length > 0,
  );
}

export function findNavItem(pathname: string): NavItem | undefined {
  return NAV.flatMap((section) => section.items).find((item) => item.href === pathname);
}
