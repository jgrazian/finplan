import assert from "node:assert/strict";
import test from "node:test";
import { addYears, yearsBetween } from "../lib/view/format.ts";

test("plan end preserves leap dates and clamps only in non-leap years", () => {
  assert.equal(addYears("2024-02-29", 4), "2028-02-29");
  assert.equal(addYears("2024-02-29", 1), "2025-02-28");
  assert.equal(addYears("2096-02-29", 4), "2100-02-28");
  assert.equal(addYears("1996-02-29", 4), "2000-02-29");
});

test("end age uses the actual birthday rather than calendar-year subtraction", () => {
  const end = addYears("2026-09-13", 30);
  assert.equal(yearsBetween("1981-01-01", end), 75);
  assert.equal(yearsBetween("1981-10-01", end), 74);
});
