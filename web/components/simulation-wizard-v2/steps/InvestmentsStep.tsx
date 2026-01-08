"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Plus, Trash2, TrendingUp } from "lucide-react";
import { useWizardStore } from "../hooks/useWizardStore";
import { MoneyInput } from "../components/MoneyInput";
import { InvestmentAccount, PayFrequency } from "../types";

const ACCOUNT_TYPES: { value: InvestmentAccount["type"]; label: string; description: string }[] = [
    { value: "Brokerage", label: "Brokerage Account", description: "Taxable investment account" },
    { value: "Traditional401k", label: "Traditional 401(k)", description: "Pre-tax employer retirement account" },
    { value: "Roth401k", label: "Roth 401(k)", description: "After-tax employer retirement account" },
    { value: "TraditionalIRA", label: "Traditional IRA", description: "Pre-tax individual retirement account" },
    { value: "RothIRA", label: "Roth IRA", description: "After-tax individual retirement account" },
    { value: "HSA", label: "HSA", description: "Health Savings Account (triple tax-advantaged)" },
    { value: "Other", label: "Other", description: "Other investment account" },
];

const IRS_LIMITS_2026 = {
    "Traditional401k": 23500,
    "Roth401k": 23500,
    "TraditionalIRA": 7000,
    "RothIRA": 7000,
    "HSA": 4150, // Individual
};

