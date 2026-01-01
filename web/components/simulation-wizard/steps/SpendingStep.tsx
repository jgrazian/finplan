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
    SpendingTarget,
    RepeatInterval,
    WithdrawalStrategy,
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
}

export function SpendingStep({ parameters, updateParameters }: StepProps) {
    const [targets, setTargets] = React.useState<SpendingTarget[]>(parameters.spending_targets || []);

    const addTarget = () => {
        const newId = targets.length > 0 ? Math.max(...targets.map((t) => t.spending_target_id)) + 1 : 1;
        const newTarget: SpendingTarget = {
            spending_target_id: newId,
            amount: 0,
            net_amount_mode: false,
            repeats: "Monthly",
            adjust_for_inflation: true,
            withdrawal_strategy: "TaxOptimized",
            exclude_accounts: [],
            state: "Pending",
        };
        const updated = [...targets, newTarget];
        setTargets(updated);
        updateParameters("spending_targets", updated);
    };

    const updateTarget = (index: number, updates: Partial<SpendingTarget>) => {
        const updated = targets.map((t, i) => (i === index ? { ...t, ...updates } : t));
        setTargets(updated);
        updateParameters("spending_targets", updated);
    };

    const removeTarget = (index: number) => {
        const updated = targets.filter((_, i) => i !== index);
        setTargets(updated);
        updateParameters("spending_targets", updated);
    };

    const STRATEGY_OPTIONS: { value: string; label: string; description: string }[] = [
        { value: "TaxOptimized", label: "Tax Optimized", description: "Minimize taxes by withdrawing from taxable accounts first" },
        { value: "ProRata", label: "Pro-Rata", description: "Withdraw proportionally from all accounts" },
    ];

    return (
        <div className="space-y-6">
            <div className="flex justify-between items-center">
                <p className="text-sm text-muted-foreground">
                    Define spending targets for retirement. These determine how much you&apos;ll withdraw from accounts.
                </p>
                <Button onClick={addTarget} size="sm">
                    <Plus className="mr-2 h-4 w-4" />
                    Add Spending Target
                </Button>
            </div>

            {targets.length === 0 ? (
                <Card className="border-dashed">
                    <CardContent className="flex flex-col items-center justify-center py-10">
                        <p className="text-muted-foreground mb-4">No spending targets defined</p>
                        <p className="text-xs text-muted-foreground mb-4">
                            Spending targets are used during retirement to model withdrawals from your accounts.
                        </p>
                        <Button onClick={addTarget} variant="outline">
                            <Plus className="mr-2 h-4 w-4" />
                            Add Spending Target
                        </Button>
                    </CardContent>
                </Card>
            ) : (
                <div className="space-y-4">
                    {targets.map((target, index) => (
                        <Card key={target.spending_target_id}>
                            <CardHeader className="pb-3">
                                <div className="flex justify-between items-start">
                                    <CardTitle className="text-base">
                                        Spending Target #{target.spending_target_id}
                                    </CardTitle>
                                    <Button variant="ghost" size="icon" onClick={() => removeTarget(index)}>
                                        <Trash2 className="h-4 w-4" />
                                    </Button>
                                </div>
                            </CardHeader>
                            <CardContent className="space-y-4">
                                <div className="grid gap-4 md:grid-cols-3">
                                    <div className="space-y-2">
                                        <Label>Amount ($)</Label>
                                        <MoneyInput
                                            value={target.amount}
                                            onChange={(value) => updateTarget(index, { amount: value })}
                                        />
                                    </div>
                                    <div className="space-y-2">
                                        <Label>Frequency</Label>
                                        <Select
                                            value={target.repeats}
                                            onValueChange={(v) => updateTarget(index, { repeats: v as RepeatInterval })}
                                        >
                                            <SelectTrigger>
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                <SelectItem value="Monthly">Monthly</SelectItem>
                                                <SelectItem value="Yearly">Yearly</SelectItem>
                                            </SelectContent>
                                        </Select>
                                    </div>
                                    <div className="space-y-2">
                                        <Label>Withdrawal Strategy</Label>
                                        <Select
                                            value={typeof target.withdrawal_strategy === "string" ? target.withdrawal_strategy : "Sequential"}
                                            onValueChange={(v) => updateTarget(index, { withdrawal_strategy: v as WithdrawalStrategy })}
                                        >
                                            <SelectTrigger>
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                {STRATEGY_OPTIONS.map((opt) => (
                                                    <SelectItem key={opt.value} value={opt.value}>
                                                        {opt.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectContent>
                                        </Select>
                                    </div>
                                </div>
                                <div className="flex items-center gap-4">
                                    <div className="flex items-center space-x-2">
                                        <input
                                            type="checkbox"
                                            id={`inflation-${target.spending_target_id}`}
                                            checked={target.adjust_for_inflation}
                                            onChange={(e) => updateTarget(index, { adjust_for_inflation: e.target.checked })}
                                            className="rounded border-input"
                                        />
                                        <Label htmlFor={`inflation-${target.spending_target_id}`}>Adjust for inflation</Label>
                                    </div>
                                    <div className="flex items-center space-x-2">
                                        <input
                                            type="checkbox"
                                            id={`net-${target.spending_target_id}`}
                                            checked={target.net_amount_mode}
                                            onChange={(e) => updateTarget(index, { net_amount_mode: e.target.checked })}
                                            className="rounded border-input"
                                        />
                                        <Label htmlFor={`net-${target.spending_target_id}`}>Net amount (after taxes)</Label>
                                    </div>
                                </div>
                            </CardContent>
                        </Card>
                    ))}
                </div>
            )}
        </div>
    );
}
