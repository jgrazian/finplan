"use client"

import { AppSidebar } from "@/components/app-sidebar"
import {
    Breadcrumb,
    BreadcrumbItem,
    BreadcrumbLink,
    BreadcrumbList,
    BreadcrumbPage,
    BreadcrumbSeparator,
} from "@/components/ui/breadcrumb"
import { Separator } from "@/components/ui/separator"
import {
    SidebarInset,
    SidebarProvider,
    SidebarTrigger,
} from "@/components/ui/sidebar"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Button } from "@/components/ui/button"
import Link from "next/link"
import { useEffect, useState } from "react"
import { listSimulations, listSimulationRuns } from "@/lib/api"
import { SimulationListItem, SimulationRunRecord } from "@/lib/types"
import { History, Play } from "lucide-react"
import { formatDistanceToNow } from "date-fns"

interface RunWithSimulation extends SimulationRunRecord {
    simulationName: string
}

export default function HistoryPage() {
    const [runs, setRuns] = useState<RunWithSimulation[]>([])
    const [loading, setLoading] = useState(true)

    useEffect(() => {
        async function load() {
            try {
                const simulations = await listSimulations()
                const allRuns: RunWithSimulation[] = []

                for (const sim of simulations) {
                    const simRuns = await listSimulationRuns(sim.id)
                    for (const run of simRuns) {
                        allRuns.push({ ...run, simulationName: sim.name })
                    }
                }

                // Sort by date descending
                allRuns.sort((a, b) => new Date(b.ran_at).getTime() - new Date(a.ran_at).getTime())
                setRuns(allRuns)
            } catch (err) {
                console.error(err)
            } finally {
                setLoading(false)
            }
        }
        load()
    }, [])

    return (
        <SidebarProvider>
            <AppSidebar />
            <SidebarInset>
                <header className="flex h-16 shrink-0 items-center gap-2 transition-[width,height] ease-linear group-has-data-[collapsible=icon]/sidebar-wrapper:h-12">
                    <div className="flex items-center gap-2 px-4">
                        <SidebarTrigger className="-ml-1" />
                        <Separator
                            orientation="vertical"
                            className="mr-2 data-[orientation=vertical]:h-4"
                        />
                        <Breadcrumb>
                            <BreadcrumbList>
                                <BreadcrumbItem className="hidden md:block">
                                    <BreadcrumbLink href="/">Dashboard</BreadcrumbLink>
                                </BreadcrumbItem>
                                <BreadcrumbSeparator className="hidden md:block" />
                                <BreadcrumbItem className="hidden md:block">
                                    <BreadcrumbLink href="/results">Results</BreadcrumbLink>
                                </BreadcrumbItem>
                                <BreadcrumbSeparator className="hidden md:block" />
                                <BreadcrumbItem>
                                    <BreadcrumbPage>Run History</BreadcrumbPage>
                                </BreadcrumbItem>
                            </BreadcrumbList>
                        </Breadcrumb>
                    </div>
                </header>
                <div className="flex flex-1 flex-col gap-6 p-6 pt-0">
                    <div className="flex flex-col gap-2">
                        <h1 className="text-3xl font-bold tracking-tight">Run History</h1>
                        <p className="text-muted-foreground">
                            View all past simulation runs across all simulations
                        </p>
                    </div>

                    {loading ? (
                        <Card>
                            <CardContent className="flex items-center justify-center py-10">
                                <div className="text-muted-foreground">Loading history...</div>
                            </CardContent>
                        </Card>
                    ) : runs.length === 0 ? (
                        <Card>
                            <CardContent className="flex flex-col items-center justify-center py-16">
                                <History className="h-12 w-12 text-muted-foreground mb-4" />
                                <h3 className="text-lg font-semibold mb-2">No Run History</h3>
                                <p className="text-muted-foreground text-center mb-6 max-w-md">
                                    Run a simulation to see your history here.
                                </p>
                                <Button asChild>
                                    <Link href="/simulations">Go to Simulations</Link>
                                </Button>
                            </CardContent>
                        </Card>
                    ) : (
                        <Card>
                            <CardHeader>
                                <CardTitle>All Simulation Runs</CardTitle>
                                <CardDescription>
                                    {runs.length} run{runs.length !== 1 ? "s" : ""} found
                                </CardDescription>
                            </CardHeader>
                            <CardContent>
                                <div className="space-y-3">
                                    {runs.map((run) => (
                                        <div
                                            key={run.id}
                                            className="flex items-center justify-between p-4 rounded-lg border hover:bg-muted/50"
                                        >
                                            <div className="space-y-1">
                                                <Link
                                                    href={`/simulations/${run.simulation_id}`}
                                                    className="font-medium hover:underline"
                                                >
                                                    {run.simulationName}
                                                </Link>
                                                <p className="text-sm text-muted-foreground">
                                                    {run.iterations} iterations • {formatDistanceToNow(new Date(run.ran_at), { addSuffix: true })}
                                                </p>
                                            </div>
                                            <Button variant="outline" size="sm" asChild>
                                                <Link href={`/simulations/${run.simulation_id}`}>
                                                    <Play className="mr-2 h-3 w-3" />
                                                    View Results
                                                </Link>
                                            </Button>
                                        </div>
                                    ))}
                                </div>
                            </CardContent>
                        </Card>
                    )}
                </div>
            </SidebarInset>
        </SidebarProvider>
    )
}
