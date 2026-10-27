import type { Metadata, Viewport } from "next";
import localFont from "next/font/local";
import { headers } from "next/headers";

import { CsrfFetch } from "@/components/csrf-fetch";
import { ServiceWorker } from "@/components/service-worker";
import { ThemeProvider } from "@/components/theme-provider";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";

import "./globals.css";

// Font files come from npm (@fontsource-variable), so neither the build nor
// the browser contacts Google; next/font serves them from our own origin.
const inter = localFont({
  variable: "--font-inter",
  src: "../node_modules/@fontsource-variable/inter/files/inter-latin-wght-normal.woff2",
  weight: "100 900",
});

const geistMono = localFont({
  variable: "--font-geist-mono",
  src: "../node_modules/@fontsource-variable/geist-mono/files/geist-mono-latin-wght-normal.woff2",
  weight: "100 900",
});

export const metadata: Metadata = {
  title: "Acadex",
  description: "College ERP for students, faculty and administration",
  appleWebApp: { capable: true, title: "Acadex", statusBarStyle: "default" },
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#ffffff" },
    { media: "(prefers-color-scheme: dark)", color: "#0a0a0a" },
  ],
};

export default async function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  // Set by the middleware for this request (the CSP's nonce, and whether the
  // CSRF cookie has its HTTPS-only name).
  const requestHeaders = await headers();
  const nonce = requestHeaders.get("x-nonce") ?? undefined;
  const https = requestHeaders.get("x-forwarded-proto") === "https";
  return (
    // next-themes sets the theme class before hydration.
    // Font variables go on <html>, where globals.css applies font-sans.
    <html lang="en" className={`${inter.variable} ${geistMono.variable}`} suppressHydrationWarning>
      <body className="antialiased">
        <ServiceWorker />
        <CsrfFetch cookieName={https ? "__Host-acadex-csrf" : "acadex-csrf"} />
        <ThemeProvider nonce={nonce}>
          <TooltipProvider>
            {children}
            <Toaster />
          </TooltipProvider>
        </ThemeProvider>
      </body>
    </html>
  );
}
