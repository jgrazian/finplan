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
import { SimulationWizard } from "@/components/simulation-wizard"
import { useParams } from "next/navigation"
import { useEffect, useState } from "react"
import { getSimulation } from "@/lib/api"
import { SavedSimulation } from "@/lib/types"
import { Card, CardContent } from "@/components/ui/card"

export default function EditSimulationPage() {
    const params = useParams()
    const id = params.id as string
    const [simulation, setSimulation] = useState<SavedSimulation | null>(null)
    const [loading, setLoading] = useState(true)
    const [error, setError] = useState<string | null>(null)

    useEffect(() => {
        async function load() {
            try {
                const data = await getSimulation(id)
                setSimulation(data)
            } catch (err) {
                setError("Failed to load simulation")
                console.error(err)
            } finally {
                setLoading(false)
            }
        }
        load()
    }, [id])

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
                                    <BreadcrumbLink href="/simulations">Simulations</BreadcrumbLink>
                                </BreadcrumbItem>
                                <BreadcrumbSeparator className="hidden md:block" />
                                <BreadcrumbItem>
                                    <BreadcrumbPage>Edit</BreadcrumbPage>
                                </BreadcrumbItem>
                            </BreadcrumbList>
                        </Breadcrumb>
                    </div>
                </header>
                <div className="flex flex-1 flex-col gap-6 p-6 pt-0">
                    <div className="flex flex-col gap-2">
                        <h1 className="text-3xl font-bold tracking-tight">Edit Simulation</h1>
                        <p className="text-muted-foreground">
                            Modify your simulation parameters
                        </p>
                    </div>
                    {loading ? (
                        <Card>
                            <CardContent className="flex items-center justify-center py-10">
                                <div className="text-muted-foreground">Loading simulation...</div>
                            </CardContent>
                        </Card>
                    ) : error ? (
                        <Card>
                            <CardContent className="flex items-center justify-center py-10">
                                <div className="text-destructive">{error}</div>
                            </CardContent>
                        </Card>
                    ) : simulation ? (
                        <SimulationWizard
                            initialData={{
                                id: simulation.id,
                                name: simulation.name,
                                description: simulation.description,
                                parameters: simulation.parameters,
                            }}
                        />
                    ) : null}
                </div>
            </SidebarInset>
        </SidebarProvider>
    )
}
