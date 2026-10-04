/**
 * What the Review tab does to a note, in one shape for both homes.
 *
 * On a cloud plan the notes live on the server and these are its routes
 * (`api.suggestions`); on a plan in this browser they are the local review
 * (`runtime.review`, rule-based notes written on the device). The screen calls
 * `ReviewActions` and never learns which. Previewing a path simulates it against
 * the reviewed run, which the local review does not do, so it is absent there
 * and the screen does not offer it.
 *
 * Types only: nothing here reaches `fetch` or `http`.
 */
import type {
  AppliedSuggestion,
  ApplySuggestion,
  DismissSuggestion,
  PreviewSuggestion,
  Suggestion,
} from "../api/suggestions.ts";
import type { Preview } from "../api/generated/Preview.ts";
import type { LocalReviewApi } from "../engine/review.ts";

export interface ReviewActions {
  apply(id: number, body: ApplySuggestion): Promise<AppliedSuggestion>;
  /** Absent where a path cannot be simulated (the local review). */
  preview?: (id: number, body: PreviewSuggestion) => Promise<Preview>;
  dismiss(id: number, body: DismissSuggestion): Promise<Suggestion>;
  reopen(id: number): Promise<Suggestion>;
}

/** The actions of the local review of plan `scenarioId`. */
export function localReviewActions(review: LocalReviewApi, scenarioId: number): ReviewActions {
  return {
    apply: (id, body) => review.apply(scenarioId, id, body),
    dismiss: (id, body) => review.dismiss(scenarioId, id, body.as),
    reopen: (id) => review.reopen(scenarioId, id),
  };
}
