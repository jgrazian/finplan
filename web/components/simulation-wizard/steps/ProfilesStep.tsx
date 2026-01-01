"use client";

import * as React from "react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Plus, Trash2 } from "lucide-react";
import {
    SimulationParameters,
    InflationProfile,
    ReturnProfile,
    NamedReturnProfile,
    NamedInflationProfile,
    DEFAULT_NAMED_RETURN_PROFILES,
    DEFAULT_NAMED_INFLATION_PROFILES,
} from "@/lib/types";

interface ProfilesStepProps {
    parameters: SimulationParameters;
    updateParameters: <K extends keyof SimulationParameters>(key: K, value: SimulationParameters[K]) => void;
}

export function ProfilesStep({ parameters, updateParameters }: ProfilesStepProps) {
    const [inflationType, setInflationType] = React.useState<string>(() => {
        if (parameters.inflation_profile === "None") return "none";
        if (typeof parameters.inflation_profile === "object") {
            if ("Fixed" in parameters.inflation_profile) return "fixed";
            if ("Normal" in parameters.inflation_profile) return "normal";
        }
        return "normal";
    });

    const [namedProfiles, setNamedProfiles] = React.useState<NamedReturnProfile[]>(
        parameters.named_return_profiles || DEFAULT_NAMED_RETURN_PROFILES
    );

    const [namedInflationProfiles, setNamedInflationProfiles] = React.useState<NamedInflationProfile[]>(
        parameters.named_inflation_profiles || DEFAULT_NAMED_INFLATION_PROFILES
    );

    const getInflationValue = (key: "mean" | "std_dev" | "fixed"): number => {
        const profile = parameters.inflation_profile;
        if (key === "fixed" && typeof profile === "object" && "Fixed" in profile) {
            return Math.round(profile.Fixed * 100 * 10) / 10;
        }
        if (typeof profile === "object" && "Normal" in profile) {
            const value = key === "mean" ? profile.Normal.mean * 100 : profile.Normal.std_dev * 100;
            return Math.round(value * 10) / 10;
        }
        return key === "mean" ? 3.5 : 2.8;
    };

    const updateInflation = (type: string, mean?: number, stdDev?: number, fixed?: number) => {
        let profile: InflationProfile;
        if (type === "none") {
            profile = "None";
        } else if (type === "fixed") {
            profile = { Fixed: (fixed ?? 3.5) / 100 };
        } else {
            profile = { Normal: { mean: (mean ?? 3.5) / 100, std_dev: (stdDev ?? 2.8) / 100 } };
        }
        updateParameters("inflation_profile", profile);
    };

    // Inflation profile helpers
    const addInflationProfile = () => {
        const newProfile: NamedInflationProfile = {
            name: `Inflation ${namedInflationProfiles.length + 1}`,
            profile: { Normal: { mean: 0.035, std_dev: 0.028 } },
        };
        const updated = [...namedInflationProfiles, newProfile];
        setNamedInflationProfiles(updated);
        updateParameters("named_inflation_profiles", updated);
    };

    const updateInflationProfile = (index: number, updates: Partial<NamedInflationProfile>) => {
        const updated = namedInflationProfiles.map((p, i) => (i === index ? { ...p, ...updates } : p));
        setNamedInflationProfiles(updated);
        updateParameters("named_inflation_profiles", updated);
    };

    const removeInflationProfile = (index: number) => {
        const updated = namedInflationProfiles.filter((_, i) => i !== index);
        setNamedInflationProfiles(updated);
        updateParameters("named_inflation_profiles", updated);
    };

    const getInflationProfileValue = (profile: InflationProfile, key: "mean" | "std_dev" | "fixed"): number => {
        if (key === "fixed" && typeof profile === "object" && "Fixed" in profile) {
            return Math.round(profile.Fixed * 100 * 10) / 10;
        }
        if (typeof profile === "object" && "Normal" in profile) {
            const value = key === "mean" ? profile.Normal.mean * 100 : profile.Normal.std_dev * 100;
            return Math.round(value * 10) / 10;
        }
        return key === "mean" ? 3.5 : 2.8;
    };

    const getInflationProfileType = (profile: InflationProfile): string => {
        if (profile === "None") return "none";
        if (typeof profile === "object" && "Fixed" in profile) return "fixed";
        return "normal";
    };

    const updateInflationProfileValues = (index: number, type: string, mean?: number, stdDev?: number, fixed?: number) => {
        let profile: InflationProfile;
        if (type === "none") {
            profile = "None";
        } else if (type === "fixed") {
            profile = { Fixed: (fixed ?? 3.5) / 100 };
        } else {
            profile = { Normal: { mean: (mean ?? 3.5) / 100, std_dev: (stdDev ?? 2.8) / 100 } };
        }
        updateInflationProfile(index, { profile });
    };

    // Return profile helpers
    const addReturnProfile = () => {
        const newProfile: NamedReturnProfile = {
            name: `Profile ${namedProfiles.length + 1}`,
            profile: { Normal: { mean: 0.07, std_dev: 0.12 } },
        };
        const updated = [...namedProfiles, newProfile];
        setNamedProfiles(updated);
        updateParameters("named_return_profiles", updated);
        // Keep return_profiles in sync for backend compatibility
        updateParameters("return_profiles", updated.map((p) => p.profile));
    };

    const updateReturnProfile = (index: number, updates: Partial<NamedReturnProfile>) => {
        const updated = namedProfiles.map((p, i) => (i === index ? { ...p, ...updates } : p));
        setNamedProfiles(updated);
        updateParameters("named_return_profiles", updated);
        updateParameters("return_profiles", updated.map((p) => p.profile));
    };

    const removeReturnProfile = (index: number) => {
        const updated = namedProfiles.filter((_, i) => i !== index);
        setNamedProfiles(updated);
        updateParameters("named_return_profiles", updated);
        updateParameters("return_profiles", updated.map((p) => p.profile));
    };

    const getProfileValue = (profile: ReturnProfile, key: "mean" | "std_dev" | "fixed"): number => {
        if (key === "fixed" && typeof profile === "object" && "Fixed" in profile) {
            return Math.round(profile.Fixed * 100 * 10) / 10;
        }
        if (typeof profile === "object" && "Normal" in profile) {
            const value = key === "mean" ? profile.Normal.mean * 100 : profile.Normal.std_dev * 100;
            return Math.round(value * 10) / 10;
        }
        return key === "mean" ? 7.0 : 12.0;
    };

    const getProfileType = (profile: ReturnProfile): string => {
        if (profile === "None") return "none";
        if (typeof profile === "object" && "Fixed" in profile) return "fixed";
        return "normal";
    };

    const updateProfileValues = (index: number, type: string, mean?: number, stdDev?: number, fixed?: number) => {
        let profile: ReturnProfile;
        if (type === "none") {
            profile = "None";
        } else if (type === "fixed") {
            profile = { Fixed: (fixed ?? 5.0) / 100 };
        } else {
            profile = { Normal: { mean: (mean ?? 7.0) / 100, std_dev: (stdDev ?? 12.0) / 100 } };
        }
        updateReturnProfile(index, { profile });
    };

    return (
        <div className="space-y-8">
            {/* Default Inflation Profile */}
            <div className="space-y-4">
                <h3 className="text-lg font-medium">Default Inflation Profile</h3>
                <p className="text-sm text-muted-foreground">
                    This is the default inflation rate used for cash flows and general adjustments.
                </p>
                <RadioGroup
                    value={inflationType}
                    onValueChange={(v) => {
                        setInflationType(v);
                        updateInflation(v);
                    }}
                    className="grid grid-cols-3 gap-4"
                >
                    <div className="flex items-center space-x-2">
                        <RadioGroupItem value="none" id="inflation-none" />
                        <Label htmlFor="inflation-none">None</Label>
                    </div>
                    <div className="flex items-center space-x-2">
                        <RadioGroupItem value="fixed" id="inflation-fixed" />
                        <Label htmlFor="inflation-fixed">Fixed Rate</Label>
                    </div>
                    <div className="flex items-center space-x-2">
                        <RadioGroupItem value="normal" id="inflation-normal" />
                        <Label htmlFor="inflation-normal">Variable (Normal)</Label>
                    </div>
                </RadioGroup>

                {inflationType === "fixed" && (
                    <div className="space-y-2">
                        <Label>Annual Inflation Rate (%)</Label>
                        <Input
                            type="number"
                            step="0.1"
                            value={getInflationValue("fixed")}
                            onChange={(e) => updateInflation("fixed", undefined, undefined, parseFloat(e.target.value))}
                        />
                    </div>
                )}

                {inflationType === "normal" && (
                    <div className="grid gap-4 md:grid-cols-2">
                        <div className="space-y-2">
                            <Label>Mean (%)</Label>
                            <Input
                                type="number"
                                step="0.1"
                                value={getInflationValue("mean")}
                                onChange={(e) => updateInflation("normal", parseFloat(e.target.value), getInflationValue("std_dev"))}
                            />
                            <p className="text-xs text-muted-foreground">US historical: ~3.5%</p>
                        </div>
                        <div className="space-y-2">
                            <Label>Standard Deviation (%)</Label>
                            <Input
                                type="number"
                                step="0.1"
                                value={getInflationValue("std_dev")}
                                onChange={(e) => updateInflation("normal", getInflationValue("mean"), parseFloat(e.target.value))}
                            />
                            <p className="text-xs text-muted-foreground">US historical: ~2.8%</p>
                        </div>
                    </div>
                )}
            </div>

            {/* Named Inflation Profiles */}
            <div className="space-y-4">
                <div className="flex justify-between items-center">
                    <div>
                        <h3 className="text-lg font-medium">Inflation Profiles</h3>
                        <p className="text-sm text-muted-foreground">
                            Define different inflation profiles for various asset categories (e.g., healthcare, housing).
                            Assets can be linked to these in the next step.
                        </p>
                    </div>
                    <Button onClick={addInflationProfile} size="sm" variant="outline">
                        <Plus className="mr-2 h-4 w-4" />
                        Add Inflation Profile
                    </Button>
                </div>

                {namedInflationProfiles.length === 0 ? (
                    <Card className="border-dashed">
                        <CardContent className="flex flex-col items-center justify-center py-6">
                            <p className="text-muted-foreground mb-4">No custom inflation profiles defined</p>
                            <Button onClick={addInflationProfile} variant="outline" size="sm">
                                <Plus className="mr-2 h-4 w-4" />
                                Add Inflation Profile
                            </Button>
                        </CardContent>
                    </Card>
                ) : (
                    <div className="space-y-3">
                        {namedInflationProfiles.map((namedProfile, index) => {
                            const profileType = getInflationProfileType(namedProfile.profile);
                            return (
                                <Card key={index} className="p-4">
                                    <div className="flex justify-between items-start mb-3">
                                        <Input
                                            value={namedProfile.name}
                                            onChange={(e) => updateInflationProfile(index, { name: e.target.value })}
                                            className="font-medium max-w-xs"
                                            placeholder="Profile Name"
                                        />
                                        <Button
                                            variant="ghost"
                                            size="icon"
                                            onClick={() => removeInflationProfile(index)}
                                        >
                                            <Trash2 className="h-4 w-4" />
                                        </Button>
                                    </div>
                                    <div className="grid gap-3 md:grid-cols-3">
                                        <Select
                                            value={profileType}
                                            onValueChange={(v) => updateInflationProfileValues(index, v)}
                                        >
                                            <SelectTrigger className="h-8">
                                                <SelectValue />
                                            </SelectTrigger>
                                            <SelectContent>
                                                <SelectItem value="none">None</SelectItem>
                                                <SelectItem value="fixed">Fixed</SelectItem>
                                                <SelectItem value="normal">Variable</SelectItem>
                                            </SelectContent>
                                        </Select>
                                        {profileType === "fixed" && (
                                            <div className="flex items-center gap-2">
                                                <Label className="text-xs whitespace-nowrap">Rate (%)</Label>
                                                <Input
                                                    type="number"
                                                    step="0.1"
                                                    value={getInflationProfileValue(namedProfile.profile, "fixed")}
                                                    onChange={(e) => updateInflationProfileValues(index, "fixed", undefined, undefined, parseFloat(e.target.value))}
                                                    className="h-8"
                                                />
                                            </div>
                                        )}
                                        {profileType === "normal" && (
                                            <>
                                                <div className="flex items-center gap-2">
                                                    <Label className="text-xs whitespace-nowrap">Mean (%)</Label>
                                                    <Input
                                                        type="number"
                                                        step="0.1"
                                                        value={getInflationProfileValue(namedProfile.profile, "mean")}
                                                        onChange={(e) => updateInflationProfileValues(index, "normal", parseFloat(e.target.value), getInflationProfileValue(namedProfile.profile, "std_dev"))}
                                                        className="h-8"
                                                    />
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <Label className="text-xs whitespace-nowrap">Std Dev (%)</Label>
                                                    <Input
                                                        type="number"
                                                        step="0.1"
                                                        value={getInflationProfileValue(namedProfile.profile, "std_dev")}
                                                        onChange={(e) => updateInflationProfileValues(index, "normal", getInflationProfileValue(namedProfile.profile, "mean"), parseFloat(e.target.value))}
                                                        className="h-8"
                                                    />
                                                </div>
                                            </>
                                        )}
                                    </div>
                                </Card>
                            );
                        })}
                    </div>
                )}
            </div>

            {/* Named Return Profiles */}
            <div className="space-y-4">
                <div className="flex justify-between items-center">
                    <div>
                        <h3 className="text-lg font-medium">Return Profiles</h3>
                        <p className="text-sm text-muted-foreground">
                            Define return profiles that can be assigned to assets in your accounts.
                        </p>
                    </div>
                    <Button onClick={addReturnProfile} size="sm" variant="outline">
                        <Plus className="mr-2 h-4 w-4" />
                        Add Profile
                    </Button>
                </div>

                {namedProfiles.length === 0 ? (
                    <Card className="border-dashed">
                        <CardContent className="flex flex-col items-center justify-center py-8">
                            <p className="text-muted-foreground mb-4">No return profiles defined</p>
                            <Button onClick={addReturnProfile} variant="outline">
                                <Plus className="mr-2 h-4 w-4" />
                                Add Return Profile
                            </Button>
                        </CardContent>
                    </Card>
                ) : (
                    <div className="space-y-4">
                        {namedProfiles.map((namedProfile, index) => {
                            const profileType = getProfileType(namedProfile.profile);
                            return (
                                <Card key={index}>
                                    <CardHeader className="pb-3">
                                        <div className="flex justify-between items-start">
                                            <div className="flex-1 mr-4">
                                                <Input
                                                    value={namedProfile.name}
                                                    onChange={(e) => updateReturnProfile(index, { name: e.target.value })}
                                                    className="font-medium"
                                                    placeholder="Profile Name"
                                                />
                                            </div>
                                            <Button
                                                variant="ghost"
                                                size="icon"
                                                onClick={() => removeReturnProfile(index)}
                                                disabled={namedProfiles.length === 1}
                                            >
                                                <Trash2 className="h-4 w-4" />
                                            </Button>
                                        </div>
                                    </CardHeader>
                                    <CardContent className="space-y-4">
                                        <RadioGroup
                                            value={profileType}
                                            onValueChange={(v) => updateProfileValues(index, v)}
                                            className="grid grid-cols-3 gap-4"
                                        >
                                            <div className="flex items-center space-x-2">
                                                <RadioGroupItem value="none" id={`return-none-${index}`} />
                                                <Label htmlFor={`return-none-${index}`}>None</Label>
                                            </div>
                                            <div className="flex items-center space-x-2">
                                                <RadioGroupItem value="fixed" id={`return-fixed-${index}`} />
                                                <Label htmlFor={`return-fixed-${index}`}>Fixed Rate</Label>
                                            </div>
                                            <div className="flex items-center space-x-2">
                                                <RadioGroupItem value="normal" id={`return-normal-${index}`} />
                                                <Label htmlFor={`return-normal-${index}`}>Variable (Normal)</Label>
                                            </div>
                                        </RadioGroup>

                                        {profileType === "fixed" && (
                                            <div className="space-y-2">
                                                <Label>Annual Return Rate (%)</Label>
                                                <Input
                                                    type="number"
                                                    step="0.1"
                                                    value={getProfileValue(namedProfile.profile, "fixed")}
                                                    onChange={(e) => updateProfileValues(index, "fixed", undefined, undefined, parseFloat(e.target.value))}
                                                />
                                            </div>
                                        )}

                                        {profileType === "normal" && (
                                            <div className="grid gap-4 md:grid-cols-2">
                                                <div className="space-y-2">
                                                    <Label>Mean (%)</Label>
                                                    <Input
                                                        type="number"
                                                        step="0.1"
                                                        value={getProfileValue(namedProfile.profile, "mean")}
                                                        onChange={(e) => updateProfileValues(index, "normal", parseFloat(e.target.value), getProfileValue(namedProfile.profile, "std_dev"))}
                                                    />
                                                </div>
                                                <div className="space-y-2">
                                                    <Label>Standard Deviation (%)</Label>
                                                    <Input
                                                        type="number"
                                                        step="0.1"
                                                        value={getProfileValue(namedProfile.profile, "std_dev")}
                                                        onChange={(e) => updateProfileValues(index, "normal", getProfileValue(namedProfile.profile, "mean"), parseFloat(e.target.value))}
                                                    />
                                                </div>
                                            </div>
                                        )}
                                    </CardContent>
                                </Card>
                            );
                        })}
                    </div>
                )}
            </div>
        </div>
    );
}
