import { Blueprint } from "@/components/ui";
import type { EventEffect } from "@/lib/types";

/** One effect as a drawn object: its kind, then how it is configured. */
export function EffectCard({ effect }: { effect: EventEffect }) {
  return (
    <Blueprint style={{ padding: "7px 9px" }}>
      <div style={{ fontFamily: "var(--font-heading)", fontWeight: 600, fontSize: 13 }}>
        {effect.kind}
      </div>
      <div
        style={{
          fontSize: 12,
          color: "color-mix(in srgb, var(--color-text) 62%, transparent)",
        }}
      >
        {effect.detail}
      </div>
    </Blueprint>
  );
}
