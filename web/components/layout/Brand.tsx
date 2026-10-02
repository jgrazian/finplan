/**
 * The FinPlan wordmark: a fan of three rising paths from one starting point —
 * the picture a Monte Carlo run draws — beside the name, its second half set
 * in the serif's italic.
 */
export function BrandMark({ size = 26 }: { size?: number }) {
  return (
    <svg
      className="brand-mark"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      aria-hidden="true"
    >
      <rect x="0.75" y="0.75" width="22.5" height="22.5" rx="6.5" fill="currentColor" fillOpacity=".12" />
      <path d="M5 18 Q 14 17.5, 19 5.5" stroke="currentColor" strokeWidth="2" strokeLinecap="round" />
      <path d="M5 18 Q 14 17, 19 10.5" stroke="currentColor" strokeOpacity=".62" strokeWidth="2" strokeLinecap="round" />
      <path d="M5 18 Q 14 17.8, 19 15" stroke="currentColor" strokeOpacity=".34" strokeWidth="2" strokeLinecap="round" />
      <circle cx="5" cy="18" r="1.9" fill="currentColor" />
    </svg>
  );
}

export function Wordmark() {
  return (
    <span className="brand-word">
      Fin<em>plan</em>
    </span>
  );
}
