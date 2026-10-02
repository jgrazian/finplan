import type { Metadata } from "next";
import { Geist_Mono, Instrument_Sans, Newsreader } from "next/font/google";
import { APPEARANCE_KEY, DARK_STYLE_KEY } from "@/lib/theme";
import "./globals.css";

/**
 * The three Almanac faces, self-hosted by next/font at build time: no request
 * to Google at runtime and no flash of the fallback while one is in flight.
 * Each lands as a CSS variable on <html>, which design-system.css reads into
 * --font-display / --font-body / --font-mono.
 */
const display = Newsreader({
  subsets: ["latin"],
  style: ["normal", "italic"],
  axes: ["opsz"],
  variable: "--font-newsreader",
});
const sans = Instrument_Sans({ subsets: ["latin"], axes: ["wdth"], variable: "--font-instrument" });
const mono = Geist_Mono({ subsets: ["latin"], variable: "--font-geist-mono" });

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
      <body>{children}</body>
    </html>
  );
}
