/**
 * What a guest sees (spec 17): the retention notice and the iteration upsell.
 * Pure, so the wording is tested without a render.
 */

/** The iteration count a free account runs at, named in the upsell. */
export const FREE_ITERATIONS = 1_000;

export const SIGN_UP_FOR_ITERATIONS = `Sign up free for ${FREE_ITERATIONS.toLocaleString("en-US")} iterations`;

export const CREATE_ACCOUNT = "Create a free account";

/** The banner, in three parts so the middle one can be the link that opens Sign up. */
export function guestNotice(retentionDays: number | null | undefined): {
  lead: string;
  action: string;
  tail: string;
} {
  const lead =
    retentionDays == null
      ? "Guest plan — deleted if you stop visiting. "
      : `Guest plan — deleted after ${retentionDays} ${retentionDays === 1 ? "day" : "days"} without a visit. `;
  return { lead, action: CREATE_ACCOUNT, tail: " to keep it." };
}

/** Where the guest's archive is adopted: the prefix every imported plan gets. */
export const GUEST_PLAN_PREFIX = "Guest – ";
