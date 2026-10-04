/**
 * The stand-in for a signed-out visitor with local mode on (spec 19, phase 7).
 *
 * The workbench is built around a `UserResponse` (defaults for new plans, the
 * display name, the guest flag). A visitor has no account and so no row on the
 * server, but is not turned away: they get these defaults, marked `guest` so
 * every "needs an account" gate (AI, offload, the account screen) closes the
 * way it does for a guest. Nothing here is sent anywhere.
 */
import type { UserResponse } from "../api/types.ts";

export const LOCAL_VISITOR_ID = "local-visitor";

export const LOCAL_VISITOR: UserResponse = {
  id: LOCAL_VISITOR_ID,
  email: "",
  email_verified_at: null,
  display_name: null,
  birth_date: null,
  default_iterations: 2000,
  default_duration_years: 40,
  auto_run: false,
  default_plan_home: "local",
  created_at: "",
  guest: true,
};

export function isLocalVisitor(user: Pick<UserResponse, "id">): boolean {
  return user.id === LOCAL_VISITOR_ID;
}
