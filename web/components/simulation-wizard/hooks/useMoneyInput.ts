"use client";

import * as React from "react";

// Format number with commas for display
export const formatMoney = (value: number): string => {
    return new Intl.NumberFormat("en-US").format(value);
};

// Parse formatted string back to number
export const parseMoney = (value: string): number => {
    return parseFloat(value.replace(/,/g, "")) || 0;
};

// Custom hook for money input formatting
export function useMoneyInput(initialValue: number, onChange: (value: number) => void) {
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
