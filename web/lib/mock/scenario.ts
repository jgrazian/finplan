import type { Scenario } from "@/lib/types";

export const MOCK_SCENARIOS: Scenario[] = [
  { id: "retirement-base", name: "retirement-base", dirty: true },
  { id: "aggressive", name: "aggressive", dirty: false },
  { id: "conservative", name: "conservative", dirty: false },
];

export const MOCK_USER = { initials: "DA", name: "Dana Alvarez" };
