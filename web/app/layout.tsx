import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "FinPlan",
  description: "Monte Carlo retirement simulation",
};

export default function RootLayout({
  children,
}: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
