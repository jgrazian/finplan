"use client";

import * as React from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Plus, Trash2 } from "lucide-react";
import {
    SimulationParameters,
    CashFlow,
    RepeatInterval,
    SavedPortfolio,
} from "@/lib/types";

// Format number with commas for display
const formatMoney = (value: number): string => {
    return new Intl.NumberFormat("en-US").format(value);
};

// Parse formatted string back to number
const parseMoney = (value: string): number => {
    return parseFloat(value.replace(/,/g, "")) || 0;
};

// Custom hook for money input formatting
function useMoneyInput(initialValue: number, onChange: (value: number) => void) {
    const [displayValue, setDisplayValue] = React.useState(formatMoney(initialValue));
    const [isFocused, setIsFocused] = React.useState(false);

    React.useEffect(() => {
        if (!isFocused) {
            setDisplayValue(formatMoney(initialValue));
        }
    }, [initialValue, isFocused]);

    const handleFocus = (e: React.FocusEvent<HTMLInputElement>) => {
        setIsFocused(true);
        // Show raw number on focus
        setDisplayValue(initialValue.toString());
        // Select all text for easy replacement
        e.target.select();
    };

    const handleBlur = () => {
        setIsFocused(false);
        const numValue = parseMoney(displayValue);
        onChange(numValue);
        setDisplayValue(formatMoney(numValue));
    };

    const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        setDisplayValue(e.target.value);
    };

    return {
        value: displayValue,
        onChange: handleChange,
        onFocus: handleFocus,
        onBlur: handleBlur,
    };
}

// MoneyInput component for formatted currency inputs
function MoneyInput({
    value,
    onChange,
    ...props
}: {
    value: number;
    onChange: (value: number) => void;
} & Omit<React.ComponentProps<typeof Input>, 'value' | 'onChange' | 'type'>) {
    const moneyProps = useMoneyInput(value, onChange);
    return <Input {...props} {...moneyProps} />;
}

interface StepProps {
    parameters: SimulationParameters;
    updateParameters: <K extends keyof SimulationParameters>(key: K, value: SimulationParameters[K]) => void;
    selectedPortfolio: SavedPortfolio | null;
}

