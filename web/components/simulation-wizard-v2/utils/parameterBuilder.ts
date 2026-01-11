/**
 * Parameter Builder Utility
 * 
 * Converts wizard state (user-friendly inputs) into SimulationParameters
 * that can be sent to the backend API.
 */

import { WizardState } from "../types";
import {
    SimulationParameters,
    Account,
    Asset,
    CashFlow,
    Event,
    SpendingTarget,
    TaxConfig,
    ReturnProfile,
    InflationProfile,
    AccountType,
    RepeatInterval,
    CashFlowDirection,
    EventTrigger,
    EventEffect,
    WithdrawalStrategy,
} from "@/lib/types";

// Map wizard frequency to API repeat interval
function mapPayFrequency(frequency: string): RepeatInterval {
    switch (frequency) {
        case "Weekly": return "Weekly";
        case "BiWeekly": return "BiWeekly";
        case "SemiMonthly": return "Monthly"; // Approximate to monthly
        case "Monthly": return "Monthly";
        default: return "Monthly";
    }
}

// Generate unique IDs
let nextAccountId = 1;
let nextAssetId = 1;
let nextCashFlowId = 1;
let nextEventId = 1;
let nextSpendingTargetId = 1;

function resetIds() {
    nextAccountId = 1;
    nextAssetId = 1;
    nextCashFlowId = 1;
    nextEventId = 1;
    nextSpendingTargetId = 1;
}

// Return profile indexes
const CASH_PROFILE_INDEX = 0;
const STOCK_BOND_PROFILE_INDEX = 1;
const HOUSING_PROFILE_INDEX = 2;

/**
 * Build complete simulation parameters from wizard state
 */