export function InvestmentsStep() {
    const investments = useWizardStore((state) => state.investments);
    const addInvestment = useWizardStore((state) => state.addInvestment);
    const updateInvestment = useWizardStore((state) => state.updateInvestment);
    const removeInvestment = useWizardStore((state) => state.removeInvestment);

    const [editingId, setEditingId] = React.useState<string | null>(null);
    const [showAllocation, setShowAllocation] = React.useState<Record<string, boolean>>({});

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            maximumFractionDigits: 0,
        }).format(amount);

    const totalByType = React.useMemo(() => {
        const taxDeferred = investments
            .filter((inv) => inv.type === "Traditional401k" || inv.type === "TraditionalIRA")
            .reduce((sum, inv) => sum + inv.balance, 0);

        const taxFree = investments
            .filter((inv) => inv.type === "Roth401k" || inv.type === "RothIRA" || inv.type === "HSA")
            .reduce((sum, inv) => sum + inv.balance, 0);

        const taxable = investments
            .filter((inv) => inv.type === "Brokerage")
            .reduce((sum, inv) => sum + inv.balance, 0);

        return { taxDeferred, taxFree, taxable };
    }, [investments]);

    const handleAddAccount = (type: InvestmentAccount["type"]) => {
        const id = `inv-${Date.now()}`;
        addInvestment({
            id,
            type,
            balance: 0,
        });
        setEditingId(id);
    };

    const handleUpdateAllocation = (id: string, field: keyof NonNullable<InvestmentAccount["allocation"]>, value: number) => {
        const investment = investments.find((inv) => inv.id === id);
        if (!investment) return;

        const currentAllocation = investment.allocation || { stocks: 0, bonds: 0, international: 0, cash: 0 };
        const updatedAllocation = { ...currentAllocation, [field]: value };

        updateInvestment(id, { allocation: updatedAllocation });
    };

    const getAllocationTotal = (id: string) => {
        const investment = investments.find((inv) => inv.id === id);
        if (!investment?.allocation) return 0;
        return investment.allocation.stocks + investment.allocation.bonds +
            investment.allocation.international + investment.allocation.cash;
    };

    return (
        <div className="space-y-6 max-w-2xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Let's look at your investments</h2>
                <p className="text-muted-foreground mt-2">
                    Investment and retirement accounts help you build wealth for the future.
                </p>
            </div>

            {investments.length === 0 ? (
                <Card>
                    <CardHeader>
                        <CardTitle>Do you have any investment accounts?</CardTitle>
                        <CardDescription>
                            Select the types of accounts you have. We'll ask for details next.
                        </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-3">
                        <div className="grid gap-2">
                            {ACCOUNT_TYPES.map((accountType) => (
                                <Button
                                    key={accountType.value}
                                    variant="outline"
                                    className="justify-start h-auto py-3"
                                    onClick={() => handleAddAccount(accountType.value)}
                                >
                                    <div className="flex flex-col items-start flex-1">
                                        <span className="font-medium">{accountType.label}</span>
                                        <span className="text-xs text-muted-foreground">{accountType.description}</span>
                                    </div>
                                    <Plus className="h-4 w-4 ml-2" />
                                </Button>
                            ))}
                        </div>
                    </CardContent>
                </Card>
            ) : (
                <>
                    {investments.map((investment) => (
                        <Card key={investment.id}>
                            <CardHeader>
                                <div className="flex items-start justify-between">
                                    <div>
                                        <CardTitle>
                                            {ACCOUNT_TYPES.find((t) => t.value === investment.type)?.label || investment.type}
                                        </CardTitle>
                                        <CardDescription>
                                            {ACCOUNT_TYPES.find((t) => t.value === investment.type)?.description}
                                        </CardDescription>
                                    </div>
                                    <Button
                                        variant="ghost"
                                        size="icon"
                                        onClick={() => removeInvestment(investment.id)}
                                    >
                                        <Trash2 className="h-4 w-4" />
                                    </Button>
                                </div>
                            </CardHeader>
                            <CardContent className="space-y-4">
                                <div className="space-y-2">
                                    <Label htmlFor={`balance-${investment.id}`}>Current Balance</Label>
                                    <MoneyInput
                                        value={investment.balance}
                                        onChange={(value) => updateInvestment(investment.id, { balance: value })}
                                        placeholder="0"
                                    />
                                </div>

                                <div className="space-y-3">
                                    <div className="flex items-center space-x-2">
                                        <Checkbox
                                            id={`contributing-${investment.id}`}
                                            checked={!!investment.contributions}
                                            onCheckedChange={(checked) => {
                                                if (checked) {
                                                    updateInvestment(investment.id, {
                                                        contributions: { amount: 0, frequency: "Monthly" },
                                                    });
                                                } else {
                                                    updateInvestment(investment.id, { contributions: undefined });
                                                }
                                            }}
                                        />
                                        <Label htmlFor={`contributing-${investment.id}`} className="cursor-pointer">
                                            I'm currently contributing to this account
                                        </Label>
                                    </div>

                                    {investment.contributions && (
                                        <div className="pl-6 space-y-3 border-l-2">
                                            <div className="grid grid-cols-2 gap-4">
                                                <div className="space-y-2">
                                                    <Label>Contribution Amount</Label>
                                                    <MoneyInput
                                                        value={investment.contributions.amount}
                                                        onChange={(value) =>
                                                            updateInvestment(investment.id, {
                                                                contributions: { ...investment.contributions!, amount: value },
                                                            })
                                                        }
                                                        placeholder="500"
                                                    />
                                                </div>
                                                <div className="space-y-2">
                                                    <Label>Frequency</Label>
                                                    <RadioGroup
                                                        value={investment.contributions.frequency}
                                                        onValueChange={(value) =>
                                                            updateInvestment(investment.id, {
                                                                contributions: { ...investment.contributions!, frequency: value as PayFrequency },
                                                            })
                                                        }
                                                    >
                                                        <div className="flex flex-col space-y-1">
                                                            <div className="flex items-center space-x-2">
                                                                <RadioGroupItem value="Monthly" id={`monthly-${investment.id}`} />
                                                                <Label htmlFor={`monthly-${investment.id}`} className="cursor-pointer text-sm">
                                                                    Monthly
                                                                </Label>
                                                            </div>
                                                            <div className="flex items-center space-x-2">
                                                                <RadioGroupItem value="BiWeekly" id={`biweekly-${investment.id}`} />
                                                                <Label htmlFor={`biweekly-${investment.id}`} className="cursor-pointer text-sm">
                                                                    Bi-weekly
                                                                </Label>
                                                            </div>
                                                        </div>
                                                    </RadioGroup>
                                                </div>
                                            </div>

                                            {investment.contributions.amount > 0 && investment.type in IRS_LIMITS_2026 && (
                                                <div className="rounded-lg bg-muted p-2 text-xs">
                                                    {(() => {
                                                        const annualContribution =
                                                            investment.contributions.frequency === "Monthly"
                                                                ? investment.contributions.amount * 12
                                                                : investment.contributions.amount * 26;
                                                        const limit = IRS_LIMITS_2026[investment.type as keyof typeof IRS_LIMITS_2026];

                                                        if (annualContribution > limit) {
                                                            return (
                                                                <p className="text-orange-600">
                                                                    ⚠️ Annual contribution of {formatCurrency(annualContribution)} exceeds 2026 IRS limit of {formatCurrency(limit)}
                                                                </p>
                                                            );
                                                        } else {
                                                            return (
                                                                <p className="text-muted-foreground">
                                                                    Annual contribution: {formatCurrency(annualContribution)} (Limit: {formatCurrency(limit)})
                                                                </p>
                                                            );
                                                        }
                                                    })()}
                                                </div>
                                            )}
                                        </div>
                                    )}
                                </div>

                                <div className="space-y-3">
                                    <div className="flex items-center space-x-2">
                                        <Checkbox
                                            id={`allocation-${investment.id}`}
                                            checked={showAllocation[investment.id] || false}
                                            onCheckedChange={(checked) =>
                                                setShowAllocation((prev) => ({ ...prev, [investment.id]: !!checked }))
                                            }
                                        />
                                        <Label htmlFor={`allocation-${investment.id}`} className="cursor-pointer">
                                            Specify asset allocation
                                        </Label>
                                    </div>

                                    {showAllocation[investment.id] && (
                                        <div className="pl-6 space-y-3 border-l-2">
                                            <p className="text-xs text-muted-foreground">
                                                Enter percentages for each asset class (should total 100%)
                                            </p>
                                            <div className="grid grid-cols-2 gap-3">
                                                <div className="space-y-1">
                                                    <Label className="text-xs">US Stocks</Label>
                                                    <div className="relative">
                                                        <Input
                                                            type="number"
                                                            value={investment.allocation?.stocks || 0}
                                                            onChange={(e) =>
                                                                handleUpdateAllocation(investment.id, "stocks", parseFloat(e.target.value) || 0)
                                                            }
                                                            min="0"
                                                            max="100"
                                                            className="pr-8"
                                                        />
                                                        <span className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-muted-foreground">
                                                            %
                                                        </span>
                                                    </div>
                                                </div>
                                                <div className="space-y-1">
                                                    <Label className="text-xs">Bonds</Label>
                                                    <div className="relative">
                                                        <Input
                                                            type="number"
                                                            value={investment.allocation?.bonds || 0}
                                                            onChange={(e) =>
                                                                handleUpdateAllocation(investment.id, "bonds", parseFloat(e.target.value) || 0)
                                                            }
                                                            min="0"
                                                            max="100"
                                                            className="pr-8"
                                                        />
                                                        <span className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-muted-foreground">
                                                            %
                                                        </span>
                                                    </div>
                                                </div>
                                                <div className="space-y-1">
                                                    <Label className="text-xs">International</Label>
                                                    <div className="relative">
                                                        <Input
                                                            type="number"
                                                            value={investment.allocation?.international || 0}
                                                            onChange={(e) =>
                                                                handleUpdateAllocation(investment.id, "international", parseFloat(e.target.value) || 0)
                                                            }
                                                            min="0"
                                                            max="100"
                                                            className="pr-8"
                                                        />
                                                        <span className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-muted-foreground">
                                                            %
                                                        </span>
                                                    </div>
                                                </div>
                                                <div className="space-y-1">
                                                    <Label className="text-xs">Cash</Label>
                                                    <div className="relative">
                                                        <Input
                                                            type="number"
                                                            value={investment.allocation?.cash || 0}
                                                            onChange={(e) =>
                                                                handleUpdateAllocation(investment.id, "cash", parseFloat(e.target.value) || 0)
                                                            }
                                                            min="0"
                                                            max="100"
                                                            className="pr-8"
                                                        />
                                                        <span className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-muted-foreground">
                                                            %
                                                        </span>
                                                    </div>
                                                </div>
                                            </div>
                                            {(() => {
                                                const total = getAllocationTotal(investment.id);
                                                if (total !== 100 && total > 0) {
                                                    return (
                                                        <p className="text-xs text-orange-600">
                                                            ⚠️ Total is {total}% (should be 100%)
                                                        </p>
                                                    );
                                                }
                                                return null;
                                            })()}
                                        </div>
                                    )}
                                </div>
                            </CardContent>
                        </Card>
                    ))}

                    <Card>
                        <CardHeader>
                            <CardTitle>Add Another Account</CardTitle>
                        </CardHeader>
                        <CardContent>
                            <div className="grid gap-2">
                                {ACCOUNT_TYPES.filter(
                                    (type) => !investments.some((inv) => inv.type === type.value)
                                ).map((accountType) => (
                                    <Button
                                        key={accountType.value}
                                        variant="outline"
                                        className="justify-start h-auto py-2"
                                        onClick={() => handleAddAccount(accountType.value)}
                                    >
                                        <div className="flex flex-col items-start flex-1">
                                            <span className="font-medium text-sm">{accountType.label}</span>
                                            <span className="text-xs text-muted-foreground">{accountType.description}</span>
                                        </div>
                                        <Plus className="h-4 w-4 ml-2" />
                                    </Button>
                                ))}
                            </div>
                        </CardContent>
                    </Card>

                    {investments.length > 0 && (
                        <Card>
                            <CardHeader>
                                <CardTitle className="flex items-center gap-2">
                                    <TrendingUp className="h-5 w-5" />
                                    Investment Summary
                                </CardTitle>
                            </CardHeader>
                            <CardContent>
                                <div className="space-y-2 text-sm">
                                    <div className="flex justify-between items-center py-2 border-b">
                                        <span className="text-muted-foreground">Tax-Deferred (401k, Trad IRA)</span>
                                        <span className="font-medium">{formatCurrency(totalByType.taxDeferred)}</span>
                                    </div>
                                    <div className="flex justify-between items-center py-2 border-b">
                                        <span className="text-muted-foreground">Tax-Free (Roth, HSA)</span>
                                        <span className="font-medium">{formatCurrency(totalByType.taxFree)}</span>
                                    </div>
                                    <div className="flex justify-between items-center py-2 border-b">
                                        <span className="text-muted-foreground">Taxable (Brokerage)</span>
                                        <span className="font-medium">{formatCurrency(totalByType.taxable)}</span>
                                    </div>
                                    <div className="flex justify-between items-center py-2 font-medium">
                                        <span>Total Investments</span>
                                        <span>{formatCurrency(totalByType.taxDeferred + totalByType.taxFree + totalByType.taxable)}</span>
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
