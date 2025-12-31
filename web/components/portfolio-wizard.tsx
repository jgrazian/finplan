"use client";

import * as React from "react";
import { useRouter } from "next/navigation";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Plus, Trash2, Save, Wallet, Building2, Car, CreditCard } from "lucide-react";
import {
    Account,
    Asset,
    AccountType,
    AssetClass,
} from "@/lib/types";
import { createPortfolio, updatePortfolio } from "@/lib/api";

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
        setDisplayValue(initialValue.toString());
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

interface PortfolioWizardProps {
    initialData?: {
        id?: string;
        name?: string;
        description?: string;
        accounts?: Account[];
    };
    onComplete?: (portfolio: { id: string }) => void;
}

const ACCOUNT_TYPES: { value: AccountType; label: string; description: string; icon: React.ReactNode }[] = [
    { value: "Taxable", label: "Taxable", description: "Brokerage account", icon: <Wallet className="h-4 w-4" /> },
    { value: "TaxDeferred", label: "Tax-Deferred", description: "401(k), Traditional IRA", icon: <Building2 className="h-4 w-4" /> },
    { value: "TaxFree", label: "Tax-Free", description: "Roth IRA, Roth 401(k)", icon: <Building2 className="h-4 w-4" /> },
    { value: "Illiquid", label: "Illiquid", description: "Real estate, vehicles", icon: <Car className="h-4 w-4" /> },
];

const ASSET_CLASSES: { value: AssetClass; label: string; icon: React.ReactNode }[] = [
    { value: "Investable", label: "Investable", icon: <Wallet className="h-4 w-4" /> },
    { value: "RealEstate", label: "Real Estate", icon: <Building2 className="h-4 w-4" /> },
    { value: "Depreciating", label: "Depreciating", icon: <Car className="h-4 w-4" /> },
    { value: "Liability", label: "Liability (Debt)", icon: <CreditCard className="h-4 w-4" /> },
];

