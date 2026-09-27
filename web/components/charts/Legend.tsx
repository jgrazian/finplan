/** Swatch legend for the stacked account bands. */
export function Legend({
  items,
}: {
  items: Array<{ label: string; color: string }>;
}) {
  return (
    <div
      style={{
        display: "flex",
        flexWrap: "wrap",
        gap: "6px 14px",
        fontSize: 11,
        color: "color-mix(in srgb, var(--color-text) 60%, transparent)",
      }}
    >
      {items.map((item) => (
        <span
          key={item.label}
          style={{ display: "flex", alignItems: "center", gap: 5, whiteSpace: "nowrap" }}
        >
          <i
            style={{ width: 10, height: 10, background: item.color, display: "block" }}
            aria-hidden
          />
          {item.label}
        </span>
      ))}
    </div>
  );
}
