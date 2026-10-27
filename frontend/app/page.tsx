import { redirect } from "next/navigation";

import { signInAs } from "@/app/sign-in/actions";
import { DotGrid } from "@/components/dot-grid";
import { ThemeToggle } from "@/components/theme-toggle";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { getInstitution, type PublicInstitution } from "@/lib/api";
import { PERSONAS } from "@/lib/personas";
import { getSession } from "@/lib/session";
import { cn } from "@/lib/utils";

export const dynamic = "force-dynamic";

const SIGN_IN_MESSAGES: Record<string, string> = {
  "no-account": "That person isn't in the database yet. Has the seed run? (docker compose logs seed)",
  failed: "Sign-in didn't complete. Is the API running? (docker compose ps)",
};


/**
 * The demo's sign-in: pick a person. (The full product signs in through Auth0,
 * with Google or a password.) Signed-in users go straight to the dashboard,
 * unless they're here because sign-in went wrong.
 */
export default async function SignInPage({ searchParams }: { searchParams: Promise<{ signin?: string; returnTo?: string }> }) {
  const { signin, returnTo } = await searchParams;
  const session = await getSession();
  if (session && !signin) redirect("/dashboard");
  const problem = signin ? SIGN_IN_MESSAGES[signin] : undefined;
  const institution = await getInstitution();

  return (
    <div className="bg-background grid min-h-svh lg:grid-cols-[1fr_minmax(28rem,36rem)]">
      <aside className="bg-sidebar relative hidden items-center justify-center overflow-hidden border-r p-10 lg:flex">
        <DotGrid className="absolute inset-0" />
        <UniversityMark institution={institution} />
      </aside>

      <main className="relative flex flex-col items-center justify-center px-4 py-12 sm:px-8">
        <div className="absolute top-3 right-3">
          <ThemeToggle />
        </div>
        <div className="w-full max-w-md">
          <div className="mb-8 lg:hidden">
            <UniversityMark institution={institution} compact />
          </div>
          <h1 className="text-2xl font-semibold tracking-tight">Sign in to the demo</h1>
          <p className="text-muted-foreground mt-1 mb-6 text-sm">
            Pick a person to see Acadex as them. {institution.name} and everyone in it are made up.
          </p>

          {problem && (
            <Alert variant="destructive" className="mb-6">
              <AlertTitle>Couldn&apos;t sign you in</AlertTitle>
              <AlertDescription>{problem}</AlertDescription>
            </Alert>
          )}
          {session ? (
            <Button asChild className="w-full" variant="outline">
              {/* A server route, not client navigation. */}
              {/* eslint-disable-next-line @next/next/no-html-link-for-pages */}
              <a href="/auth/logout">Sign out</a>
            </Button>
          ) : (
            <ul className="grid gap-2">
              {PERSONAS.map((p) => (
                <li key={p.email}>
                  <form action={signInAs}>
                    <input type="hidden" name="email" value={p.email} />
                    <input type="hidden" name="returnTo" value={returnTo ?? "/dashboard"} />
                    <button
                      type="submit"
                      className="hover:bg-accent focus-visible:ring-ring w-full rounded-lg border px-4 py-3 text-left transition-colors outline-none focus-visible:ring-2"
                    >
                      <span className="flex items-baseline justify-between gap-3">
                        <span className="font-medium">{p.name}</span>
                        <span className="text-muted-foreground shrink-0 text-xs">{p.role}</span>
                      </span>
                      <span className="text-muted-foreground mt-0.5 block text-sm">{p.story}</span>
                    </button>
                  </form>
                </li>
              ))}
            </ul>
          )}

        </div>
      </main>
    </div>
  );
}

/**
 * The university's own logo, raised out of the pane like a phone's camera
 * bump, or its name when it has none.
 */
function UniversityMark({ institution, compact = false }: { institution: PublicInstitution; compact?: boolean }) {
  if (!institution.logo_updated_at) {
    return (
      <p className={cn("relative font-semibold tracking-tight", compact ? "text-lg" : "max-w-md text-center text-3xl")}>
        {institution.name}
      </p>
    );
  }
  return (
    <div
      className={cn(
        "camera-bump relative w-full",
        compact ? "max-w-60 p-3 [--bump-depth:0.35] [--bump-surface:var(--background)]" : "max-w-lg p-9",
      )}
    >
      {/* eslint-disable-next-line @next/next/no-img-element -- the API serves it; there's nothing to optimise */}
      <img
        src={`/institution/logo?v=${encodeURIComponent(institution.logo_updated_at)}`}
        alt={institution.name}
        // Logos are made for white: on dark, invert and turn the hues back, so navy reads light blue.
        className="h-auto w-full dark:[filter:invert(1)_hue-rotate(180deg)_saturate(1.3)]"
      />
    </div>
  );
}
