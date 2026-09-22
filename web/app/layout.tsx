import type { Metadata } from "next";
import { APPEARANCE_KEY } from "@/lib/theme";
import "./globals.css";

export const metadata: Metadata = {
  title: "FinPlan",
  description: "Monte Carlo retirement simulation",
};

/**
 * Stamp the palette before the first paint.
 *
 * The account's real choice arrives with the session, a fetch later; without
 * this the page would paint light-blue and then swap under the reader. So the
 * last applied palette is cached locally and replayed here, inline and
 * blocking, which is the one place code can run before anything is drawn.
 *
 * Everything it touches is optional — no storage, bad JSON, an old shape — and
 * the catch leaves the stylesheet's own defaults standing.
 */
const PRE_PAINT = `try{
  var a=JSON.parse(localStorage.getItem(${JSON.stringify(APPEARANCE_KEY)})||"{}");
  var m=a.mode==="light"||a.mode==="dark"?a.mode
    :matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light";
  var r=document.documentElement;
  r.dataset.theme=m;
  r.dataset.accent=a.accent==="green"||a.accent==="purple"?a.accent:"blue";
}catch(e){}`;

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    // The script above writes attributes the server did not render, which is
    // the whole point of it; React is told not to call that a mismatch.
    <html lang="en" suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: PRE_PAINT }} />
      </head>
      <body>{children}</body>
    </html>
  );
}
