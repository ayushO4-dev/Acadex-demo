import type { Role } from "@/lib/api";

export const ROLE_LABELS: Record<Role, string> = {
  student: "Student",
  faculty: "Faculty",
  hod: "Head of department",
  exam_cell: "Exam cell",
  accounts: "Accounts",
  support_agent: "Support",
  admin: "Administrator",
  super_admin: "Super administrator",
  platform_admin: "Platform operator",
};
