"use client";

import { useState } from "react";
import { Button } from "@/components/ui";
import { download } from "@/components/account/download";
import { api } from "@/lib/api/client";
import { guestNotice } from "@/lib/view/guest";
import { useGuest } from "./GuestContext";

/**
 * The slim notice on every screen for a guest: when the plan is deleted and
 * how to keep it. A guest has no Account screen, so the whole-account export
 * lives here too, right beside the notice that makes it worth using.
 */
export function GuestBanner() {
  const { retentionDays, openSignUp } = useGuest();
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string>();
  const notice = guestNotice(retentionDays);

  const exportAll = async () => {
    setExporting(true);
    setError(undefined);
    try {
      download(
        `finplan-${new Date().toISOString().slice(0, 10)}.json`,
        await api.archives.exportAll(),
      );
    } catch (err) {
      setError(err instanceof Error ? err.message : "Could not export.");
    } finally {
      setExporting(false);
    }
  };

  return (
    <p
      role="status"
      style={{
        margin: 0,
        padding: "8px 16px",
        fontSize: 12,
        borderBottom: "1px solid var(--color-divider)",
        display: "flex",
        flexWrap: "wrap",
        alignItems: "center",
        gap: "4px 12px",
      }}
    >
      <span>
        {notice.lead}
        <button type="button" className="linkbtn" onClick={openSignUp}>
          {notice.action}
        </button>
        {notice.tail}
      </span>
      <Button variant="ghost" disabled={exporting} onClick={() => void exportAll()}>
        {exporting ? "…" : "Export plans"}
      </Button>
      {error && <span style={{ color: "var(--color-accent-700)" }}>{error}</span>}
    </p>
  );
}
