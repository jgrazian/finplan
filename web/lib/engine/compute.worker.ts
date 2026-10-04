/**
 * A compute worker: its own WASM instance, running the batches and analyses the
 * store worker's pool hands it (see `compute.ts` for the protocol).
 *
 * Created with `new Worker(new URL("./compute.worker.ts", import.meta.url), {
 * type: "module" })` by the store worker. The `.wasm` loads here, lazily, when
 * the first compute worker is made: a page that never runs anything never
 * fetches it for compute.
 */
import init, * as wasm from "./pkg/finplan_wasm.js";
import { type ComputeHost, type ComputeReply, type ComputeRequest, createComputeHost } from "./compute.ts";
import type { Engine } from "./engine.ts";

interface WorkerScope {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
}
const scope = self as unknown as WorkerScope;

const ready: Promise<ComputeHost> = init({
  module_or_path: new URL("./pkg/finplan_wasm_bg.wasm", import.meta.url),
}).then(() => createComputeHost(wasm as unknown as Engine));

scope.addEventListener("message", (event) => {
  const request = event.data as ComputeRequest;
  ready.then(
    (host) => host.handle(request, (reply: ComputeReply) => scope.postMessage(reply)),
    (error: unknown) => {
      // The module would not load: every request fails, so the pool reports why.
      scope.postMessage({
        id: request.id,
        ok: false,
        error: JSON.stringify({
          status: 500,
          code: "internal",
          message: `the engine could not start: ${error instanceof Error ? error.message : String(error)}`,
        }),
      } satisfies ComputeReply);
    },
  );
});
