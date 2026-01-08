"use client";

import * as React from "react";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

interface MoneyInputProps {
    value: number;
    onChange: (value: number) => void;
    placeholder?: string;
    className?: string;
    disabled?: boolean;
    showHelper?: boolean;
    helperFrequency?: "year" | "month" | "week";
}

export function MoneyInput({
    value,
    onChange,
    placeholder = "0",
    className,
    disabled,
    showHelper = false,
    helperFrequency = "year",
}: MoneyInputProps) {
    const [displayValue, setDisplayValue] = React.useState("");

    React.useEffect(() => {
        if (value === 0) {
            setDisplayValue("");
        } else {
            setDisplayValue(value.toString());
        }
    }, [value]);

    const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        const input = e.target.value.replace(/[^0-9.]/g, "");
        setDisplayValue(input);

        const numValue = parseFloat(input);
        onChange(isNaN(numValue) ? 0 : numValue);
    };

    const handleBlur = () => {
        if (value > 0) {
            setDisplayValue(value.toString());
        }
    };

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            maximumFractionDigits: 0,
        }).format(amount);

    const getHelperText = () => {
        if (!showHelper || value === 0) return null;

        if (helperFrequency === "month") {
            const monthly = value / 12;
            return `About ${formatCurrency(monthly)} per month`;
        } else if (helperFrequency === "week") {
            const weekly = value / 52;
            return `About ${formatCurrency(weekly)} per week`;
        } else {
            const monthly = value / 12;
            return `That's about ${formatCurrency(monthly)} per month`;
        }
    };

    return (
        <div className="space-y-1">
            <div className="relative">
                <span className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground">
                    $
                </span>
                <Input
                    type="text"
                    value={displayValue}
                    onChange={handleChange}
                    onBlur={handleBlur}
                    placeholder={placeholder}
                    disabled={disabled}
                    className={cn("pl-7", className)}
                />
            </div>
            {getHelperText() && (
                <p className="text-xs text-muted-foreground">{getHelperText()}</p>
            )}
        </div>
    );
}