export function buildSimulationParameters(state: WizardState): SimulationParameters {
    resetIds();

    const accounts: Account[] = [];
    const cashFlows: CashFlow[] = [];
    const events: Event[] = [];
    const spendingTargets: SpendingTarget[] = [];

    // Get current age for age-based triggers
    const currentAge = state.personalInfo.birthDate
        ? Math.floor((Date.now() - state.personalInfo.birthDate.getTime()) / (365.25 * 24 * 60 * 60 * 1000))
        : 30;

    // =========================================================================
    // 1. Create Checking/Savings Accounts
    // =========================================================================

    const checkingAccountId = nextAccountId++;
    const checkingAssetId = nextAssetId++;

    if (state.savings.checking > 0 || state.savings.savings > 0 || state.savings.hysa > 0) {
        const totalLiquid = state.savings.checking + state.savings.savings + state.savings.hysa;

        accounts.push({
            account_id: checkingAccountId,
            account_type: "Taxable",
            name: "Checking & Savings",
            assets: [{
                asset_id: checkingAssetId,
                asset_class: "Investable",
                initial_value: totalLiquid,
                return_profile_index: CASH_PROFILE_INDEX,
                name: "Cash",
            }],
        });
    }

    // =========================================================================
    // 2. Create Investment Accounts
    // =========================================================================

    for (const investment of state.investments) {
        const accountId = nextAccountId++;
        const assetId = nextAssetId++;

        let accountType: AccountType = "Taxable";
        if (investment.type === "Traditional401k" || investment.type === "TraditionalIRA") {
            accountType = "TaxDeferred";
        } else if (investment.type === "Roth401k" || investment.type === "RothIRA" || investment.type === "HSA") {
            accountType = "TaxFree";
        }

        accounts.push({
            account_id: accountId,
            account_type: accountType,
            name: investment.type,
            assets: [{
                asset_id: assetId,
                asset_class: "Investable",
                initial_value: investment.balance,
                return_profile_index: STOCK_BOND_PROFILE_INDEX,
                name: `${investment.type} Investments`,
            }],
        });

        // Add ongoing contributions if specified
        if (investment.contributions && investment.contributions.amount > 0) {
            const cashFlowId = nextCashFlowId++;

            cashFlows.push({
                cash_flow_id: cashFlowId,
                amount: investment.contributions.amount,
                repeats: mapPayFrequency(investment.contributions.frequency),
                adjust_for_inflation: true,
                direction: {
                    Income: {
                        target_account_id: accountId,
                        target_asset_id: assetId,
                    },
                },
                state: "Active",
            });
        }
    }

    // =========================================================================
    // 3. Create Income Cash Flows
    // =========================================================================

    if (state.income.employed && state.income.salary > 0) {
        const cashFlowId = nextCashFlowId++;

        // Convert annual salary to payment frequency
        const paymentMultipliers: Record<string, number> = {
            Weekly: 52,
            BiWeekly: 26,
            SemiMonthly: 24,
            Monthly: 12,
        };

        const multiplier = paymentMultipliers[state.income.payFrequency] || 12;
        const paymentAmount = state.income.salary / multiplier;

        cashFlows.push({
            cash_flow_id: cashFlowId,
            amount: paymentAmount,
            repeats: mapPayFrequency(state.income.payFrequency),
            adjust_for_inflation: true,
            direction: {
                Income: {
                    target_account_id: checkingAccountId,
                    target_asset_id: checkingAssetId,
                },
            },
            state: "Active",
        });

        // Add employer 401k match if applicable
        if (state.income.employer401k?.hasMatch && state.income.employer401k.matchPercentage > 0) {
            const matchCashFlowId = nextCashFlowId++;
            const matchAmount = Math.min(
                state.income.salary * (state.income.employer401k.employeeContribution / 100),
                state.income.salary * (state.income.employer401k.matchUpTo / 100)
            ) * (state.income.employer401k.matchPercentage / 100);

            // Find the 401k account to deposit match
            const account401k = state.investments.find(inv =>
                inv.type === "Traditional401k" || inv.type === "Roth401k"
            );

            if (account401k) {
                const match401kAccountId = accounts.find(acc => acc.name === account401k.type)?.account_id;
                const match401kAssetId = accounts.find(acc => acc.name === account401k.type)?.assets[0].asset_id;

                if (match401kAccountId && match401kAssetId) {
                    cashFlows.push({
                        cash_flow_id: matchCashFlowId,
                        amount: matchAmount / multiplier,
                        repeats: mapPayFrequency(state.income.payFrequency),
                        adjust_for_inflation: true,
                        direction: {
                            Income: {
                                target_account_id: match401kAccountId,
                                target_asset_id: match401kAssetId,
                            },
                        },
                        state: "Active",
                    });
                }
            }
        }
    }

    // Add other income sources
    for (const otherIncome of state.income.otherIncome) {
        const cashFlowId = nextCashFlowId++;

        cashFlows.push({
            cash_flow_id: cashFlowId,
            amount: otherIncome.amount,
            repeats: mapPayFrequency(otherIncome.frequency),
            adjust_for_inflation: true,
            direction: {
                Income: {
                    target_account_id: checkingAccountId,
                    target_asset_id: checkingAssetId,
                },
            },
            state: "Active",
        });
    }

    // =========================================================================
    // 4. Create Real Estate Assets
    // =========================================================================

    for (const property of state.realEstate) {
        const propertyAccountId = nextAccountId++;
        const propertyAssetId = nextAssetId++;

        accounts.push({
            account_id: propertyAccountId,
            account_type: "Illiquid",
            name: `${property.type} Property`,
            assets: [{
                asset_id: propertyAssetId,
                asset_class: "RealEstate",
                initial_value: property.value,
                return_profile_index: HOUSING_PROFILE_INDEX,
                name: property.type,
            }],
        });

        // Create mortgage liability account
        if (property.mortgage && property.mortgage.balance > 0) {
            const mortgageAccountId = nextAccountId++;
            const mortgageAssetId = nextAssetId++;

            accounts.push({
                account_id: mortgageAccountId,
                account_type: "Illiquid",
                name: `${property.type} Mortgage`,
                assets: [{
                    asset_id: mortgageAssetId,
                    asset_class: "Liability",
                    initial_value: -property.mortgage.balance,
                    return_profile_index: CASH_PROFILE_INDEX,
                    name: "Mortgage Debt",
                }],
            });

            // Create mortgage payment expense
            const mortgageCashFlowId = nextCashFlowId++;
            cashFlows.push({
                cash_flow_id: mortgageCashFlowId,
                amount: property.mortgage.monthlyPayment,
                repeats: "Monthly",
                adjust_for_inflation: false,
                direction: {
                    Expense: {
                        source_account_id: checkingAccountId,
                        source_asset_id: checkingAssetId,
                    },
                },
                state: "Active",
            });
        }

        // Add rental income if applicable
        if (property.rentalIncome && property.rentalIncome > 0) {
            const rentalCashFlowId = nextCashFlowId++;
            cashFlows.push({
                cash_flow_id: rentalCashFlowId,
                amount: property.rentalIncome,
                repeats: "Monthly",
                adjust_for_inflation: true,
                direction: {
                    Income: {
                        target_account_id: checkingAccountId,
                        target_asset_id: checkingAssetId,
                    },
                },
                state: "Active",
            });
        }

        // Add property sale event if planned
        if (property.plannedSale) {
            const saleEventId = nextEventId++;
            let trigger: EventTrigger;

            if (property.plannedSale.trigger === "Retirement" && state.retirement.targetAge) {
                trigger = { Age: { years: state.retirement.targetAge } };
            } else if (property.plannedSale.trigger === "SpecificAge" && property.plannedSale.age) {
                trigger = { Age: { years: property.plannedSale.age } };
            } else {
                continue; // Skip if trigger not properly set
            }

            events.push({
                event_id: saleEventId,
                trigger,
                effects: [{
                    DeleteAccount: propertyAccountId,
                }],
                once: true,
            });
        }
    }

    // =========================================================================
    // 5. Create Debt Accounts
    // =========================================================================

    for (const debt of state.debts) {
        const debtAccountId = nextAccountId++;
        const debtAssetId = nextAssetId++;

        accounts.push({
            account_id: debtAccountId,
            account_type: "Illiquid",
            name: `${debt.type} Debt`,
            assets: [{
                asset_id: debtAssetId,
                asset_class: "Liability",
                initial_value: -debt.balance,
                return_profile_index: CASH_PROFILE_INDEX,
                name: debt.description || debt.type,
            }],
        });

        // Create debt payment expense
        const debtCashFlowId = nextCashFlowId++;
        cashFlows.push({
            cash_flow_id: debtCashFlowId,
            amount: debt.monthlyPayment,
            repeats: "Monthly",
            adjust_for_inflation: false,
            direction: {
                Expense: {
                    source_account_id: checkingAccountId,
                    source_asset_id: checkingAssetId,
                },
            },
            state: "Active",
        });
    }

    // =========================================================================
    // 6. Create Retirement Spending Target
    // =========================================================================

    if (state.retirement.targetAge && state.retirement.targetIncome) {
        const spendingTargetId = nextSpendingTargetId++;
        const retirementEventId = nextEventId++;

        // Convert annual income to monthly
        const monthlyIncome = state.retirement.targetIncome / 12;

        spendingTargets.push({
            spending_target_id: spendingTargetId,
            amount: monthlyIncome,
            net_amount_mode: false,
            repeats: "Monthly",
            adjust_for_inflation: true,
            withdrawal_strategy: "TaxOptimized",
            exclude_accounts: [],
            state: "Pending",
        });

        // Create event to activate spending target at retirement age
        events.push({
            event_id: retirementEventId,
            trigger: { Age: { years: state.retirement.targetAge } },
            effects: [{
                ActivateSpendingTarget: spendingTargetId,
            }],
            once: true,
        });

        // Terminate work income at retirement
        const workIncomeCashFlows = cashFlows.filter(cf =>
            'Income' in cf.direction &&
            cf.direction.Income.target_account_id === checkingAccountId
        );

        if (workIncomeCashFlows.length > 0) {
            events.push({
                event_id: nextEventId++,
                trigger: { Age: { years: state.retirement.targetAge } },
                effects: workIncomeCashFlows.map(cf => ({
                    TerminateCashFlow: cf.cash_flow_id,
                })),
                once: true,
            });
        }
    }

    // =========================================================================
    // 7. Add Social Security
    // =========================================================================

    if (state.retirement.socialSecurity.hasSSI &&
        state.retirement.socialSecurity.estimatedBenefit &&
        state.retirement.socialSecurity.claimingAge) {

        const ssEventId = nextEventId++;
        const ssCashFlowId = nextCashFlowId++;

        cashFlows.push({
            cash_flow_id: ssCashFlowId,
            amount: state.retirement.socialSecurity.estimatedBenefit,
            repeats: "Monthly",
            adjust_for_inflation: true,
            direction: {
                Income: {
                    target_account_id: checkingAccountId,
                    target_asset_id: checkingAssetId,
                },
            },
            state: "Pending",
        });

        events.push({
            event_id: ssEventId,
            trigger: { Age: { years: state.retirement.socialSecurity.claimingAge } },
            effects: [{
                ActivateCashFlow: ssCashFlowId,
            }],
            once: true,
        });
    }

    // =========================================================================
    // 8. Add Pension
    // =========================================================================

    if (state.retirement.pension.hasPension &&
        state.retirement.pension.monthlyAmount &&
        state.retirement.pension.startAge) {

        const pensionEventId = nextEventId++;
        const pensionCashFlowId = nextCashFlowId++;

        cashFlows.push({
            cash_flow_id: pensionCashFlowId,
            amount: state.retirement.pension.monthlyAmount,
            repeats: "Monthly",
            adjust_for_inflation: true,
            direction: {
                Income: {
                    target_account_id: checkingAccountId,
                    target_asset_id: checkingAssetId,
                },
            },
            state: "Pending",
        });

        events.push({
            event_id: pensionEventId,
            trigger: { Age: { years: state.retirement.pension.startAge } },
            effects: [{
                ActivateCashFlow: pensionCashFlowId,
            }],
            once: true,
        });
    }

    // =========================================================================
    // 9. Add Life Events
    // =========================================================================

    for (const lifeEvent of state.lifeEvents) {
        const eventId = nextEventId++;
        const eventAge = currentAge + lifeEvent.yearsFromNow;

        if (lifeEvent.recurring) {
            // Create recurring expense
            const eventCashFlowId = nextCashFlowId++;

            cashFlows.push({
                cash_flow_id: eventCashFlowId,
                amount: lifeEvent.amount,
                repeats: "Yearly",
                adjust_for_inflation: lifeEvent.recurring.inflationAdjusted,
                direction: {
                    Expense: {
                        source_account_id: checkingAccountId,
                        source_asset_id: checkingAssetId,
                    },
                },
                state: "Pending",
            });

            // Activate at event age
            events.push({
                event_id: eventId,
                trigger: { Age: { years: eventAge } },
                effects: [{
                    ActivateCashFlow: eventCashFlowId,
                }],
                once: true,
            });

            // Terminate after duration
            if (lifeEvent.recurring.duration > 0) {
                const endEventId = nextEventId++;
                events.push({
                    event_id: endEventId,
                    trigger: { Age: { years: eventAge + lifeEvent.recurring.duration } },
                    effects: [{
                        TerminateCashFlow: eventCashFlowId,
                    }],
                    once: true,
                });
            }
        } else {
            // One-time expense or income
            const isIncome = lifeEvent.type === "Inheritance";
            const eventCashFlowId = nextCashFlowId++;

            cashFlows.push({
                cash_flow_id: eventCashFlowId,
                amount: Math.abs(lifeEvent.amount),
                repeats: "Never",
                adjust_for_inflation: false,
                direction: isIncome ? {
                    Income: {
                        target_account_id: checkingAccountId,
                        target_asset_id: checkingAssetId,
                    },
                } : {
                    Expense: {
                        source_account_id: checkingAccountId,
                        source_asset_id: checkingAssetId,
                    },
                },
                state: "Pending",
            });

            events.push({
                event_id: eventId,
                trigger: { Age: { years: eventAge } },
                effects: [{
                    ActivateCashFlow: eventCashFlowId,
                }],
                once: true,
            });
        }
    }

    // =========================================================================
    // 10. Build Tax Config
    // =========================================================================

    const taxConfig: TaxConfig = {
        federal_brackets: [
            { threshold: 0, rate: 0.10 },
            { threshold: 11000, rate: 0.12 },
            { threshold: 44725, rate: 0.22 },
            { threshold: 95375, rate: 0.24 },
            { threshold: 182100, rate: 0.32 },
            { threshold: 231250, rate: 0.35 },
            { threshold: 578125, rate: 0.37 },
        ],
        state_rate: getStateRate(state.personalInfo.state),
        capital_gains_rate: 0.15,
        taxable_gains_percentage: 0.5,
    };

    // =========================================================================
    // 11. Build Return Profiles
    // =========================================================================

    const returnProfiles: ReturnProfile[] = [
        { Fixed: 0.03 }, // Cash (index 0)
        { Normal: { mean: 0.08, std_dev: 0.15 } }, // Stocks/Bonds (index 1)
        { Normal: { mean: 0.04, std_dev: 0.035 } }, // Housing (index 2)
    ];

    // =========================================================================
    // 12. Assemble Final Parameters
    // =========================================================================

    const birthDate = state.personalInfo.birthDate?.toISOString().split('T')[0];

    return {
        start_date: new Date().toISOString().split('T')[0],
        duration_years: state.retirement.targetAge
            ? (state.retirement.targetAge - currentAge + 30) // Run 30 years past retirement
            : 50,
        birth_date: birthDate,
        retirement_age: state.retirement.targetAge || undefined,
        inflation_profile: { Normal: { mean: 0.03, std_dev: 0.02 } },
        return_profiles: returnProfiles,
        events,
        accounts,
        cash_flows: cashFlows,
        spending_targets: spendingTargets,
        tax_config: taxConfig,
    };
}

/**
 * Get state tax rate based on state code
 */
function getStateRate(state: string | null): number {
    const stateRates: Record<string, number> = {
        CA: 0.093, NY: 0.0882, NJ: 0.1075, HI: 0.11,
        OR: 0.099, MN: 0.0985, DC: 0.0895, IA: 0.0853,
        WI: 0.0765, VT: 0.0875, ME: 0.0715, SC: 0.07,
        CT: 0.0699, ID: 0.058, NE: 0.0684, MT: 0.0675,
        WV: 0.065, KS: 0.057, AK: 0.0, FL: 0.0,
        NV: 0.0, SD: 0.0, TN: 0.0, TX: 0.0,
        WA: 0.0, WY: 0.0,
    };

    return state ? (stateRates[state] || 0.05) : 0.05;
}
