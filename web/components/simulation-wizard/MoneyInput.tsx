"use client";

import * as React from "react";
import { Input } from "@/components/ui/input";
import { useMoneyInput } from "./hooks/useMoneyInput";

// MoneyInput component for formatted currency inputs
export function MoneyInput({
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
