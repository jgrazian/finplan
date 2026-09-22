/** Shown where a screen has nothing to draw, in place of an empty table. */
export function EmptyState({ title, detail }: { title: string; detail: string }) {
  return (
    <div style={{ padding: "40px 24px", maxWidth: 520 }}>
      <h4 style={{ margin: "0 0 6px" }}>{title}</h4>
      <p
        style={{
          margin: 0,
          fontSize: 13,
          lineHeight: 1.5,
          color: "color-mix(in srgb, var(--color-text) 58%, transparent)",
        }}
      >
        {detail}
      </p>
    </div>
  );
}
