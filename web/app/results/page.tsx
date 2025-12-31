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
import { listSimulations } from "@/lib/api"
import { SimulationListItem } from "@/lib/types"
import { BarChart3, Play } from "lucide-react"

export default function ResultsPage() {
    const [simulations, setSimulations] = useState<SimulationListItem[]>([])
    const [loading, setLoading] = useState(true)

    useEffect(() => {
        async function load() {
            try {
                const data = await listSimulations()
                setSimulations(data)
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
                                <BreadcrumbItem>
                                    <BreadcrumbPage>Results</BreadcrumbPage>
                                </BreadcrumbItem>
                            </BreadcrumbList>
                        </Breadcrumb>
                    </div>
                </header>
                <div className="flex flex-1 flex-col gap-6 p-6 pt-0">
                    <div className="flex flex-col gap-2">
                        <h1 className="text-3xl font-bold tracking-tight">Results</h1>
                        <p className="text-muted-foreground">
                            View simulation results and portfolio projections
                        </p>
                    </div>

                    {loading ? (
                        <Card>
                            <CardContent className="flex items-center justify-center py-10">
                                <div className="text-muted-foreground">Loading...</div>
                            </CardContent>
                        </Card>
                    ) : simulations.length === 0 ? (
                        <Card>
                            <CardContent className="flex flex-col items-center justify-center py-16">
                                <BarChart3 className="h-12 w-12 text-muted-foreground mb-4" />
                                <h3 className="text-lg font-semibold mb-2">No Simulations Yet</h3>
                                <p className="text-muted-foreground text-center mb-6 max-w-md">
                                    Create a simulation first to view results and projections.
                                </p>
                                <Button asChild>
                                    <Link href="/simulations/new">Create Simulation</Link>
                                </Button>
                            </CardContent>
                        </Card>
                    ) : (
                        <Card>
                            <CardHeader>
                                <CardTitle>Select a Simulation</CardTitle>
                                <CardDescription>
                                    Choose a simulation to view or run to see results
                                </CardDescription>
                            </CardHeader>
                            <CardContent>
                                <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
                                    {simulations.map((sim) => (
                                        <Card key={sim.id} className="hover:bg-muted/50 transition-colors">
                                            <CardHeader className="pb-2">
                                                <CardTitle className="text-base">{sim.name}</CardTitle>
                                                {sim.description && (
                                                    <CardDescription className="text-xs truncate">
                                                        {sim.description}
                                                    </CardDescription>
                                                )}
                                            </CardHeader>
                                            <CardContent>
                                                <Button asChild className="w-full">
                                                    <Link href={`/simulations/${sim.id}`}>
                                                        <Play className="mr-2 h-4 w-4" />
                                                        View & Run
                                                    </Link>
                                                </Button>
                                            </CardContent>
                                        </Card>
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
