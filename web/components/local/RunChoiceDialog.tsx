"use client";

import { Button, Dialog } from "@/components/ui";
import { api } from "@/lib/api/client";
import { useAsync } from "@/lib/hooks/useAsync";
import { type RunChoice, formatDuration, offloadAvailability } from "@/lib/local/estimate";

type Offer = Extract<RunChoice, { kind: "offer-offload" }>;

/**
 * "Run on FinPlan servers (about N s)" next to "Run here anyway", for a run
 * this device says is too big for it. The offload is explicit and per run, and
 * this is where it says the plan is sent: to the server for this run, and not
 * stored there.
 */
export function RunChoiceDialog({
  offer,
  account,
  onOffload,
  onRunHere,
  onSignIn,
  onClose,
}: {
  offer: Offer;
  /** Signed in and not a guest: the only callers the server will take. */
  account: boolean;
  onOffload: () => void;
  onRunHere: () => void;
  onSignIn: () => void;
  onClose: () => void;
}) {
  // Asked only for an account; nobody else has a budget to read.
  const budget = useAsync(async () => (account ? api.compute.budget() : undefined), [account]);
  const checking = account && budget.loading && budget.data === undefined;
  const availability = offloadAvailability(account, budget.data);

  return (
    <Dialog
      title="This run may take a while here"
      width={520}
      onClose={onClose}
      onSubmit={onRunHere}
      footer={
        <div style={{ display: "flex", flexWrap: "wrap", gap: 8, justifyContent: "flex-end", marginTop: 4 }}>
          <Button type="button" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit">Run here anyway</Button>
          <Button
            type="button"
            variant="primary"
            disabled={checking || !availability.available}
            onClick={onOffload}
          >
            Run on FinPlan servers (about {formatDuration(offer.serverSeconds)})
          </Button>
        </div>
      }
    >
      <p style={{ margin: 0, fontSize: 13.5, lineHeight: 1.5 }}>
        This device expects about <strong style={{ fontWeight: 500 }}>{formatDuration(offer.seconds)}</strong>{" "}
        for this run
        {offer.reason === "constrained" ? ", and it is a device with limited power or memory" : ""}.
        You can run it here anyway, or have FinPlan&rsquo;s servers do it.
      </p>
      <p style={{ margin: 0, fontSize: 12.5, lineHeight: 1.5 }}>
        Running on the servers sends this plan to FinPlan for this run only. It is not stored
        there, and the results are saved back on this device.
      </p>
      {checking && (
        <p role="status" className="ns-mut" style={{ margin: 0, fontSize: 12.5 }}>
          Checking your server run budget…
        </p>
      )}
      {!checking && !availability.available && (
        <p role="status" style={{ margin: 0, fontSize: 12.5, lineHeight: 1.5 }}>
          {availability.reason}{" "}
          {availability.signIn && (
            <button type="button" className="linkbtn" onClick={onSignIn}>
              Sign in
            </button>
          )}
        </p>
      )}
    </Dialog>
  );
}
