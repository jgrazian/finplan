import {
    SimulationParameters,
    SavedSimulation,
    SimulationListItem,
    AggregatedResult,
    SimulationRunRecord,
} from "./types";

const API_BASE = process.env.NEXT_PUBLIC_API_URL || "http://localhost:3001";

async function handleResponse<T>(response: Response): Promise<T> {
    if (!response.ok) {
        const text = await response.text();
        throw new Error(`API Error: ${response.status} - ${text}`);
    }
    return response.json();
}

// ============================================================================
// Simulation CRUD
// ============================================================================

export async function listSimulations(): Promise<SimulationListItem[]> {
    const response = await fetch(`${API_BASE}/api/simulations`);
    return handleResponse(response);
}

export async function getSimulation(id: string): Promise<SavedSimulation> {
    const response = await fetch(`${API_BASE}/api/simulations/${id}`);
    return handleResponse(response);
}

export async function createSimulation(data: {
    name: string;
    description?: string;
    parameters: SimulationParameters;
}): Promise<SavedSimulation> {
    const response = await fetch(`${API_BASE}/api/simulations`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data),
    });
    return handleResponse(response);
}

export async function updateSimulation(
    id: string,
    data: {
        name?: string;
        description?: string;
        parameters?: SimulationParameters;
    }
): Promise<SavedSimulation> {
    const response = await fetch(`${API_BASE}/api/simulations/${id}`, {
        method: "PUT",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(data),
    });
    return handleResponse(response);
}

export async function deleteSimulation(id: string): Promise<void> {
    const response = await fetch(`${API_BASE}/api/simulations/${id}`, {
        method: "DELETE",
    });
    if (!response.ok) {
        throw new Error(`Failed to delete simulation: ${response.status}`);
    }
}

// ============================================================================
// Run Simulations
// ============================================================================

export async function runSimulation(
    id: string,
    iterations: number = 100
): Promise<AggregatedResult> {
    const response = await fetch(`${API_BASE}/api/simulations/${id}/run`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ iterations }),
    });
    return handleResponse(response);
}

export async function runSimulationDirect(
    params: SimulationParameters
): Promise<AggregatedResult> {
    const response = await fetch(`${API_BASE}/api/simulate`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(params),
    });
    return handleResponse(response);
}

// ============================================================================
// Simulation Runs History
// ============================================================================

export async function listSimulationRuns(
    simulationId: string
): Promise<SimulationRunRecord[]> {
    const response = await fetch(
        `${API_BASE}/api/simulations/${simulationId}/runs`
    );
    return handleResponse(response);
}

export async function getSimulationRun(runId: string): Promise<AggregatedResult> {
    const response = await fetch(`${API_BASE}/api/runs/${runId}`);
    return handleResponse(response);
}
