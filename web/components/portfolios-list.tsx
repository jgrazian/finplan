"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { MoreVertical, Plus, Wallet, Pencil, Trash2, TrendingUp } from "lucide-react";
import { PortfolioListItem } from "@/lib/types";
import { listPortfolios, deletePortfolio } from "@/lib/api";
import { formatDistanceToNow } from "date-fns";

export function PortfoliosList() {
    const router = useRouter();
    const [portfolios, setPortfolios] = React.useState<PortfolioListItem[]>([]);
    const [loading, setLoading] = React.useState(true);
    const [error, setError] = React.useState<string | null>(null);

    const fetchPortfolios = React.useCallback(async () => {
        try {
            setLoading(true);
            const data = await listPortfolios();
            setPortfolios(data);
            setError(null);
        } catch (err) {
            setError("Failed to load portfolios");
            console.error(err);
        } finally {
            setLoading(false);
        }
    }, []);

    React.useEffect(() => {
        fetchPortfolios();
    }, [fetchPortfolios]);

    const handleDelete = async (id: string, name: string) => {
        if (!confirm(`Are you sure you want to delete "${name}"?`)) return;
        try {
            await deletePortfolio(id);
            setPortfolios(portfolios.filter((p) => p.id !== id));
        } catch (err) {
            console.error("Failed to delete portfolio:", err);
        }
    };

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 }).format(amount);

    if (loading) {
        return (
            <div className="space-y-4">
                {[1, 2, 3].map((i) => (
                    <Card key={i} className="animate-pulse">
                        <CardHeader>
                            <div className="h-6 bg-muted rounded w-1/3"></div>
                            <div className="h-4 bg-muted rounded w-1/4 mt-2"></div>
                        </CardHeader>
                    </Card>
                ))}
            </div>
        );
    }

    if (error) {
        return (
            <Card>
                <CardContent className="py-10 text-center">
                    <p className="text-destructive mb-4">{error}</p>
                    <Button onClick={fetchPortfolios}>Retry</Button>
                </CardContent>
            </Card>
        );
    }

    if (portfolios.length === 0) {
        return (
            <Card className="border-dashed">
                <CardContent className="flex flex-col items-center justify-center py-16">
                    <Wallet className="h-16 w-16 text-muted-foreground mb-4" />
                    <h3 className="text-xl font-semibold mb-2">No Portfolios Yet</h3>
                    <p className="text-muted-foreground mb-6 text-center max-w-md">
                        Create a portfolio to track your accounts and assets. Portfolios can be linked to simulations for financial planning.
                    </p>
                    <Link href="/portfolios/new">
                        <Button>
                            <Plus className="mr-2 h-4 w-4" />
                            Create Your First Portfolio
                        </Button>
                    </Link>
                </CardContent>
            </Card>
        );
    }

    return (
        <div className="space-y-4">
            <div className="flex justify-between items-center">
                <div>
                    <h2 className="text-lg font-semibold">Your Portfolios</h2>
                    <p className="text-sm text-muted-foreground">
                        {portfolios.length} portfolio{portfolios.length !== 1 ? "s" : ""}
                    </p>
                </div>
                <Link href="/portfolios/new">
                    <Button>
                        <Plus className="mr-2 h-4 w-4" />
                        New Portfolio
                    </Button>
                </Link>
            </div>

            <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-3">
                {portfolios.map((portfolio) => (
                    <Card key={portfolio.id} className="hover:shadow-md transition-shadow">
                        <CardHeader className="pb-2">
                            <div className="flex justify-between items-start">
                                <div className="flex-1 min-w-0">
                                    <CardTitle className="text-base truncate">
                                        <Link href={`/portfolios/${portfolio.id}`} className="hover:underline">
                                            {portfolio.name}
                                        </Link>
                                    </CardTitle>
                                    <CardDescription className="truncate">
                                        {portfolio.description || "No description"}
                                    </CardDescription>
                                </div>
                                <DropdownMenu>
                                    <DropdownMenuTrigger asChild>
                                        <Button variant="ghost" size="icon" className="h-8 w-8">
                                            <MoreVertical className="h-4 w-4" />
                                        </Button>
                                    </DropdownMenuTrigger>
                                    <DropdownMenuContent align="end">
                                        <DropdownMenuItem onClick={() => router.push(`/portfolios/${portfolio.id}/edit`)}>
                                            <Pencil className="mr-2 h-4 w-4" />
                                            Edit
                                        </DropdownMenuItem>
                                        <DropdownMenuItem onClick={() => router.push(`/simulations/new?portfolio=${portfolio.id}`)}>
                                            <TrendingUp className="mr-2 h-4 w-4" />
                                            Run Simulation
                                        </DropdownMenuItem>
                                        <DropdownMenuItem
                                            onClick={() => handleDelete(portfolio.id, portfolio.name)}
                                            className="text-destructive"
                                        >
                                            <Trash2 className="mr-2 h-4 w-4" />
                                            Delete
                                        </DropdownMenuItem>
                                    </DropdownMenuContent>
                                </DropdownMenu>
                            </div>
                        </CardHeader>
                        <CardContent>
                            <div className="space-y-2">
                                <div className="flex justify-between items-center">
                                    <span className="text-sm text-muted-foreground">Net Worth</span>
                                    <span className="text-lg font-bold">{formatCurrency(portfolio.total_value)}</span>
                                </div>
                                <div className="flex justify-between items-center text-sm">
                                    <span className="text-muted-foreground">Accounts</span>
                                    <span>{portfolio.account_count}</span>
                                </div>
                                <div className="text-xs text-muted-foreground pt-2 border-t">
                                    Updated {formatDistanceToNow(new Date(portfolio.updated_at), { addSuffix: true })}
                                </div>
                            </div>
                        </CardContent>
                    </Card>
                ))}
            </div>
        </div>
    );
}
