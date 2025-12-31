"use client";

import * as React from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Pencil, TrendingUp, Wallet, Building2, Car, CreditCard, ArrowLeft } from "lucide-react";
import { SavedPortfolio, PortfolioNetworth } from "@/lib/types";
import { getPortfolio, getPortfolioNetworth } from "@/lib/api";
import { formatDistanceToNow } from "date-fns";

interface PortfolioDetailProps {
    portfolioId: string;
}

const formatCurrency = (amount: number) =>
    new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 }).format(amount);

const ACCOUNT_TYPE_COLORS: Record<string, string> = {
    Taxable: "bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-300",
    TaxDeferred: "bg-yellow-100 text-yellow-800 dark:bg-yellow-900 dark:text-yellow-300",
    TaxFree: "bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-300",
    Illiquid: "bg-gray-100 text-gray-800 dark:bg-gray-800 dark:text-gray-300",
};

const ASSET_CLASS_ICONS: Record<string, React.ReactNode> = {
    Investable: <Wallet className="h-4 w-4" />,
    RealEstate: <Building2 className="h-4 w-4" />,
    Depreciating: <Car className="h-4 w-4" />,
    Liability: <CreditCard className="h-4 w-4" />,
};

export function PortfolioDetail({ portfolioId }: PortfolioDetailProps) {
    const router = useRouter();
    const [portfolio, setPortfolio] = React.useState<SavedPortfolio | null>(null);
    const [networth, setNetworth] = React.useState<PortfolioNetworth | null>(null);
    const [loading, setLoading] = React.useState(true);
    const [error, setError] = React.useState<string | null>(null);

    React.useEffect(() => {
        async function fetchData() {
            try {
                setLoading(true);
                const [portfolioData, networthData] = await Promise.all([
                    getPortfolio(portfolioId),
                    getPortfolioNetworth(portfolioId),
                ]);
                setPortfolio(portfolioData);
                setNetworth(networthData);
                setError(null);
            } catch (err) {
                setError("Failed to load portfolio");
                console.error(err);
            } finally {
                setLoading(false);
            }
        }
        fetchData();
    }, [portfolioId]);

    if (loading) {
        return (
            <div className="space-y-4">
                <Card className="animate-pulse">
                    <CardHeader>
                        <div className="h-8 bg-muted rounded w-1/3"></div>
                        <div className="h-4 bg-muted rounded w-1/4 mt-2"></div>
                    </CardHeader>
                    <CardContent>
                        <div className="h-20 bg-muted rounded"></div>
                    </CardContent>
                </Card>
            </div>
        );
    }

    if (error || !portfolio) {
        return (
            <Card>
                <CardContent className="py-10 text-center">
                    <p className="text-destructive mb-4">{error || "Portfolio not found"}</p>
                    <Button onClick={() => router.push("/portfolios")}>Back to Portfolios</Button>
                </CardContent>
            </Card>
        );
    }

    return (
        <div className="space-y-6">
            {/* Header */}
            <div className="flex items-center gap-4">
                <Button variant="ghost" size="icon" onClick={() => router.push("/portfolios")}>
                    <ArrowLeft className="h-4 w-4" />
                </Button>
                <div className="flex-1">
                    <h1 className="text-2xl font-bold">{portfolio.name}</h1>
                    {portfolio.description && (
                        <p className="text-muted-foreground">{portfolio.description}</p>
                    )}
                </div>
                <div className="flex gap-2">
                    <Link href={`/portfolios/${portfolioId}/edit`}>
                        <Button variant="outline">
                            <Pencil className="mr-2 h-4 w-4" />
                            Edit
                        </Button>
                    </Link>
                    <Link href={`/simulations/new?portfolio=${portfolioId}`}>
                        <Button>
                            <TrendingUp className="mr-2 h-4 w-4" />
                            Run Simulation
                        </Button>
                    </Link>
                </div>
            </div>

            {/* Net Worth Summary */}
            <div className="grid gap-4 md:grid-cols-3">
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Total Net Worth</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-3xl font-bold">{formatCurrency(networth?.total_value || 0)}</p>
                        <p className="text-xs text-muted-foreground mt-1">
                            Updated {formatDistanceToNow(new Date(portfolio.updated_at), { addSuffix: true })}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">By Account Type</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <div className="space-y-2">
                            {networth && Object.entries(networth.by_account_type).map(([type, value]) => (
                                <div key={type} className="flex justify-between items-center text-sm">
                                    <Badge variant="outline" className={ACCOUNT_TYPE_COLORS[type]}>
                                        {type}
                                    </Badge>
                                    <span className="font-medium">{formatCurrency(value)}</span>
                                </div>
                            ))}
                        </div>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">By Asset Class</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <div className="space-y-2">
                            {networth && Object.entries(networth.by_asset_class).map(([assetClass, value]) => (
                                <div key={assetClass} className="flex justify-between items-center text-sm">
                                    <div className="flex items-center gap-2">
                                        {ASSET_CLASS_ICONS[assetClass]}
                                        <span>{assetClass}</span>
                                    </div>
                                    <span className="font-medium">{formatCurrency(value)}</span>
                                </div>
                            ))}
                        </div>
                    </CardContent>
                </Card>
            </div>

            {/* Accounts List */}
            <div className="space-y-4">
                <h2 className="text-lg font-semibold">Accounts ({portfolio.accounts.length})</h2>

                {portfolio.accounts.length === 0 ? (
                    <Card className="border-dashed">
                        <CardContent className="py-10 text-center">
                            <p className="text-muted-foreground mb-4">No accounts in this portfolio</p>
                            <Link href={`/portfolios/${portfolioId}/edit`}>
                                <Button variant="outline">
                                    <Pencil className="mr-2 h-4 w-4" />
                                    Add Accounts
                                </Button>
                            </Link>
                        </CardContent>
                    </Card>
                ) : (
                    <div className="grid gap-4 md:grid-cols-2">
                        {portfolio.accounts.map((account) => {
                            const accountTotal = account.assets.reduce(
                                (sum, a) => sum + (a.asset_class === "Liability" ? -a.initial_value : a.initial_value),
                                0
                            );
                            return (
                                <Card key={account.account_id}>
                                    <CardHeader className="pb-2">
                                        <div className="flex justify-between items-start">
                                            <div>
                                                <CardTitle className="text-base">
                                                    {account.name || `Account #${account.account_id}`}
                                                </CardTitle>
                                                <Badge variant="outline" className={ACCOUNT_TYPE_COLORS[account.account_type]}>
                                                    {account.account_type}
                                                </Badge>
                                            </div>
                                            <span className="text-lg font-bold">{formatCurrency(accountTotal)}</span>
                                        </div>
                                    </CardHeader>
                                    <CardContent>
                                        {account.assets.length === 0 ? (
                                            <p className="text-sm text-muted-foreground">No assets</p>
                                        ) : (
                                            <div className="space-y-2">
                                                {account.assets.map((asset) => (
                                                    <div
                                                        key={asset.asset_id}
                                                        className="flex justify-between items-center text-sm py-1 border-b last:border-0"
                                                    >
                                                        <div className="flex items-center gap-2">
                                                            {ASSET_CLASS_ICONS[asset.asset_class]}
                                                            <span>{asset.name || `Asset #${asset.asset_id}`}</span>
                                                        </div>
                                                        <span className={asset.asset_class === "Liability" ? "text-destructive" : ""}>
                                                            {asset.asset_class === "Liability" ? "-" : ""}
                                                            {formatCurrency(asset.initial_value)}
                                                        </span>
                                                    </div>
                                                ))}
                                            </div>
                                        )}
                                    </CardContent>
                                </Card>
                            );
                        })}
                    </div>
                )}
            </div>
        </div>
    );
}
