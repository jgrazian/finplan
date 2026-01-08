"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Plus, Trash2, AlertCircle } from "lucide-react";
import { useWizardStore } from "../hooks/useWizardStore";
import { MoneyInput } from "../components/MoneyInput";
import { Debt } from "../types";

const DEBT_TYPES: { value: Debt["type"]; label: string; description: string; avgRate: number }[] = [
    { value: "StudentLoan", label: "Student Loans", description: "Federal or private student loans", avgRate: 5.5 },
    { value: "CarLoan", label: "Car Loan", description: "Auto financing", avgRate: 6.5 },
    { value: "CreditCard", label: "Credit Card Debt", description: "Revolving credit card balances", avgRate: 22.0 },
    { value: "Personal", label: "Personal Loan", description: "Unsecured personal loan", avgRate: 11.0 },
    { value: "Medical", label: "Medical Debt", description: "Healthcare bills or payment plans", avgRate: 0 },
    { value: "Other", label: "Other Debt", description: "Other loans or debts", avgRate: 8.0 },
];

export function DebtsStep() {
    const debts = useWizardStore((state) => state.debts);
    const addDebt = useWizardStore((state) => state.addDebt);
    const updateDebt = useWizardStore((state) => state.updateDebt);
    const removeDebt = useWizardStore((state) => state.removeDebt);

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            maximumFractionDigits: 0,
        }).format(amount);

    const handleAddDebt = (type: Debt["type"]) => {
        const id = `debt-${Date.now()}`;
        const defaultRate = DEBT_TYPES.find((t) => t.value === type)?.avgRate || 0;
        addDebt({
            id,
            type,
            balance: 0,
            monthlyPayment: 0,
            interestRate: defaultRate,
        });
    };

    const calculatePayoffDate = (balance: number, monthlyPayment: number, interestRate: number) => {
        if (balance === 0 || monthlyPayment === 0) return null;

        const monthlyRate = interestRate / 100 / 12;
        if (monthlyRate === 0) {
            const months = balance / monthlyPayment;
            return Math.ceil(months);
        }

        // Formula: n = -log(1 - r*P/M) / log(1 + r)
        // where n = months, r = monthly rate, P = principal, M = monthly payment
        const months = -Math.log(1 - (monthlyRate * balance) / monthlyPayment) / Math.log(1 + monthlyRate);

        if (isNaN(months) || months < 0 || months > 600) return null; // Sanity check
        return Math.ceil(months);
    };

    const formatPayoffDate = (months: number | null) => {
        if (!months) return "Unable to calculate";

        const years = Math.floor(months / 12);
        const remainingMonths = months % 12;

        if (years === 0) return `${remainingMonths} months`;
        if (remainingMonths === 0) return `${years} years`;
        return `${years} years, ${remainingMonths} months`;
    };

    const totalDebt = debts.reduce((sum, debt) => sum + debt.balance, 0);
    const totalMonthlyPayments = debts.reduce((sum, debt) => sum + debt.monthlyPayment, 0);
    const highInterestDebt = debts.filter((debt) => debt.interestRate >= 15).reduce((sum, debt) => sum + debt.balance, 0);

    return (
        <div className="space-y-6 max-w-2xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Let's account for any debts</h2>
                <p className="text-muted-foreground mt-2">
                    Understanding your debts helps us plan for debt payoff and optimize your cash flow.
                </p>
            </div>

            {debts.length === 0 ? (
                <Card>
                    <CardHeader>
                        <CardTitle>Do you have any of these debts?</CardTitle>
                        <CardDescription>
                            Select the types of debt you currently have
                        </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-3">
                        <div className="grid gap-2">
                            {DEBT_TYPES.map((debtType) => (
                                <Button
                                    key={debtType.value}
                                    variant="outline"
                                    className="justify-start h-auto py-3"
                                    onClick={() => handleAddDebt(debtType.value)}
                                >
                                    <div className="flex flex-col items-start flex-1">
                                        <span className="font-medium">{debtType.label}</span>
                                        <span className="text-xs text-muted-foreground">{debtType.description}</span>
                                    </div>
                                    <Plus className="h-4 w-4 ml-2" />
                                </Button>
                            ))}
                        </div>

                        <Button variant="ghost" className="w-full" onClick={() => { }}>
                            Skip - I don't have any debts
                        </Button>
                    </CardContent>
                </Card>
            ) : (
                <>
                    {debts.map((debt) => {
                        const debtTypeInfo = DEBT_TYPES.find((t) => t.value === debt.type);
                        const payoffMonths = calculatePayoffDate(debt.balance, debt.monthlyPayment, debt.interestRate);

                        return (
                            <Card key={debt.id}>
                                <CardHeader>
                                    <div className="flex items-start justify-between">
                                        <div>
                                            <CardTitle>{debtTypeInfo?.label || debt.type}</CardTitle>
                                            <CardDescription>{debtTypeInfo?.description}</CardDescription>
                                        </div>
                                        <Button
                                            variant="ghost"
                                            size="icon"
                                            onClick={() => removeDebt(debt.id)}
                                        >
                                            <Trash2 className="h-4 w-4" />
                                        </Button>
                                    </div>
                                </CardHeader>
                                <CardContent className="space-y-4">
                                    <div className="space-y-2">
                                        <Label htmlFor={`balance-${debt.id}`}>Current Balance</Label>
                                        <MoneyInput
                                            value={debt.balance}
                                            onChange={(value) => updateDebt(debt.id, { balance: value })}
                                            placeholder="0"
                                        />
                                    </div>

                                    <div className="grid grid-cols-2 gap-4">
                                        <div className="space-y-2">
                                            <Label htmlFor={`payment-${debt.id}`}>Monthly Payment</Label>
                                            <MoneyInput
                                                value={debt.monthlyPayment}
                                                onChange={(value) => updateDebt(debt.id, { monthlyPayment: value })}
                                                placeholder="0"
                                            />
                                        </div>
                                        <div className="space-y-2">
                                            <Label htmlFor={`rate-${debt.id}`}>Interest Rate</Label>
                                            <div className="relative">
                                                <Input
                                                    id={`rate-${debt.id}`}
                                                    type="number"
                                                    value={debt.interestRate}
                                                    onChange={(e) => updateDebt(debt.id, { interestRate: parseFloat(e.target.value) || 0 })}
                                                    placeholder={debtTypeInfo?.avgRate.toString() || "0"}
                                                    step="0.1"
                                                />
                                                <span className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground">
                                                    %
                                                </span>
                                            </div>
                                        </div>
                                    </div>

                                    {debt.type === "Other" && (
                                        <div className="space-y-2">
                                            <Label htmlFor={`description-${debt.id}`}>Description (optional)</Label>
                                            <Input
                                                id={`description-${debt.id}`}
                                                value={debt.description || ""}
                                                onChange={(e) => updateDebt(debt.id, { description: e.target.value })}
                                                placeholder="e.g., Home equity loan"
                                            />
                                        </div>
                                    )}

                                    {debt.balance > 0 && debt.monthlyPayment > 0 && (
                                        <div className="rounded-lg bg-muted p-3 space-y-2 text-sm">
                                            <div className="flex justify-between">
                                                <span className="text-muted-foreground">Estimated Payoff:</span>
                                                <span className="font-medium">{formatPayoffDate(payoffMonths)}</span>
                                            </div>
                                            {debt.interestRate >= 15 && (
                                                <div className="flex items-start gap-2 text-orange-600">
                                                    <AlertCircle className="h-4 w-4 mt-0.5 flex-shrink-0" />
                                                    <span className="text-xs">
                                                        High interest rate - consider prioritizing this debt
                                                    </span>
                                                </div>
                                            )}
                                            {debt.monthlyPayment < (debt.balance * debt.interestRate / 100 / 12) && debt.interestRate > 0 && (
                                                <div className="flex items-start gap-2 text-red-600">
                                                    <AlertCircle className="h-4 w-4 mt-0.5 flex-shrink-0" />
                                                    <span className="text-xs">
                                                        ⚠️ Payment is less than monthly interest - balance will grow
                                                    </span>
                                                </div>
                                            )}
                                        </div>
                                    )}
                                </CardContent>
                            </Card>
                        );
                    })}

                    <Card>
                        <CardHeader>
                            <CardTitle>Add Another Debt</CardTitle>
                        </CardHeader>
                        <CardContent>
                            <div className="grid gap-2">
                                {DEBT_TYPES.map((debtType) => (
                                    <Button
                                        key={`add-${debtType.value}`}
                                        variant="outline"
                                        className="justify-start h-auto py-2"
                                        onClick={() => handleAddDebt(debtType.value)}
                                    >
                                        <div className="flex flex-col items-start flex-1">
                                            <span className="font-medium text-sm">{debtType.label}</span>
                                            <span className="text-xs text-muted-foreground">{debtType.description}</span>
                                        </div>
                                        <Plus className="h-4 w-4 ml-2" />
                                    </Button>
                                ))}
                            </div>
                        </CardContent>
                    </Card>

                    {debts.length > 0 && (
                        <Card>
                            <CardHeader>
                                <CardTitle>Debt Summary</CardTitle>
                            </CardHeader>
                            <CardContent>
                                <div className="space-y-3 text-sm">
                                    <div className="grid grid-cols-3 gap-4 pb-3 border-b">
                                        <div className="text-center">
                                            <div className="text-2xl font-bold">{formatCurrency(totalDebt)}</div>
                                            <div className="text-xs text-muted-foreground">Total Debt</div>
                                        </div>
                                        <div className="text-center">
                                            <div className="text-2xl font-bold">{formatCurrency(totalMonthlyPayments)}</div>
                                            <div className="text-xs text-muted-foreground">Monthly Payments</div>
                                        </div>
                                        <div className="text-center">
                                            <div className="text-2xl font-bold">{debts.length}</div>
                                            <div className="text-xs text-muted-foreground">Active Debts</div>
                                        </div>
                                    </div>

                                    {highInterestDebt > 0 && (
                                        <div className="rounded-lg bg-orange-50 dark:bg-orange-950 p-3 border border-orange-200 dark:border-orange-800">
                                            <div className="flex items-start gap-2">
                                                <AlertCircle className="h-4 w-4 text-orange-600 mt-0.5 flex-shrink-0" />
                                                <div className="text-xs space-y-1">
                                                    <p className="font-medium text-orange-900 dark:text-orange-100">
                                                        You have {formatCurrency(highInterestDebt)} in high-interest debt
                                                    </p>
                                                    <p className="text-orange-700 dark:text-orange-300">
                                                        Consider the avalanche method: pay minimums on all debts, then put extra toward the highest interest rate first.
                                                    </p>
                                                </div>
                                            </div>
                                        </div>
                                    )}

                                    <div className="space-y-2 pt-2">
                                        <p className="font-medium">Debt Breakdown:</p>
                                        {debts.map((debt) => {
                                            const debtTypeInfo = DEBT_TYPES.find((t) => t.value === debt.type);
                                            return (
                                                <div key={debt.id} className="flex justify-between items-center text-xs">
                                                    <span className="text-muted-foreground">
                                                        {debtTypeInfo?.label} @ {debt.interestRate}%
                                                    </span>
                                                    <span className="font-medium">
                                                        {formatCurrency(debt.balance)} ({formatCurrency(debt.monthlyPayment)}/mo)
                                                    </span>
                                                </div>
                                            );
                                        })}
                                    </div>
                                </div>
                            </CardContent>
                        </Card>
                    )}
                </>
            )}
        </div>
    );
}
