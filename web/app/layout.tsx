import type { Metadata } from "next";
import { IBM_Plex_Mono, IBM_Plex_Sans, Source_Serif_4 } from "next/font/google";
import { PwaRegistrar } from "@/components/local/PwaRegistrar";
import { APPEARANCE_KEY, DARK_STYLE_KEY } from "@/lib/theme";
import "./globals.css";

/**
 * The three Ledger faces, self-hosted by next/font at build time: no request
 * to Google at runtime and no flash of the fallback while one is in flight.
 * Plex Sans and Plex Mono are one superfamily, so tickers and units set in the
 * mono belong to the same system as the controls around them. Each lands as a
 * CSS variable on <html>, which design-system.css reads into
 * --font-display / --font-body / --font-mono.
 */
const display = Source_Serif_4({
  subsets: ["latin"],
  style: ["normal", "italic"],
  axes: ["opsz"],
  variable: "--font-source-serif",
});
const sans = IBM_Plex_Sans({ subsets: ["latin"], variable: "--font-plex-sans" });
const mono = IBM_Plex_Mono({
  subsets: ["latin"],
  weight: ["400", "500"],
  variable: "--font-plex-mono",
});

export const metadata: Metadata = {
  title: "FinPlan",
  description: "Monte Carlo retirement simulation",
};

/**
 * Stamp the palette before the first paint.
 *
 * Mode and dark style are kept in local storage (lib/theme), and React only
 * reads them after hydration; without this a dark device would paint light
 * and then swap under the reader. So they are replayed here, inline and
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
  if(localStorage.getItem(${JSON.stringify(DARK_STYLE_KEY)})==="midnight")r.dataset.ground="midnight";
}catch(e){}`;

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    // The script above writes attributes the server did not render, which is
    // the whole point of it; React is told not to call that a mismatch.
    <html
      lang="en"
      className={`${display.variable} ${sans.variable} ${mono.variable}`}
      suppressHydrationWarning
    >
      <head>
        <script dangerouslySetInnerHTML={{ __html: PRE_PAINT }} />
      </head>
      <body>
        {children}
        <PwaRegistrar />
      </body>
    </html>
  );
}
