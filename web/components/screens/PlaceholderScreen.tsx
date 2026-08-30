/** Stands in for the tabs not yet built, so the header's nav stays honest. */
export function PlaceholderScreen({ label }: { label: string }) {
  return (
    <div
      style={{
        padding: "26px 20px 30px",
        fontSize: 12,
        color: "color-mix(in srgb, var(--color-text) 42%, transparent)",
      }}
    >
      {label} screen — not built yet.
    </div>
  );
}