export function CashFlowsStep({ parameters, updateParameters, selectedPortfolio }: StepProps) {
    const [cashFlows, setCashFlows] = React.useState<CashFlow[]>(parameters.cash_flows || []);
    // Use portfolio accounts for names if available, otherwise fall back to parameters.accounts
    const accounts = selectedPortfolio?.accounts || parameters.accounts || [];

    // Build a list of account + asset options
    const accountAssetOptions = React.useMemo(() => {
        const options: { accountId: number; assetId: number; label: string }[] = [];
        accounts.forEach((acc) => {
            acc.assets.forEach((asset) => {
                const accountName = acc.name || `Account #${acc.account_id}`;
                const assetName = asset.name || `Asset #${asset.asset_id}`;
                options.push({
                    accountId: acc.account_id,
                    assetId: asset.asset_id,
                    label: `${accountName} → ${assetName}`,
                });
            });
        });
        return options;
    }, [accounts]);

    const getDefaultAccountAsset = () => {
        if (accountAssetOptions.length > 0) {
            return { accountId: accountAssetOptions[0].accountId, assetId: accountAssetOptions[0].assetId };
        }
        return { accountId: 1, assetId: 100 };
    };

    const addCashFlow = (type: "income" | "expense") => {
        const newId = cashFlows.length > 0 ? Math.max(...cashFlows.map((cf) => cf.cash_flow_id)) + 1 : 1;
        const defaults = getDefaultAccountAsset();
        const newCashFlow: CashFlow = {
            cash_flow_id: newId,
            amount: 0,
            repeats: "Monthly",
            adjust_for_inflation: true,
            direction: type === "income"
                ? { Income: { target_account_id: defaults.accountId, target_asset_id: defaults.assetId } }
                : { Expense: { source_account_id: defaults.accountId, source_asset_id: defaults.assetId } },
            state: "Active",
        };
        const updated = [...cashFlows, newCashFlow];
        setCashFlows(updated);
        updateParameters("cash_flows", updated);
    };

    const updateCashFlow = (index: number, updates: Partial<CashFlow>) => {
        const updated = cashFlows.map((cf, i) => (i === index ? { ...cf, ...updates } : cf));
        setCashFlows(updated);
        updateParameters("cash_flows", updated);
    };

    const removeCashFlow = (index: number) => {
        const updated = cashFlows.filter((_, i) => i !== index);
        setCashFlows(updated);
        updateParameters("cash_flows", updated);
    };

    const isIncome = (cf: CashFlow) => "Income" in cf.direction;

    const REPEAT_OPTIONS: { value: RepeatInterval; label: string }[] = [
        { value: "Never", label: "One-time" },
        { value: "Weekly", label: "Weekly" },
        { value: "BiWeekly", label: "Bi-weekly" },
        { value: "Monthly", label: "Monthly" },
        { value: "Quarterly", label: "Quarterly" },
        { value: "Yearly", label: "Yearly" },
    ];

    return (
        <div className="space-y-6">
            <div className="flex justify-between items-center">
                <p className="text-sm text-muted-foreground">
                    Define your income sources and regular expenses.
                </p>
                <div className="flex gap-2">
                    <Button onClick={() => addCashFlow("income")} size="sm" variant="outline">
                        <Plus className="mr-2 h-4 w-4" />
                        Add Income
                    </Button>
                    <Button onClick={() => addCashFlow("expense")} size="sm" variant="outline">
                        <Plus className="mr-2 h-4 w-4" />
                        Add Expense
                    </Button>
                </div>
            </div>

            {cashFlows.length === 0 ? (
                <Card className="border-dashed">
                    <CardContent className="flex flex-col items-center justify-center py-10">
                        <p className="text-muted-foreground mb-4">No cash flows defined yet</p>
                        <div className="flex gap-2">
                            <Button onClick={() => addCashFlow("income")} variant="outline">
                                <Plus className="mr-2 h-4 w-4" />
                                Add Income
                            </Button>
                            <Button onClick={() => addCashFlow("expense")} variant="outline">
                                <Plus className="mr-2 h-4 w-4" />
                                Add Expense
                            </Button>
                        </div>
                    </CardContent>
                </Card>
            ) : (
                <div className="space-y-4">
                    {cashFlows.map((cf, index) => (
                        <Card key={cf.cash_flow_id}>
                            <CardHeader className="pb-3">
                                <div className="flex justify-between items-start">
                                    <CardTitle className="text-base">
                                        {isIncome(cf) ? "💰 Income" : "💸 Expense"} #{cf.cash_flow_id}
                                    </CardTitle>
                                    <Button variant="ghost" size="icon" onClick={() => removeCashFlow(index)}>
                                        <Trash2 className="h-4 w-4" />
                                    </Button>
                                </div>
                            </CardHeader>
                            <CardContent className="space-y-4">
                                <div className="grid gap-4 md:grid-cols-3">
                                    <div className="space-y-2">
                                        <Label>Amount ($)</Label>
                                        <MoneyInput
                                            value={cf.amount}
                                            onChange={(value) => updateCashFlow(index, { amount: value })}
                                        />
                                    </div>
                                    <div className="space-y-2">
                                        <Label>Frequency</Label>
                                        <Select
                                            value={cf.repeats}
                                            onValueChange={(v) => updateCashFlow(index, { repeats: v as RepeatInterval })}
                                        >
                                            <SelectTrigger>
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                {REPEAT_OPTIONS.map((opt) => (
                                                    <SelectItem key={opt.value} value={opt.value}>
                                                        {opt.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectContent>
                                        </Select>
                                    </div>
                                    <div className="space-y-2">
                                        <Label>Adjust for Inflation</Label>
                                        <Select
                                            value={cf.adjust_for_inflation ? "yes" : "no"}
                                            onValueChange={(v) => updateCashFlow(index, { adjust_for_inflation: v === "yes" })}
                                        >
                                            <SelectTrigger>
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                <SelectItem value="yes">Yes</SelectItem>
                                                <SelectItem value="no">No</SelectItem>
                                            </SelectContent>
                                        </Select>
                                    </div>
                                </div>
                                {isIncome(cf) && accountAssetOptions.length > 0 && (
                                    <div className="space-y-2">
                                        <Label>Deposit to Account / Asset</Label>
                                        <Select
                                            value={
                                                "Income" in cf.direction
                                                    ? `${cf.direction.Income.target_account_id}-${cf.direction.Income.target_asset_id}`
                                                    : accountAssetOptions[0] ? `${accountAssetOptions[0].accountId}-${accountAssetOptions[0].assetId}` : ""
                                            }
                                            onValueChange={(v) => {
                                                const [accountId, assetId] = v.split("-").map(Number);
                                                updateCashFlow(index, {
                                                    direction: { Income: { target_account_id: accountId, target_asset_id: assetId } },
                                                });
                                            }}
                                        >
                                            <SelectTrigger>
                                                <SelectValue placeholder="Select account & asset" />
                                            </SelectTrigger>
                                            <SelectContent>
                                                {accountAssetOptions.map((opt) => (
                                                    <SelectItem key={`${opt.accountId}-${opt.assetId}`} value={`${opt.accountId}-${opt.assetId}`}>
                                                        {opt.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectContent>
                                        </Select>
                                    </div>
                                )}
                                {!isIncome(cf) && accountAssetOptions.length > 0 && (
                                    <div className="space-y-2">
                                        <Label>Withdraw from Account / Asset</Label>
                                        <Select
                                            value={
                                                "Expense" in cf.direction
                                                    ? `${cf.direction.Expense.source_account_id}-${cf.direction.Expense.source_asset_id}`
                                                    : accountAssetOptions[0] ? `${accountAssetOptions[0].accountId}-${accountAssetOptions[0].assetId}` : ""
                                            }
                                            onValueChange={(v) => {
                                                const [accountId, assetId] = v.split("-").map(Number);
                                                updateCashFlow(index, {
                                                    direction: { Expense: { source_account_id: accountId, source_asset_id: assetId } },
                                                });
                                            }}
                                        >
                                            <SelectTrigger>
                                                <SelectValue placeholder="Select account & asset" />
                                            </SelectTrigger>
                                            <SelectContent>
                                                {accountAssetOptions.map((opt) => (
                                                    <SelectItem key={`${opt.accountId}-${opt.assetId}`} value={`${opt.accountId}-${opt.assetId}`}>
                                                        {opt.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectContent>
                                        </Select>
                                    </div>
                                )}
                            </CardContent>
                        </Card>
                    ))}
                </div>
            )}
        </div>
    );
}