export function PortfolioWizard({ initialData, onComplete }: PortfolioWizardProps) {
    const router = useRouter();
    const [isSubmitting, setIsSubmitting] = React.useState(false);
    const [name, setName] = React.useState(initialData?.name || "");
    const [description, setDescription] = React.useState(initialData?.description || "");
    const [accounts, setAccounts] = React.useState<Account[]>(initialData?.accounts || []);

    const addAccount = () => {
        const newId = accounts.length > 0 ? Math.max(...accounts.map((a) => a.account_id)) + 1 : 1;
        const newAccount: Account = {
            account_id: newId,
            name: `Account ${newId}`,
            account_type: "Taxable",
            assets: [],
        };
        setAccounts([...accounts, newAccount]);
    };

    const updateAccount = (index: number, updates: Partial<Account>) => {
        setAccounts(accounts.map((acc, i) => (i === index ? { ...acc, ...updates } : acc)));
    };

    const removeAccount = (index: number) => {
        setAccounts(accounts.filter((_, i) => i !== index));
    };

    const addAsset = (accountIndex: number) => {
        const account = accounts[accountIndex];
        const newAssetId = account.assets.length > 0
            ? Math.max(...account.assets.map((a) => a.asset_id)) + 1
            : account.account_id * 100;
        const newAsset: Asset = {
            asset_id: newAssetId,
            name: `Asset ${account.assets.length + 1}`,
            asset_class: "Investable",
            initial_value: 0,
            return_profile_index: 0,
        };
        updateAccount(accountIndex, { assets: [...account.assets, newAsset] });
    };

    const updateAsset = (accountIndex: number, assetIndex: number, updates: Partial<Asset>) => {
        const newAccounts = accounts.map((acc, i) => {
            if (i !== accountIndex) return acc;
            const newAssets = acc.assets.map((asset, j) =>
                j === assetIndex ? { ...asset, ...updates } : asset
            );
            return { ...acc, assets: newAssets };
        });
        setAccounts(newAccounts);
    };

    const removeAsset = (accountIndex: number, assetIndex: number) => {
        const newAccounts = accounts.map((acc, i) => {
            if (i !== accountIndex) return acc;
            return { ...acc, assets: acc.assets.filter((_, j) => j !== assetIndex) };
        });
        setAccounts(newAccounts);
    };

    const totalValue = accounts.reduce(
        (sum, acc) => sum + acc.assets.reduce((s, a) => s + (a.asset_class === "Liability" ? -a.initial_value : a.initial_value), 0),
        0
    );

    const handleSave = async () => {
        setIsSubmitting(true);
        try {
            let result;
            if (initialData?.id) {
                result = await updatePortfolio(initialData.id, { name, description, accounts });
            } else {
                result = await createPortfolio({ name, description, accounts });
            }
            onComplete?.(result);
            router.push(`/portfolios/${result.id}`);
        } catch (error) {
            console.error("Failed to save portfolio:", error);
        } finally {
            setIsSubmitting(false);
        }
    };

    return (
        <div className="space-y-6">
            {/* Header Info */}
            <Card>
                <CardHeader>
                    <CardTitle>Portfolio Details</CardTitle>
                    <CardDescription>Define your portfolio with accounts and assets</CardDescription>
                </CardHeader>
                <CardContent className="space-y-4">
                    <div className="grid gap-4 md:grid-cols-2">
                        <div className="space-y-2">
                            <Label htmlFor="name">Portfolio Name *</Label>
                            <Input
                                id="name"
                                placeholder="My Investment Portfolio"
                                value={name}
                                onChange={(e) => setName(e.target.value)}
                            />
                        </div>
                        <div className="space-y-2">
                            <Label>Total Net Worth</Label>
                            <div className="text-2xl font-bold">
                                ${formatMoney(totalValue)}
                            </div>
                        </div>
                    </div>
                    <div className="space-y-2">
                        <Label htmlFor="description">Description</Label>
                        <Textarea
                            id="description"
                            placeholder="Describe your portfolio..."
                            value={description}
                            onChange={(e) => setDescription(e.target.value)}
                            rows={2}
                        />
                    </div>
                </CardContent>
            </Card>

            {/* Accounts Section */}
            <div className="space-y-4">
                <div className="flex justify-between items-center">
                    <div>
                        <h2 className="text-lg font-semibold">Accounts</h2>
                        <p className="text-sm text-muted-foreground">
                            Add your financial accounts and their holdings
                        </p>
                    </div>
                    <Button onClick={addAccount}>
                        <Plus className="mr-2 h-4 w-4" />
                        Add Account
                    </Button>
                </div>

                {accounts.length === 0 ? (
                    <Card className="border-dashed">
                        <CardContent className="flex flex-col items-center justify-center py-10">
                            <Wallet className="h-12 w-12 text-muted-foreground mb-4" />
                            <p className="text-muted-foreground mb-4">No accounts added yet</p>
                            <Button onClick={addAccount} variant="outline">
                                <Plus className="mr-2 h-4 w-4" />
                                Add Your First Account
                            </Button>
                        </CardContent>
                    </Card>
                ) : (
                    <div className="space-y-4">
                        {accounts.map((account, accountIndex) => (
                            <Card key={account.account_id}>
                                <CardHeader className="pb-3">
                                    <div className="flex justify-between items-start">
                                        <div className="flex-1 space-y-2">
                                            <div className="flex items-center gap-4">
                                                <Input
                                                    value={account.name || `Account ${account.account_id}`}
                                                    onChange={(e) => updateAccount(accountIndex, { name: e.target.value })}
                                                    className="font-medium max-w-xs"
                                                    placeholder="Account Name"
                                                />
                                                <Select
                                                    value={account.account_type}
                                                    onValueChange={(v) => updateAccount(accountIndex, { account_type: v as AccountType })}
                                                >
                                                    <SelectTrigger className="w-[180px]">
                                                        <SelectValue />
                                                    </SelectTrigger>
                                                    <SelectContent>
                                                        {ACCOUNT_TYPES.map((type) => (
                                                            <SelectItem key={type.value} value={type.value}>
                                                                <div className="flex items-center gap-2">
                                                                    {type.icon}
                                                                    <span>{type.label}</span>
                                                                </div>
                                                            </SelectItem>
                                                        ))}
                                                    </SelectContent>
                                                </Select>
                                            </div>
                                            <p className="text-sm text-muted-foreground">
                                                Balance: ${formatMoney(account.assets.reduce((s, a) => s + (a.asset_class === "Liability" ? -a.initial_value : a.initial_value), 0))}
                                            </p>
                                        </div>
                                        <Button
                                            variant="ghost"
                                            size="icon"
                                            onClick={() => removeAccount(accountIndex)}
                                        >
                                            <Trash2 className="h-4 w-4" />
                                        </Button>
                                    </div>
                                </CardHeader>
                                <CardContent className="space-y-4">
                                    <div className="flex justify-between items-center">
                                        <Label className="text-sm font-medium">Assets / Holdings</Label>
                                        <Button onClick={() => addAsset(accountIndex)} size="sm" variant="outline">
                                            <Plus className="mr-2 h-3 w-3" />
                                            Add Asset
                                        </Button>
                                    </div>

                                    {account.assets.length === 0 ? (
                                        <div className="border-2 border-dashed rounded-lg p-4 text-center">
                                            <p className="text-sm text-muted-foreground mb-2">No assets in this account</p>
                                            <Button onClick={() => addAsset(accountIndex)} size="sm" variant="ghost">
                                                <Plus className="mr-2 h-3 w-3" />
                                                Add First Asset
                                            </Button>
                                        </div>
                                    ) : (
                                        <div className="space-y-3">
                                            {account.assets.map((asset, assetIndex) => (
                                                <div
                                                    key={asset.asset_id}
                                                    className="border rounded-lg p-3 space-y-3 bg-muted/30"
                                                >
                                                    <div className="flex items-center gap-2">
                                                        <Input
                                                            value={asset.name || `Asset ${assetIndex + 1}`}
                                                            onChange={(e) => updateAsset(accountIndex, assetIndex, { name: e.target.value })}
                                                            className="flex-1 h-8"
                                                            placeholder="Asset Name (e.g., VTSAX, House)"
                                                        />
                                                        <Button
                                                            variant="ghost"
                                                            size="icon"
                                                            className="h-8 w-8"
                                                            onClick={() => removeAsset(accountIndex, assetIndex)}
                                                        >
                                                            <Trash2 className="h-3 w-3" />
                                                        </Button>
                                                    </div>
                                                    <div className="grid gap-3 md:grid-cols-2">
                                                        <div className="space-y-1">
                                                            <Label className="text-xs">Current Value ($)</Label>
                                                            <MoneyInput
                                                                value={asset.initial_value}
                                                                onChange={(value) => updateAsset(accountIndex, assetIndex, { initial_value: value })}
                                                                className="h-8"
                                                            />
                                                        </div>
                                                        <div className="space-y-1">
                                                            <Label className="text-xs">Asset Class</Label>
                                                            <Select
                                                                value={asset.asset_class}
                                                                onValueChange={(v) => updateAsset(accountIndex, assetIndex, { asset_class: v as AssetClass })}
                                                            >
                                                                <SelectTrigger className="h-8">
                                                                    <SelectValue />
                                                                </SelectTrigger>
                                                                <SelectContent>
                                                                    {ASSET_CLASSES.map((ac) => (
                                                                        <SelectItem key={ac.value} value={ac.value}>
                                                                            <div className="flex items-center gap-2">
                                                                                {ac.icon}
                                                                                <span>{ac.label}</span>
                                                                            </div>
                                                                        </SelectItem>
                                                                    ))}
                                                                </SelectContent>
                                                            </Select>
                                                        </div>
                                                    </div>
                                                </div>
                                            ))}
                                        </div>
                                    )}
                                </CardContent>
                            </Card>
                        ))}
                    </div>
                )}
            </div>

            {/* Save Button */}
            <div className="flex justify-end gap-2">
                <Button variant="outline" onClick={() => router.back()}>
                    Cancel
                </Button>
                <Button onClick={handleSave} disabled={isSubmitting || !name}>
                    <Save className="mr-2 h-4 w-4" />
                    {isSubmitting ? "Saving..." : initialData?.id ? "Update Portfolio" : "Create Portfolio"}
                </Button>
            </div>
        </div>
    );
}
