"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter, useSearchParams } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogFooter,
    DialogHeader,
    DialogTitle,
    DialogTrigger,
} from "@/components/ui/dialog";
import { Play, Pencil, Settings, History, ArrowLeft, Loader2, CheckCircle2 } from "lucide-react";
import { SavedSimulation, AggregatedResult, SimulationRunRecord } from "@/lib/types";
import { getSimulation, runSimulation, listSimulationRuns, getSimulationRun } from "@/lib/api";
import { ResultsDashboard } from "./results-dashboard";
import { formatDistanceToNow } from "date-fns";

interface SimulationDetailProps {
    simulationId: string;
}

export function SimulationDetail({ simulationId }: SimulationDetailProps) {
    const router = useRouter();
    const searchParams = useSearchParams();
    const [simulation, setSimulation] = React.useState<SavedSimulation | null>(null);
    const [result, setResult] = React.useState<AggregatedResult | null>(null);
    const [runs, setRuns] = React.useState<SimulationRunRecord[]>([]);
    const [isLoading, setIsLoading] = React.useState(true);
    const [isRunning, setIsRunning] = React.useState(false);
    const [error, setError] = React.useState<string | null>(null);
    const [iterations, setIterations] = React.useState(100);
    const [showRunDialog, setShowRunDialog] = React.useState(false);
    const [selectedRunId, setSelectedRunId] = React.useState<string | null>(null);

    const fetchSimulation = React.useCallback(async () => {
        try {
            setIsLoading(true);
            const [simData, runsData] = await Promise.all([
                getSimulation(simulationId),
                listSimulationRuns(simulationId),
            ]);
            setSimulation(simData);
            setRuns(runsData);
            setError(null);

            // Check for run ID in URL query parameter
            const runIdFromUrl = searchParams.get("run");
            if (runIdFromUrl && runsData.some((r: SimulationRunRecord) => r.id === runIdFromUrl)) {
                const runResult = await getSimulationRun(runIdFromUrl);
                setResult(runResult);
                setSelectedRunId(runIdFromUrl);
            }
        } catch (err) {
            setError("Failed to load simulation");
            console.error(err);
        } finally {
            setIsLoading(false);
        }
    }, [simulationId, searchParams]);

    React.useEffect(() => {
        fetchSimulation();
    }, [fetchSimulation]);

    const handleRun = async () => {
        setShowRunDialog(false);
        setIsRunning(true);
        try {
            const runResult = await runSimulation(simulationId, iterations);
            setResult(runResult);
            setSelectedRunId(null); // Clear selected run since this is a new run
            // Update URL to remove run parameter
            router.replace(`/simulations/${simulationId}`, { scroll: false });
            // Refresh runs list
            const runsData = await listSimulationRuns(simulationId);
            setRuns(runsData);
        } catch (err) {
            console.error("Failed to run simulation:", err);
            setError("Failed to run simulation");
        } finally {
            setIsRunning(false);
        }
    };

    const loadPreviousRun = async (runId: string) => {
        try {
            const runResult = await getSimulationRun(runId);
            setResult(runResult);
            setSelectedRunId(runId);
            // Update URL with run ID
            router.replace(`/simulations/${simulationId}?run=${runId}`, { scroll: false });
        } catch (err) {
            console.error("Failed to load run:", err);
        }
    };

    if (isLoading) {
        return (
            <Card>
                <CardContent className="flex items-center justify-center py-10">
                    <div className="text-muted-foreground">Loading simulation...</div>
                </CardContent>
            </Card>
        );
    }

    if (error || !simulation) {
        return (
            <Card>
                <CardContent className="flex flex-col items-center justify-center py-10">
                    <p className="text-destructive mb-4">{error || "Simulation not found"}</p>
                    <Button onClick={() => router.push("/simulations")} variant="outline">
                        Back to Simulations
                    </Button>
                </CardContent>
            </Card>
        );
    }

    const formatCurrency = (value: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            notation: "compact",
        }).format(value);

    const totalAccountValue = simulation.parameters.accounts.reduce(
        (sum, acc) => sum + acc.assets.reduce((s, a) => s + a.initial_value, 0),
        0
    );

    return (
        <div className="space-y-6">
            {/* Header */}
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-4">
                    <Button variant="ghost" size="icon" asChild>
                        <Link href="/simulations">
                            <ArrowLeft className="h-4 w-4" />
                        </Link>
                    </Button>
                    <div>
                        <h1 className="text-2xl font-bold">{simulation.name}</h1>
                        {simulation.description && (
                            <p className="text-muted-foreground">{simulation.description}</p>
                        )}
                    </div>
                </div>
                <div className="flex gap-2">
                    <Button variant="outline" asChild>
                        <Link href={`/simulations/${simulationId}/edit`}>
                            <Pencil className="mr-2 h-4 w-4" />
                            Edit
                        </Link>
                    </Button>
                    <Dialog open={showRunDialog} onOpenChange={setShowRunDialog}>
                        <DialogTrigger asChild>
                            <Button disabled={isRunning}>
                                {isRunning ? (
                                    <>
                                        <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                                        Running...
                                    </>
                                ) : (
                                    <>
                                        <Play className="mr-2 h-4 w-4" />
                                        Run Simulation
                                    </>
                                )}
                            </Button>
                        </DialogTrigger>
                        <DialogContent>
                            <DialogHeader>
                                <DialogTitle>Run Simulation</DialogTitle>
                                <DialogDescription>
                                    Configure and run a Monte Carlo simulation with your parameters.
                                </DialogDescription>
                            </DialogHeader>
                            <div className="space-y-4 py-4">
                                <div className="space-y-2">
                                    <Label htmlFor="iterations">Number of Iterations</Label>
                                    <Input
                                        id="iterations"
                                        type="number"
                                        min={10}
                                        max={1000}
                                        value={iterations}
                                        onChange={(e) => setIterations(parseInt(e.target.value) || 100)}
                                    />
                                    <p className="text-xs text-muted-foreground">
                                        More iterations provide more accurate results but take longer. Recommended: 100-500.
                                    </p>
                                </div>
                            </div>
                            <DialogFooter>
                                <Button variant="outline" onClick={() => setShowRunDialog(false)}>
                                    Cancel
                                </Button>
                                <Button onClick={handleRun}>
                                    <Play className="mr-2 h-4 w-4" />
                                    Run
                                </Button>
                            </DialogFooter>
                        </DialogContent>
                    </Dialog>
                </div>
            </div>

            {/* Summary Cards */}
            <div className="grid gap-4 md:grid-cols-4">
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">
                            Duration
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{simulation.parameters.duration_years} years</p>
                    </CardContent>
                </Card>
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">
                            Starting Portfolio
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{formatCurrency(totalAccountValue)}</p>
                    </CardContent>
                </Card>
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Accounts</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{simulation.parameters.accounts.length}</p>
                    </CardContent>
                </Card>
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">
                            Previous Runs
                        </CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{runs.length}</p>
                    </CardContent>
                </Card>
            </div>

            {/* Main Content */}
            <Tabs defaultValue={result ? "results" : "overview"}>
                <TabsList>
                    <TabsTrigger value="overview">
                        <Settings className="mr-2 h-4 w-4" />
                        Overview
                    </TabsTrigger>
                    <TabsTrigger value="results" disabled={!result && runs.length === 0}>
                        <Play className="mr-2 h-4 w-4" />
                        Results
                    </TabsTrigger>
                    <TabsTrigger value="history">
                        <History className="mr-2 h-4 w-4" />
                        History
                    </TabsTrigger>
                </TabsList>

                <TabsContent value="overview" className="mt-6">
                    <div className="grid gap-6 md:grid-cols-2">
                        {/* Accounts */}
                        <Card>
                            <CardHeader>
                                <CardTitle className="text-base">Accounts</CardTitle>
                                <CardDescription>Your financial accounts</CardDescription>
                            </CardHeader>
                            <CardContent>
                                {simulation.parameters.accounts.length === 0 ? (
                                    <p className="text-muted-foreground text-sm">No accounts configured</p>
                                ) : (
                                    <div className="space-y-3">
                                        {simulation.parameters.accounts.map((acc) => (
                                            <div
                                                key={acc.account_id}
                                                className="flex items-center justify-between border-b pb-2 last:border-0"
                                            >
                                                <div>
                                                    <p className="font-medium">Account #{acc.account_id}</p>
                                                    <Badge variant="secondary" className="text-xs">
                                                        {acc.account_type}
                                                    </Badge>
                                                </div>
                                                <p className="font-mono">
                                                    {formatCurrency(
                                                        acc.assets.reduce((s, a) => s + a.initial_value, 0)
                                                    )}
                                                </p>
                                            </div>
                                        ))}
                                    </div>
                                )}
                            </CardContent>
                        </Card>

                        {/* Cash Flows */}
                        <Card>
                            <CardHeader>
                                <CardTitle className="text-base">Cash Flows</CardTitle>
                                <CardDescription>Income and expenses</CardDescription>
                            </CardHeader>
                            <CardContent>
                                {simulation.parameters.cash_flows.length === 0 ? (
                                    <p className="text-muted-foreground text-sm">No cash flows configured</p>
                                ) : (
                                    <div className="space-y-3">
                                        {simulation.parameters.cash_flows.map((cf) => (
                                            <div
                                                key={cf.cash_flow_id}
                                                className="flex items-center justify-between border-b pb-2 last:border-0"
                                            >
                                                <div>
                                                    <p className="font-medium">
                                                        {cf.source === "External" ? "💰 Income" : "💸 Expense"} #{cf.cash_flow_id}
                                                    </p>
                                                    <Badge variant="outline" className="text-xs">
                                                        {cf.repeats}
                                                    </Badge>
                                                </div>
                                                <p className="font-mono">{formatCurrency(cf.amount)}</p>
                                            </div>
                                        ))}
                                    </div>
                                )}
                            </CardContent>
                        </Card>

                        {/* Profiles */}
                        <Card>
                            <CardHeader>
                                <CardTitle className="text-base">Profiles</CardTitle>
                                <CardDescription>Inflation and return assumptions</CardDescription>
                            </CardHeader>
                            <CardContent className="space-y-4">
                                <div>
                                    <p className="text-sm font-medium">Inflation Profile</p>
                                    <p className="text-sm text-muted-foreground">
                                        {simulation.parameters.inflation_profile === "None"
                                            ? "None"
                                            : typeof simulation.parameters.inflation_profile === "object" &&
                                                "Fixed" in simulation.parameters.inflation_profile
                                                ? `Fixed ${(simulation.parameters.inflation_profile.Fixed * 100).toFixed(1)}%`
                                                : typeof simulation.parameters.inflation_profile === "object" &&
                                                    "Normal" in simulation.parameters.inflation_profile
                                                    ? `Normal (μ=${(simulation.parameters.inflation_profile.Normal.mean * 100).toFixed(1)}%, σ=${(simulation.parameters.inflation_profile.Normal.std_dev * 100).toFixed(1)}%)`
                                                    : "Unknown"}
                                    </p>
                                </div>
                                <div>
                                    <p className="text-sm font-medium">Return Profile</p>
                                    <p className="text-sm text-muted-foreground">
                                        {simulation.parameters.return_profiles.length === 0
                                            ? "None"
                                            : simulation.parameters.return_profiles[0] === "None"
                                                ? "None"
                                                : typeof simulation.parameters.return_profiles[0] === "object" &&
                                                    "Fixed" in simulation.parameters.return_profiles[0]
                                                    ? `Fixed ${(simulation.parameters.return_profiles[0].Fixed * 100).toFixed(1)}%`
                                                    : typeof simulation.parameters.return_profiles[0] === "object" &&
                                                        "Normal" in simulation.parameters.return_profiles[0]
                                                        ? `Normal (μ=${(simulation.parameters.return_profiles[0].Normal.mean * 100).toFixed(1)}%, σ=${(simulation.parameters.return_profiles[0].Normal.std_dev * 100).toFixed(1)}%)`
                                                        : "Unknown"}
                                    </p>
                                </div>
                            </CardContent>
                        </Card>

                        {/* Spending Targets */}
                        <Card>
                            <CardHeader>
                                <CardTitle className="text-base">Spending Targets</CardTitle>
                                <CardDescription>Retirement withdrawal goals</CardDescription>
                            </CardHeader>
                            <CardContent>
                                {simulation.parameters.spending_targets.length === 0 ? (
                                    <p className="text-muted-foreground text-sm">No spending targets configured</p>
                                ) : (
                                    <div className="space-y-3">
                                        {simulation.parameters.spending_targets.map((st) => (
                                            <div
                                                key={st.spending_target_id}
                                                className="flex items-center justify-between border-b pb-2 last:border-0"
                                            >
                                                <div>
                                                    <p className="font-medium">Target #{st.spending_target_id}</p>
                                                    <Badge variant="outline" className="text-xs">
                                                        {st.repeats}
                                                    </Badge>
                                                </div>
                                                <p className="font-mono">{formatCurrency(st.amount)}</p>
                                            </div>
                                        ))}
                                    </div>
                                )}
                            </CardContent>
                        </Card>
                    </div>
                </TabsContent>

                <TabsContent value="results" className="mt-6">
                    {result ? (
                        <div className="space-y-4">
                            {selectedRunId && (
                                <div className="flex items-center gap-2 p-3 bg-blue-50 dark:bg-blue-950 border border-blue-200 dark:border-blue-800 rounded-lg">
                                    <History className="h-4 w-4 text-blue-600 dark:text-blue-400" />
                                    <span className="text-sm text-blue-700 dark:text-blue-300">
                                        Viewing historical run from {formatDistanceToNow(new Date(runs.find(r => r.id === selectedRunId)?.ran_at || ''), { addSuffix: true })}
                                    </span>
                                    <Button
                                        variant="ghost"
                                        size="sm"
                                        className="ml-auto text-blue-600 dark:text-blue-400 hover:text-blue-700 dark:hover:text-blue-300"
                                        onClick={() => setShowRunDialog(true)}
                                    >
                                        Run New
                                    </Button>
                                </div>
                            )}
                            <ResultsDashboard result={result} simulationName={simulation.name} />
                        </div>
                    ) : runs.length > 0 ? (
                        <Card>
                            <CardContent className="flex flex-col items-center justify-center py-10">
                                <p className="text-muted-foreground mb-4">
                                    Select a previous run from the History tab or run a new simulation.
                                </p>
                                <Button onClick={() => setShowRunDialog(true)}>
                                    <Play className="mr-2 h-4 w-4" />
                                    Run New Simulation
                                </Button>
                            </CardContent>
                        </Card>
                    ) : (
                        <Card>
                            <CardContent className="flex flex-col items-center justify-center py-10">
                                <p className="text-muted-foreground mb-4">
                                    No simulation results yet. Run your first simulation to see projections.
                                </p>
                                <Button onClick={() => setShowRunDialog(true)}>
                                    <Play className="mr-2 h-4 w-4" />
                                    Run Simulation
                                </Button>
                            </CardContent>
                        </Card>
                    )}
                </TabsContent>

                <TabsContent value="history" className="mt-6">
                    <Card>
                        <CardHeader>
                            <CardTitle>Run History</CardTitle>
                            <CardDescription>Previous simulation runs and their results</CardDescription>
                        </CardHeader>
                        <CardContent>
                            {runs.length === 0 ? (
                                <p className="text-muted-foreground text-sm text-center py-6">
                                    No previous runs. Run your first simulation to see history.
                                </p>
                            ) : (
                                <div className="space-y-2">
                                    {runs.map((run) => {
                                        const isSelected = run.id === selectedRunId;
                                        return (
                                            <div
                                                key={run.id}
                                                className={`flex items-center justify-between p-3 rounded-lg border cursor-pointer transition-colors ${isSelected
                                                        ? 'bg-primary/10 border-primary'
                                                        : 'hover:bg-muted/50'
                                                    }`}
                                                onClick={() => loadPreviousRun(run.id)}
                                            >
                                                <div className="flex items-center gap-3">
                                                    {isSelected && (
                                                        <CheckCircle2 className="h-4 w-4 text-primary" />
                                                    )}
                                                    <div>
                                                        <p className="font-medium">
                                                            {formatDistanceToNow(new Date(run.ran_at), { addSuffix: true })}
                                                        </p>
                                                        <p className="text-sm text-muted-foreground">
                                                            {run.iterations} iterations
                                                        </p>
                                                    </div>
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    {isSelected && (
                                                        <Badge variant="secondary" className="text-xs">
                                                            Viewing
                                                        </Badge>
                                                    )}
                                                    <Button variant="ghost" size="sm">
                                                        {isSelected ? 'Reload' : 'View Results'}
                                                    </Button>
                                                </div>
                                            </div>
                                        );
                                    })}
                                </div>
                            )}
                        </CardContent>
                    </Card>
                </TabsContent>
            </Tabs>
        </div>
    );
}
