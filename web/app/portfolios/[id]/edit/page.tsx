"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import { use } from "react";
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
import { PortfolioWizard } from "@/components/portfolio-wizard";
import { getPortfolio } from "@/lib/api";
import { SavedPortfolio } from "@/lib/types";

export default function EditPortfolioPage({ params }: { params: Promise<{ id: string }> }) {
    const { id } = use(params);
    const router = useRouter();
    const [portfolio, setPortfolio] = React.useState<SavedPortfolio | null>(null);
    const [loading, setLoading] = React.useState(true);

    React.useEffect(() => {
        async function fetchPortfolio() {
            try {
                const data = await getPortfolio(id);
                setPortfolio(data);
            } catch (err) {
                console.error("Failed to load portfolio:", err);
                router.push("/portfolios");
            } finally {
                setLoading(false);
            }
        }
        fetchPortfolio();
    }, [id, router]);

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
                                    <BreadcrumbLink href="/portfolios">Portfolios</BreadcrumbLink>
                                </BreadcrumbItem>
                                <BreadcrumbSeparator className="hidden md:block" />
                                <BreadcrumbItem className="hidden md:block">
                                    <BreadcrumbLink href={`/portfolios/${id}`}>Details</BreadcrumbLink>
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
                    {loading ? (
                        <div className="animate-pulse space-y-4">
                            <div className="h-8 bg-muted rounded w-1/3"></div>
                            <div className="h-4 bg-muted rounded w-1/4"></div>
                            <div className="h-64 bg-muted rounded"></div>
                        </div>
                    ) : portfolio ? (
                        <>
                            <div className="flex flex-col gap-2">
                                <h1 className="text-3xl font-bold tracking-tight">Edit Portfolio</h1>
                                <p className="text-muted-foreground">
                                    Update your accounts and assets
                                </p>
                            </div>
                            <PortfolioWizard
                                initialData={{
                                    id: portfolio.id,
                                    name: portfolio.name,
                                    description: portfolio.description,
                                    accounts: portfolio.accounts,
                                }}
                            />
                        </>
                    ) : null}
                </div>
            </SidebarInset>
        </SidebarProvider>
    );
}
