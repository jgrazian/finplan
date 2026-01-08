"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { useWizardStore } from "../hooks/useWizardStore";
import { MoneyInput } from "../components/MoneyInput";

export function CurrentSavingsStep() {
    const savings = useWizardStore((state) => state.savings);
    const setSavings = useWizardStore((state) => state.setSavings);
    const monthlyExpenses = useWizardStore((state) => state.income.salary > 0 ? 5000 : 0); // Rough estimate

    const totalLiquidSavings = savings.checking + savings.savings + savings.hysa;

    const monthsOfExpensesCovered = React.useMemo(() => {
        if (monthlyExpenses === 0 || savings.emergencyFund === 0) return 0;
        return Math.round((savings.emergencyFund / monthlyExpenses) * 10) / 10;
    }, [savings.emergencyFund, monthlyExpenses]);

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            maximumFractionDigits: 0,
        }).format(amount);

    return (
        <div className="space-y-6 max-w-2xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Let's see what you've saved</h2>
                <p className="text-muted-foreground mt-2">
                    Your liquid savings provide financial security and flexibility for emergencies and opportunities.
                </p>
            </div>

            <Card>
                <CardHeader>
                    <CardTitle>Do you have a checking account?</CardTitle>
                    <CardDescription>
                        The account you use for daily expenses and bill payments
                    </CardDescription>
                </CardHeader>
                <CardContent>
                    <div className="space-y-2">
                        <Label htmlFor="checking">Current Balance</Label>
                        <MoneyInput
                            value={savings.checking}
                            onChange={(value) => setSavings({ checking: value })}
                            placeholder="15000"
                        />
                    </div>
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>Do you have a savings account?</CardTitle>
                    <CardDescription>
                        A standard savings account at your bank or credit union
                    </CardDescription>
                </CardHeader>
                <CardContent>
                    <div className="space-y-2">
                        <Label htmlFor="savings">Current Balance</Label>
                        <MoneyInput
                            value={savings.savings}
                            onChange={(value) => setSavings({ savings: value })}
                            placeholder="35000"
                        />
                    </div>
                </CardContent>
            </Card>

            <Card>
                <CardHeader>
                    <CardTitle>Do you have a High-Yield Savings Account (HYSA)?</CardTitle>
                    <CardDescription>
                        Online savings accounts typically offering higher interest rates
                    </CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="space-y-2">
                        <Label htmlFor="hysa">Current Balance</Label>
                        <MoneyInput
                            value={savings.hysa}
                            onChange={(value) => setSavings({ hysa: value })}
                            placeholder="25000"
                        />
                    </div>

                    {savings.hysa > 0 && (
                        <div className="space-y-2">
                            <Label htmlFor="hysa-rate">Interest Rate (APY)</Label>
                            <div className="relative">
                                <Input
                                    id="hysa-rate"
                                    type="number"
                                    value={savings.hysaRate}
                                    onChange={(e) => setSavings({ hysaRate: parseFloat(e.target.value) || 0 })}
                                    placeholder="4.5"
                                    step="0.1"
                                    min="0"
                                    max="20"
                                />
                                <span className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground">
                                    %
                                </span>
                            </div>
                            <p className="text-xs text-muted-foreground">
                                Current high-yield savings accounts typically offer 4-5% APY
                            </p>
                        </div>
                    )}
                </CardContent>
            </Card>

            {totalLiquidSavings > 0 && (
                <Card>
                    <CardHeader>
                        <CardTitle>Emergency Fund</CardTitle>
                        <CardDescription>
                            Financial advisors typically recommend 3-6 months of expenses as an emergency fund
                        </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-4">
                        <div className="rounded-lg bg-muted p-4">
                            <p className="text-sm mb-2">
                                <span className="font-medium">Total liquid savings:</span>{" "}
                                {formatCurrency(totalLiquidSavings)}
                            </p>
                            <p className="text-xs text-muted-foreground">
                                This includes your checking, savings, and high-yield savings accounts
                            </p>
                        </div>

                        <div className="space-y-2">
                            <Label htmlFor="emergency-fund">
                                How much of this is your emergency fund?
                            </Label>
                            <MoneyInput
                                value={savings.emergencyFund}
                                onChange={(value) => {
                                    // Cap at total liquid savings
                                    const cappedValue = Math.min(value, totalLiquidSavings);
                                    setSavings({ emergencyFund: cappedValue });
                                }}
                                placeholder={totalLiquidSavings.toString()}
                            />
                            {savings.emergencyFund > 0 && monthlyExpenses > 0 && (
                                <div className="rounded-lg bg-muted p-3">
                                    <p className="text-sm">
                                        This should cover about{" "}
                                        <span className="font-medium">
                                            {monthsOfExpensesCovered} months
                                        </span>{" "}
                                        of typical expenses.
                                    </p>
                                    {monthsOfExpensesCovered < 3 && (
                                        <p className="text-xs text-orange-600 mt-1">
                                            ⚠️ Consider building up to at least 3 months of expenses
                                        </p>
                                    )}
                                    {monthsOfExpensesCovered >= 3 && monthsOfExpensesCovered < 6 && (
                                        <p className="text-xs text-blue-600 mt-1">
                                            ✓ You're within the recommended 3-6 month range
                                        </p>
                                    )}
                                    {monthsOfExpensesCovered >= 6 && (
                                        <p className="text-xs text-green-600 mt-1">
                                            ✓ Great! You have a solid emergency fund
                                        </p>
                                    )}
                                </div>
                            )}
                        </div>
                    </CardContent>
                </Card>
            )}

            {totalLiquidSavings === 0 && (
                <div className="rounded-lg bg-muted p-4 text-sm">
                    <p className="font-medium mb-1">💡 Building Your Emergency Fund</p>
                    <p className="text-muted-foreground">
                        Starting to save for emergencies is an important first step in financial planning.
                        Even small contributions can add up over time. Consider setting up automatic transfers
                        from your checking to a savings account.
                    </p>
                </div>
            )}
        </div>
    );
}
