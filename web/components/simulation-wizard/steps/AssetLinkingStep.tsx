"use client";

import * as React from "react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import {
    SimulationParameters,
    AssetInflationMapping,
    SavedPortfolio,
    DEFAULT_NAMED_RETURN_PROFILES,
    DEFAULT_NAMED_INFLATION_PROFILES,
} from "@/lib/types";

interface StepProps {
    parameters: SimulationParameters;
    updateParameters: <K extends keyof SimulationParameters>(key: K, value: SimulationParameters[K]) => void;
}

export function AssetLinkingStep({
    parameters,
    updateParameters,
    selectedPortfolio,
}: StepProps & { selectedPortfolio: SavedPortfolio | null }) {
    const namedReturnProfiles = parameters.named_return_profiles || DEFAULT_NAMED_RETURN_PROFILES;
    const namedInflationProfiles = parameters.named_inflation_profiles || DEFAULT_NAMED_INFLATION_PROFILES;
    const accounts = parameters.accounts || [];

    const [assetMappings, setAssetMappings] = React.useState<AssetInflationMapping[]>(
        parameters.asset_inflation_mappings || []
    );

    const getAssetInflationIndex = (accountId: number, assetId: number): number => {
        const mapping = assetMappings.find(m => m.account_id === accountId && m.asset_id === assetId);
        return mapping?.inflation_profile_index ?? 0;
    };

    const updateAssetInflationMapping = (accountId: number, assetId: number, inflationIndex: number) => {
        const existingIndex = assetMappings.findIndex(m => m.account_id === accountId && m.asset_id === assetId);
        let newMappings: AssetInflationMapping[];

        if (existingIndex >= 0) {
            newMappings = assetMappings.map((m, i) =>
                i === existingIndex ? { ...m, inflation_profile_index: inflationIndex } : m
            );
        } else {
            newMappings = [...assetMappings, { account_id: accountId, asset_id: assetId, inflation_profile_index: inflationIndex }];
        }

        setAssetMappings(newMappings);
        updateParameters("asset_inflation_mappings", newMappings);
    };

    const updateAssetReturnProfile = (accountIndex: number, assetIndex: number, returnIndex: number) => {
        const newAccounts = accounts.map((acc, i) => {
            if (i !== accountIndex) return acc;
            const newAssets = acc.assets.map((asset, j) =>
                j === assetIndex ? { ...asset, return_profile_index: returnIndex } : asset
            );
            return { ...acc, assets: newAssets };
        });
        updateParameters("accounts", newAccounts);
    };

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", { style: "currency", currency: "USD", maximumFractionDigits: 0 }).format(amount);

    if (!selectedPortfolio && accounts.length === 0) {
        return (
            <div className="space-y-6">
                <Card className="border-dashed">
                    <CardContent className="flex flex-col items-center justify-center py-10">
                        <p className="text-muted-foreground mb-4">No portfolio selected</p>
                        <p className="text-sm text-muted-foreground text-center max-w-md">
                            Go back to the Basics step to select a portfolio, or create one first.
                        </p>
                    </CardContent>
                </Card>
            </div>
        );
    }

    return (
        <div className="space-y-6">
            <div className="bg-muted/50 rounded-lg p-4">
                <h3 className="font-medium mb-2">Link Assets to Profiles</h3>
                <p className="text-sm text-muted-foreground">
                    Assign return and inflation profiles to each asset in your portfolio.
                    This determines how each asset grows and how inflation affects its real value.
                </p>
            </div>

            {accounts.map((account, accountIndex) => (
                <Card key={account.account_id}>
                    <CardHeader className="pb-2">
                        <CardTitle className="text-base">
                            {account.name || `Account #${account.account_id}`}
                            <span className="text-sm font-normal text-muted-foreground ml-2">
                                ({account.account_type})
                            </span>
                        </CardTitle>
                        <p className="text-sm text-muted-foreground">
                            {formatCurrency(account.assets.reduce((s, a) => s + a.initial_value, 0))}
                        </p>
                    </CardHeader>
                    <CardContent>
                        {account.assets.length === 0 ? (
                            <p className="text-sm text-muted-foreground">No assets in this account</p>
                        ) : (
                            <div className="space-y-3">
                                {account.assets.map((asset, assetIndex) => (
                                    <div
                                        key={asset.asset_id}
                                        className="grid gap-3 md:grid-cols-4 items-center p-3 bg-muted/30 rounded-lg"
                                    >
                                        <div>
                                            <p className="font-medium text-sm">{asset.name || `Asset #${asset.asset_id}`}</p>
                                            <p className="text-xs text-muted-foreground">
                                                {formatCurrency(asset.initial_value)} • {asset.asset_class}
                                            </p>
                                        </div>
                                        <div className="space-y-1">
                                            <Label className="text-xs">Return Profile</Label>
                                            <Select
                                                value={asset.return_profile_index.toString()}
                                                onValueChange={(v) => updateAssetReturnProfile(accountIndex, assetIndex, parseInt(v))}
                                            >
                                                <SelectTrigger className="h-8">
                                                    <SelectValue />
                                                </SelectTrigger>
                                                <SelectContent>
                                                    {namedReturnProfiles.map((profile, idx) => (
                                                        <SelectItem key={idx} value={idx.toString()}>
                                                            {profile.name}
                                                        </SelectItem>
                                                    ))}
                                                </SelectContent>
                                            </Select>
                                        </div>
                                        <div className="space-y-1">
                                            <Label className="text-xs">Inflation Profile</Label>
                                            <Select
                                                value={getAssetInflationIndex(account.account_id, asset.asset_id).toString()}
                                                onValueChange={(v) => updateAssetInflationMapping(account.account_id, asset.asset_id, parseInt(v))}
                                            >
                                                <SelectTrigger className="h-8">
                                                    <SelectValue />
                                                </SelectTrigger>
                                                <SelectContent>
                                                    <SelectItem value="0">Default (General CPI)</SelectItem>
                                                    {namedInflationProfiles.map((profile, idx) => (
                                                        <SelectItem key={idx} value={(idx + 1).toString()}>
                                                            {profile.name}
                                                        </SelectItem>
                                                    ))}
                                                </SelectContent>
                                            </Select>
                                        </div>
                                        <div className="text-right text-xs text-muted-foreground">
                                            ID: {asset.asset_id}
                                        </div>
                                    </div>
                                ))}
                            </div>
                        )}
                    </CardContent>
                </Card>
            ))}
        </div>
    );
}
