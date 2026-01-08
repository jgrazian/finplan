"use client";

import * as React from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import { Plus, Trash2, Home } from "lucide-react";
import { useWizardStore } from "../hooks/useWizardStore";
import { MoneyInput } from "../components/MoneyInput";
import { RealEstateProperty } from "../types";

const PROPERTY_TYPES: { value: RealEstateProperty["type"]; label: string; description: string }[] = [
    { value: "PrimaryResidence", label: "Primary Residence", description: "Your home" },
    { value: "Investment", label: "Investment/Rental Property", description: "Property you rent out" },
    { value: "Vacation", label: "Vacation Home", description: "Second home or vacation property" },
];

export function RealEstateStep() {
    const realEstate = useWizardStore((state) => state.realEstate);
    const addRealEstate = useWizardStore((state) => state.addRealEstate);
    const updateRealEstate = useWizardStore((state) => state.updateRealEstate);
    const removeRealEstate = useWizardStore((state) => state.removeRealEstate);
    const currentAge = useWizardStore((state) => {
        if (!state.personalInfo.birthDate) return null;
        const today = new Date();
        const birthDate = new Date(state.personalInfo.birthDate);
        let age = today.getFullYear() - birthDate.getFullYear();
        const monthDiff = today.getMonth() - birthDate.getMonth();
        if (monthDiff < 0 || (monthDiff === 0 && today.getDate() < birthDate.getDate())) {
            age--;
        }
        return age;
    });

    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat("en-US", {
            style: "currency",
            currency: "USD",
            maximumFractionDigits: 0,
        }).format(amount);

    const handleAddProperty = (type: RealEstateProperty["type"]) => {
        const id = `prop-${Date.now()}`;
        addRealEstate({
            id,
            type,
            value: 0,
        });
    };

    const calculateMonthlyHousingCost = (property: RealEstateProperty) => {
        let total = 0;

        if (property.mortgage) {
            total += property.mortgage.monthlyPayment;
        }

        if (property.propertyTax && !property.mortgage?.includesPropertyTax) {
            total += property.propertyTax / 12;
        }

        if (property.insurance && !property.mortgage?.includesInsurance) {
            total += property.insurance / 12;
        }

        return total;
    };

    const calculateEquity = (property: RealEstateProperty) => {
        const mortgageBalance = property.mortgage?.balance || 0;
        return property.value - mortgageBalance;
    };

    const totalValue = realEstate.reduce((sum, prop) => sum + prop.value, 0);
    const totalMortgages = realEstate.reduce((sum, prop) => sum + (prop.mortgage?.balance || 0), 0);
    const totalEquity = totalValue - totalMortgages;

    return (
        <div className="space-y-6 max-w-2xl">
            <div>
                <h2 className="text-3xl font-bold tracking-tight">Do you own any real estate?</h2>
                <p className="text-muted-foreground mt-2">
                    Property can be a significant part of your net worth and retirement planning.
                </p>
            </div>

            {realEstate.length === 0 ? (
                <Card>
                    <CardHeader>
                        <CardTitle>Add Real Estate</CardTitle>
                        <CardDescription>
                            Select the type of property you own
                        </CardDescription>
                    </CardHeader>
                    <CardContent className="space-y-3">
                        <div className="grid gap-2">
                            {PROPERTY_TYPES.map((propType) => (
                                <Button
                                    key={propType.value}
                                    variant="outline"
                                    className="justify-start h-auto py-3"
                                    onClick={() => handleAddProperty(propType.value)}
                                >
                                    <div className="flex flex-col items-start flex-1">
                                        <span className="font-medium">{propType.label}</span>
                                        <span className="text-xs text-muted-foreground">{propType.description}</span>
                                    </div>
                                    <Plus className="h-4 w-4 ml-2" />
                                </Button>
                            ))}
                        </div>

                        <Button variant="ghost" className="w-full" onClick={() => { }}>
                            Skip - I don't own property
                        </Button>
                    </CardContent>
                </Card>
            ) : (
                <>
                    {realEstate.map((property) => (
                        <Card key={property.id}>
                            <CardHeader>
                                <div className="flex items-start justify-between">
                                    <div>
                                        <CardTitle>
                                            {PROPERTY_TYPES.find((t) => t.value === property.type)?.label || property.type}
                                        </CardTitle>
                                        <CardDescription>
                                            {PROPERTY_TYPES.find((t) => t.value === property.type)?.description}
                                        </CardDescription>
                                    </div>
                                    <Button
                                        variant="ghost"
                                        size="icon"
                                        onClick={() => removeRealEstate(property.id)}
                                    >
                                        <Trash2 className="h-4 w-4" />
                                    </Button>
                                </div>
                            </CardHeader>
                            <CardContent className="space-y-4">
                                <div className="space-y-2">
                                    <Label htmlFor={`value-${property.id}`}>Estimated Current Value</Label>
                                    <MoneyInput
                                        value={property.value}
                                        onChange={(value) => updateRealEstate(property.id, { value })}
                                        placeholder="650000"
                                    />
                                </div>

                                <div className="space-y-3">
                                    <div className="flex items-center space-x-2">
                                        <Checkbox
                                            id={`mortgage-${property.id}`}
                                            checked={!!property.mortgage}
                                            onCheckedChange={(checked) => {
                                                if (checked) {
                                                    updateRealEstate(property.id, {
                                                        mortgage: {
                                                            balance: 0,
                                                            monthlyPayment: 0,
                                                            interestRate: 0,
                                                            yearsRemaining: 0,
                                                            includesPropertyTax: true,
                                                            includesInsurance: true,
                                                        },
                                                    });
                                                } else {
                                                    updateRealEstate(property.id, { mortgage: undefined });
                                                }
                                            }}
                                        />
                                        <Label htmlFor={`mortgage-${property.id}`} className="cursor-pointer">
                                            I have a mortgage on this property
                                        </Label>
                                    </div>

                                    {property.mortgage && (
                                        <div className="pl-6 space-y-3 border-l-2">
                                            <div className="grid grid-cols-2 gap-4">
                                                <div className="space-y-2">
                                                    <Label>Remaining Balance</Label>
                                                    <MoneyInput
                                                        value={property.mortgage.balance}
                                                        onChange={(value) =>
                                                            updateRealEstate(property.id, {
                                                                mortgage: { ...property.mortgage!, balance: value },
                                                            })
                                                        }
                                                        placeholder="380000"
                                                    />
                                                </div>
                                                <div className="space-y-2">
                                                    <Label>Monthly Payment</Label>
                                                    <MoneyInput
                                                        value={property.mortgage.monthlyPayment}
                                                        onChange={(value) =>
                                                            updateRealEstate(property.id, {
                                                                mortgage: { ...property.mortgage!, monthlyPayment: value },
                                                            })
                                                        }
                                                        placeholder="2800"
                                                    />
                                                </div>
                                            </div>

                                            <div className="grid grid-cols-2 gap-4">
                                                <div className="space-y-2">
                                                    <Label>Interest Rate</Label>
                                                    <div className="relative">
                                                        <Input
                                                            type="number"
                                                            value={property.mortgage.interestRate}
                                                            onChange={(e) =>
                                                                updateRealEstate(property.id, {
                                                                    mortgage: { ...property.mortgage!, interestRate: parseFloat(e.target.value) || 0 },
                                                                })
                                                            }
                                                            placeholder="3.25"
                                                            step="0.01"
                                                        />
                                                        <span className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground">
                                                            %
                                                        </span>
                                                    </div>
                                                </div>
                                                <div className="space-y-2">
                                                    <Label>Years Remaining</Label>
                                                    <Input
                                                        type="number"
                                                        value={property.mortgage.yearsRemaining}
                                                        onChange={(e) =>
                                                            updateRealEstate(property.id, {
                                                                mortgage: { ...property.mortgage!, yearsRemaining: parseFloat(e.target.value) || 0 },
                                                            })
                                                        }
                                                        placeholder="22"
                                                    />
                                                </div>
                                            </div>

                                            <div className="space-y-2">
                                                <Label className="text-sm">What's included in the payment?</Label>
                                                <div className="space-y-2">
                                                    <div className="flex items-center space-x-2">
                                                        <Checkbox
                                                            id={`includes-tax-${property.id}`}
                                                            checked={property.mortgage.includesPropertyTax}
                                                            onCheckedChange={(checked) =>
                                                                updateRealEstate(property.id, {
                                                                    mortgage: { ...property.mortgage!, includesPropertyTax: !!checked },
                                                                })
                                                            }
                                                        />
                                                        <Label htmlFor={`includes-tax-${property.id}`} className="cursor-pointer text-sm font-normal">
                                                            Property tax is escrowed
                                                        </Label>
                                                    </div>
                                                    <div className="flex items-center space-x-2">
                                                        <Checkbox
                                                            id={`includes-insurance-${property.id}`}
                                                            checked={property.mortgage.includesInsurance}
                                                            onCheckedChange={(checked) =>
                                                                updateRealEstate(property.id, {
                                                                    mortgage: { ...property.mortgage!, includesInsurance: !!checked },
                                                                })
                                                            }
                                                        />
                                                        <Label htmlFor={`includes-insurance-${property.id}`} className="cursor-pointer text-sm font-normal">
                                                            Insurance is escrowed
                                                        </Label>
                                                    </div>
                                                </div>
                                            </div>
                                        </div>
                                    )}
                                </div>

                                {(!property.mortgage || !property.mortgage.includesPropertyTax) && (
                                    <div className="space-y-2">
                                        <Label htmlFor={`property-tax-${property.id}`}>Annual Property Tax</Label>
                                        <MoneyInput
                                            value={property.propertyTax || 0}
                                            onChange={(value) => updateRealEstate(property.id, { propertyTax: value })}
                                            placeholder="8500"
                                        />
                                    </div>
                                )}

                                {(!property.mortgage || !property.mortgage.includesInsurance) && (
                                    <div className="space-y-2">
                                        <Label htmlFor={`insurance-${property.id}`}>Annual Insurance</Label>
                                        <MoneyInput
                                            value={property.insurance || 0}
                                            onChange={(value) => updateRealEstate(property.id, { insurance: value })}
                                            placeholder="1800"
                                        />
                                    </div>
                                )}

                                {property.type === "Investment" && (
                                    <div className="space-y-2">
                                        <Label htmlFor={`rental-income-${property.id}`}>Monthly Rental Income</Label>
                                        <MoneyInput
                                            value={property.rentalIncome || 0}
                                            onChange={(value) => updateRealEstate(property.id, { rentalIncome: value })}
                                            placeholder="2500"
                                        />
                                    </div>
                                )}

                                <div className="space-y-3">
                                    <Label>Do you plan to sell this property?</Label>
                                    <RadioGroup
                                        value={property.plannedSale?.trigger || "no"}
                                        onValueChange={(value) => {
                                            if (value === "no") {
                                                updateRealEstate(property.id, { plannedSale: undefined });
                                            } else {
                                                updateRealEstate(property.id, {
                                                    plannedSale: { trigger: value as any },
                                                });
                                            }
                                        }}
                                    >
                                        <div className="space-y-2">
                                            <div className="flex items-center space-x-2">
                                                <RadioGroupItem value="no" id={`no-sale-${property.id}`} />
                                                <Label htmlFor={`no-sale-${property.id}`} className="cursor-pointer text-sm">
                                                    No, I plan to keep it
                                                </Label>
                                            </div>
                                            <div className="flex items-center space-x-2">
                                                <RadioGroupItem value="Retirement" id={`retire-sale-${property.id}`} />
                                                <Label htmlFor={`retire-sale-${property.id}`} className="cursor-pointer text-sm">
                                                    Yes, when I retire
                                                </Label>
                                            </div>
                                            <div className="flex items-center space-x-2">
                                                <RadioGroupItem value="SpecificAge" id={`age-sale-${property.id}`} />
                                                <Label htmlFor={`age-sale-${property.id}`} className="cursor-pointer text-sm">
                                                    Yes, at a specific age
                                                </Label>
                                            </div>
                                        </div>
                                    </RadioGroup>

                                    {property.plannedSale?.trigger === "SpecificAge" && (
                                        <div className="pl-6 space-y-2">
                                            <Label className="text-sm">At what age?</Label>
                                            <Input
                                                type="number"
                                                value={property.plannedSale.age || ""}
                                                onChange={(e) =>
                                                    updateRealEstate(property.id, {
                                                        plannedSale: { ...property.plannedSale!, age: parseInt(e.target.value) || undefined },
                                                    })
                                                }
                                                placeholder={currentAge ? (currentAge + 10).toString() : "70"}
                                            />
                                        </div>
                                    )}
                                </div>

                                {property.value > 0 && (
                                    <div className="rounded-lg bg-muted p-3 space-y-1 text-sm">
                                        <div className="flex justify-between">
                                            <span className="text-muted-foreground">Property Value:</span>
                                            <span className="font-medium">{formatCurrency(property.value)}</span>
                                        </div>
                                        {property.mortgage && (
                                            <>
                                                <div className="flex justify-between">
                                                    <span className="text-muted-foreground">Mortgage Balance:</span>
                                                    <span className="font-medium">-{formatCurrency(property.mortgage.balance)}</span>
                                                </div>
                                                <div className="flex justify-between border-t pt-1">
                                                    <span className="text-muted-foreground">Home Equity:</span>
                                                    <span className="font-medium">{formatCurrency(calculateEquity(property))}</span>
                                                </div>
                                            </>
                                        )}
                                        {calculateMonthlyHousingCost(property) > 0 && (
                                            <div className="flex justify-between border-t pt-1">
                                                <span className="text-muted-foreground">Monthly Housing Cost:</span>
                                                <span className="font-medium">{formatCurrency(calculateMonthlyHousingCost(property))}</span>
                                            </div>
                                        )}
                                    </div>
                                )}
                            </CardContent>
                        </Card>
                    ))}

                    {realEstate.length < 3 && (
                        <Card>
                            <CardHeader>
                                <CardTitle>Add Another Property</CardTitle>
                            </CardHeader>
                            <CardContent>
                                <div className="grid gap-2">
                                    {PROPERTY_TYPES.filter(
                                        (type) => !realEstate.some((prop) => prop.type === type.value) || type.value === "Investment"
                                    ).map((propType) => (
                                        <Button
                                            key={`add-${propType.value}-${Math.random()}`}
                                            variant="outline"
                                            className="justify-start h-auto py-2"
                                            onClick={() => handleAddProperty(propType.value)}
                                        >
                                            <div className="flex flex-col items-start flex-1">
                                                <span className="font-medium text-sm">{propType.label}</span>
                                                <span className="text-xs text-muted-foreground">{propType.description}</span>
                                            </div>
                                            <Plus className="h-4 w-4 ml-2" />
                                        </Button>
                                    ))}
                                </div>
                            </CardContent>
                        </Card>
                    )}

                    {realEstate.length > 0 && (
                        <Card>
                            <CardHeader>
                                <CardTitle className="flex items-center gap-2">
                                    <Home className="h-5 w-5" />
                                    Real Estate Summary
                                </CardTitle>
                            </CardHeader>
                            <CardContent>
                                <div className="space-y-2 text-sm">
                                    <div className="flex justify-between items-center py-2 border-b">
                                        <span className="text-muted-foreground">Total Property Value</span>
                                        <span className="font-medium">{formatCurrency(totalValue)}</span>
                                    </div>
                                    <div className="flex justify-between items-center py-2 border-b">
                                        <span className="text-muted-foreground">Total Mortgages</span>
                                        <span className="font-medium">-{formatCurrency(totalMortgages)}</span>
                                    </div>
                                    <div className="flex justify-between items-center py-2 font-medium">
                                        <span>Total Equity</span>
                                        <span>{formatCurrency(totalEquity)}</span>
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
