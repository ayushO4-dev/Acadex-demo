"use client";

import { ThemeProvider as NextThemesProvider } from "next-themes";

// Light/dark from the system setting (architecture.md §4.2).
// Its inline script (the theme before hydration) carries the request's CSP nonce.
export function ThemeProvider({ children, nonce }: { children: React.ReactNode; nonce?: string }) {
  return (
    <NextThemesProvider attribute="class" defaultTheme="system" enableSystem disableTransitionOnChange nonce={nonce}>
      {children}
    </NextThemesProvider>
  );
}
