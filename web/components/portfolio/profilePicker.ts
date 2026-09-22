import type { DropdownOption } from "@/components/ui";
import type { Profile } from "@/lib/api/types";

/**
 * The menu row standing in for "no profile".
 *
 * `null` is what the wire carries and what a draft holds, but a row has to name
 * a value — so the pickers trade on this sentinel and convert at the edge. Held
 * here rather than in each of the three, which had drifted into three copies of
 * the same constant and the same label.
 */
export const UNMAPPED = -1;

/**
 * The return-profile menu: the library, then the way out of it.
 *
 * Unmapped goes last. The profiles are the real answers, and an asset held flat
 * at 0% is the escape hatch — a menu that opens on it reads as if that were the
 * usual choice.
 */
export function profileOptions(profiles: Profile[]): DropdownOption<number>[] {
  return [
    ...profiles.map((p) => ({ value: p.id, label: p.name })),
    { value: UNMAPPED, label: "Unmapped — held flat at 0%" },
  ];
}
