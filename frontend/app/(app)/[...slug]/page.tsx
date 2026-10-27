import { notFound } from "next/navigation";

import { PageHeader } from "@/components/page-header";
import { Card, CardContent } from "@/components/ui/card";
import { findNavItem } from "@/lib/nav";

// Sidebar entries whose page isn't built yet land here (demo-plan.md, Phase
// 3B). Built pages have their own routes, which take precedence.
export default async function PlannedPage({ params }: { params: Promise<{ slug: string[] }> }) {
  const { slug } = await params;
  const item = findNavItem(`/${slug.join("/")}`);
  if (!item) notFound();

  return (
    <>
      <PageHeader title={item.title} />
      <Card>
        <CardContent className="flex items-center gap-4 py-6">
          <item.icon className="text-muted-foreground size-8 shrink-0" aria-hidden />
          <p className="text-muted-foreground">
            Not built yet: this page is part of the remaining work in demo-plan.md, Phase 3B.
          </p>
        </CardContent>
      </Card>
    </>
  );
}
