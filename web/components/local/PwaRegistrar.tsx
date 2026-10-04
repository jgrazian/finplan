"use client";

import { useEffect } from "react";
import { useLocalMode } from "@/lib/local/useLocalMode";

/**
 * Registers the service worker that lets the app open offline (spec 19, phase
 * 5). Only in a production build, and only with local mode on: a deployment
 * that has not opted in installs nothing, and the dev server never has a
 * worker holding stale chunks.
 */
export function PwaRegistrar() {
  const localMode = useLocalMode();
  useEffect(() => {
    if (process.env.NODE_ENV !== "production" || !localMode) return;
    if (!("serviceWorker" in navigator)) return;
    void navigator.serviceWorker.register("/sw.js", { scope: "/" }).catch(() => undefined);
  }, [localMode]);
  return null;
}
