"use client";

import { useEffect, useRef, useState } from "react";

/**
 * A figure that rolls from where it was to where it is now, once, when the
 * target changes — a run landing reads as the number arriving rather than
 * blinking. The first value shows at once (rolling up from zero on every page
 * load would be noise), and reduced motion skips the roll entirely.
 */
export function useCountUp(target: number, durationMs = 650): number {
  const [shown, setShown] = useState(target);
  const from = useRef(target);

  useEffect(() => {
    const start = from.current;
    from.current = target;
    if (start === target || !Number.isFinite(start) || !Number.isFinite(target)) {
      setShown(target);
      return;
    }
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setShown(target);
      return;
    }
    const began = performance.now();
    let frame = requestAnimationFrame(function tick(now) {
      const t = Math.min(1, (now - began) / durationMs);
      const eased = 1 - Math.pow(1 - t, 3);
      setShown(start + (target - start) * eased);
      if (t < 1) frame = requestAnimationFrame(tick);
    });
    return () => cancelAnimationFrame(frame);
  }, [target, durationMs]);

  return shown;
}
