"use client";

import * as React from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { SimulationParameters, SavedPortfolio } from "@/lib/types";

interface ReviewStepProps {
    name: string;
    description: string;
    parameters: SimulationParameters;
    selectedPortfolio: SavedPortfolio | null;
}

export function ReviewStep({
    name,
    description,
    parameters,
    selectedPortfolio,
}: ReviewStepProps) {
    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", { style: "currency", currency: "USD" }).format(amount);

    const totalAccountValue = parameters.accounts.reduce(
        (sum, acc) => sum + acc.assets.reduce((s, a) => s + a.initial_value, 0),
        0
    );

    const monthlyIncome = parameters.cash_flows
        .filter((cf) => "Income" in cf.direction)
        .reduce((sum, cf) => {
            const multiplier = cf.repeats === "Monthly" ? 1 : cf.repeats === "Yearly" ? 1 / 12 : cf.repeats === "Weekly" ? 4.33 : cf.repeats === "BiWeekly" ? 2.17 : 0;
            return sum + cf.amount * multiplier;
        }, 0);

    const monthlyExpenses = parameters.cash_flows
        .filter((cf) => "Expense" in cf.direction)
        .reduce((sum, cf) => {
            const multiplier = cf.repeats === "Monthly" ? 1 : cf.repeats === "Yearly" ? 1 / 12 : cf.repeats === "Weekly" ? 4.33 : cf.repeats === "BiWeekly" ? 2.17 : 0;
            return sum + cf.amount * multiplier;
        }, 0);

    return (
        <div className="space-y-6">
            {!name && (
                <div className="bg-destructive/10 border border-destructive/20 rounded-lg p-4">
                    <p className="text-sm text-destructive">Please provide a name for your simulation before saving.</p>
                </div>
            )}

            <div className="grid gap-4 md:grid-cols-2">
                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Simulation Name</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{name || "Untitled"}</p>
                        {description && <p className="text-sm text-muted-foreground mt-1">{description}</p>}
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Portfolio</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{selectedPortfolio?.name || "None"}</p>
                        <p className="text-sm text-muted-foreground">
                            {selectedPortfolio ? `${selectedPortfolio.accounts.length} accounts` : "No portfolio linked"}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Duration</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{parameters.duration_years} years</p>
                        <p className="text-sm text-muted-foreground">
                            {parameters.start_date ? `Starting ${parameters.start_date}` : "Starting today"}
                            {parameters.retirement_age && ` • Retire at ${parameters.retirement_age}`}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Total Account Value</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">{formatCurrency(totalAccountValue)}</p>
                        <p className="text-sm text-muted-foreground">
                            Across {parameters.accounts.length} account{parameters.accounts.length !== 1 ? "s" : ""}
                        </p>
                    </CardContent>
                </Card>

                <Card>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-sm font-medium text-muted-foreground">Monthly Cash Flow</CardTitle>
                    </CardHeader>
                    <CardContent>
                        <p className="text-2xl font-bold">
                            {formatCurrency(monthlyIncome - monthlyExpenses)}
                            <span className="text-sm font-normal text-muted-foreground">/mo</span>
                        </p>
                        <p className="text-sm text-muted-foreground">
                            {formatCurrency(monthlyIncome)} income - {formatCurrency(monthlyExpenses)} expenses
                        </p>
                    </CardContent>
                </Card>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle className="text-sm font-medium">Summary</CardTitle>
                </CardHeader>
                <CardContent>
                    <dl className="space-y-2 text-sm">
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Accounts</dt>
                            <dd>{parameters.accounts.length}</dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Cash Flows</dt>
                            <dd>{parameters.cash_flows.length}</dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Events</dt>
                            <dd>{parameters.events.length}</dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Spending Targets</dt>
                            <dd>{parameters.spending_targets.length}</dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Inflation Profile</dt>
                            <dd>
                                {parameters.inflation_profile === "None"
                                    ? "None"
                                    : typeof parameters.inflation_profile === "object" && "Fixed" in parameters.inflation_profile
                                        ? `Fixed ${(parameters.inflation_profile.Fixed * 100).toFixed(1)}%`
                                        : typeof parameters.inflation_profile === "object" && "Normal" in parameters.inflation_profile
                                            ? `Normal (μ=${(parameters.inflation_profile.Normal.mean * 100).toFixed(1)}%)`
                                            : "Unknown"}
                            </dd>
                        </div>
                        <div className="flex justify-between">
                            <dt className="text-muted-foreground">Return Profiles</dt>
                            <dd>
                                {parameters.named_return_profiles && parameters.named_return_profiles.length > 0
                                    ? `${parameters.named_return_profiles.length} profile${parameters.named_return_profiles.length !== 1 ? "s" : ""}`
                                    : "None"}
                            </dd>
                        </div>
                    </dl>
                </CardContent>
            </Card>
        </div>
    );
}
