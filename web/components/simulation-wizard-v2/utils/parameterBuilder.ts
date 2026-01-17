/**
 * Parameter Builder Utility
 *
 * Converts wizard state (user-friendly inputs) into SimulationRequest
 * that can be sent to the backend API (name-based, no IDs).
 */

import { WizardState } from "../types";
import {
    SimulationRequest,
    AccountDef,
    AccountTypeDef,
    AssetDef,
    PositionDef,
    EventDef,
    EffectDef,
    TriggerDef,
    NamedReturnProfileDef,
    RepeatInterval,
    TaxConfigDef,
    InflationProfile,
} from "@/lib/api-types";

// Named return profiles
const RETURN_PROFILES: NamedReturnProfileDef[] = [
    { name: "Cash", profile: { Fixed: 0.03 } },
    { name: "StockBond", profile: { Normal: { mean: 0.08, std_dev: 0.15 } } },
    { name: "Housing", profile: { Normal: { mean: 0.04, std_dev: 0.035 } } },
];

// Map wizard frequency to API repeat interval
function mapPayFrequency(frequency: string): RepeatInterval {
    switch (frequency) {
        case "Weekly": return "Weekly";
        case "BiWeekly": return "BiWeekly";
        case "SemiMonthly": return "Monthly"; // Approximate to monthly
        case "Monthly": return "Monthly";
        case "Yearly": return "Yearly";
        default: return "Monthly";
    }
}

// Map wizard account type to AccountTypeDef
function mapAccountType(type: string): AccountTypeDef {
    switch (type) {
        case "Traditional401k": return { type: "Traditional401k", contribution_limit: null };
        case "Roth401k": return { type: "Roth401k", contribution_limit: null };
        case "TraditionalIRA": return { type: "TraditionalIra", contribution_limit: null };
        case "RothIRA": return { type: "RothIra", contribution_limit: null };
        case "HSA": return { type: "Hsa", contribution_limit: null };
        case "Brokerage": return { type: "TaxableBrokerage" };
        default: return { type: "Bank" };
    }
}

// Generate unique names to avoid collisions
function sanitizeName(name: string): string {
    return name.replace(/[^a-zA-Z0-9]/g, '_');
}

/**
 * Build complete simulation request from wizard state
 */
export function buildSimulationRequest(state: WizardState): SimulationRequest {
    const accounts: AccountDef[] = [];
    const assets: AssetDef[] = [];
    const positions: PositionDef[] = [];
    const events: EventDef[] = [];

    // Get current age for age-based triggers
    const currentAge = state.personalInfo.birthDate
        ? Math.floor((Date.now() - state.personalInfo.birthDate.getTime()) / (365.25 * 24 * 60 * 60 * 1000))
        : 30;

    // Define standard assets
    assets.push(
        { name: "Cash", description: null, price: 1, return_profile: "Cash" },
        { name: "Investments", description: null, price: 1, return_profile: "StockBond" },
        { name: "RealEstate", description: null, price: 1, return_profile: "Housing" }
    );

    // =========================================================================
    // 1. Create Checking/Savings Account
    // =========================================================================

    const totalLiquid = state.savings.checking + state.savings.savings + state.savings.hysa;

    accounts.push({
        name: "Checking",
        description: "Checking & Savings",
        account_type: { type: "Bank" },
        cash: totalLiquid,
        cash_return_profile: "Cash",
    });

    // =========================================================================
    // 2. Create Investment Accounts
    // =========================================================================

    for (const investment of state.investments) {
        const accountName = sanitizeName(investment.type);

        accounts.push({
            name: accountName,
            description: `${investment.type} Account`,
            account_type: mapAccountType(investment.type),
            cash: 0,
            cash_return_profile: "Cash",
        });

        // Create position for the investment balance
        if (investment.balance > 0) {
            positions.push({
                account: accountName,
                asset: "Investments",
                units: investment.balance,
                cost_basis: investment.balance,
                purchase_date: null,
            });
        }

        // Add ongoing contributions as repeating income events
        if (investment.contributions && investment.contributions.amount > 0) {
            events.push({
                name: `${accountName}_Contribution`,
                description: `Regular contribution to ${investment.type}`,
                trigger: {
                    type: "Repeating",
                    interval: mapPayFrequency(investment.contributions.frequency),
                    start: null,
                    end: state.retirement.targetAge ? { type: "Age", years: state.retirement.targetAge, months: null } : null,
                },
                effects: [{
                    type: "AssetPurchase",
                    amount: investment.contributions.amount,
                    account: accountName,
                    asset: "Investments",
                    adjust_for_inflation: true,
                }],
                once: false,
            });
        }
    }

    // =========================================================================
    // 3. Create Income Events
    // =========================================================================

    if (state.income.employed && state.income.salary > 0) {
        const paymentMultipliers: Record<string, number> = {
            Weekly: 52,
            BiWeekly: 26,
            SemiMonthly: 24,
            Monthly: 12,
        };

        const multiplier = paymentMultipliers[state.income.payFrequency] || 12;
        const paymentAmount = state.income.salary / multiplier;

        events.push({
            name: "Salary",
            description: "Primary employment income",
            trigger: {
                type: "Repeating",
                interval: mapPayFrequency(state.income.payFrequency),
                start: null,
                end: state.retirement.targetAge ? { type: "Age", years: state.retirement.targetAge, months: null } : null,
            },
            effects: [{
                type: "Income",
                amount: paymentAmount,
                to_account: "Checking",
                income_type: "Taxable",
                gross: true,
                adjust_for_inflation: true,
            }],
            once: false,
        });

        // Add employer 401k match if applicable
        if (state.income.employer401k?.hasMatch && state.income.employer401k.matchPercentage > 0) {
            const account401k = state.investments.find(inv =>
                inv.type === "Traditional401k" || inv.type === "Roth401k"
            );

            if (account401k) {
                const matchAmount = Math.min(
                    state.income.salary * (state.income.employer401k.employeeContribution / 100),
                    state.income.salary * (state.income.employer401k.matchUpTo / 100)
                ) * (state.income.employer401k.matchPercentage / 100);

                const accountName = sanitizeName(account401k.type);

                events.push({
                    name: "Employer401kMatch",
                    description: "Employer 401k matching contribution",
                    trigger: {
                        type: "Repeating",
                        interval: mapPayFrequency(state.income.payFrequency),
                        start: null,
                        end: state.retirement.targetAge ? { type: "Age", years: state.retirement.targetAge, months: null } : null,
                    },
                    effects: [{
                        type: "AssetPurchase",
                        amount: matchAmount / multiplier,
                        account: accountName,
                        asset: "Investments",
                        adjust_for_inflation: true,
                    }],
                    once: false,
                });
            }
        }
    }

    // Add other income sources
    for (let i = 0; i < state.income.otherIncome.length; i++) {
        const otherIncome = state.income.otherIncome[i];
        events.push({
            name: `OtherIncome_${i + 1}`,
            description: otherIncome.description || "Other income",
            trigger: {
                type: "Repeating",
                interval: mapPayFrequency(otherIncome.frequency),
                start: null,
                end: null,
            },
            effects: [{
                type: "Income",
                amount: otherIncome.amount,
                to_account: "Checking",
                income_type: "Taxable",
                gross: true,
                adjust_for_inflation: true,
            }],
            once: false,
        });
    }

    // =========================================================================
    // 4. Create Real Estate Accounts & Events
    // =========================================================================

    for (let i = 0; i < state.realEstate.length; i++) {
        const property = state.realEstate[i];
        const propertyName = `Property_${sanitizeName(property.type)}_${i + 1}`;

        // Property account
        accounts.push({
            name: propertyName,
            description: `${property.type} Property`,
            account_type: { type: "Custom", tax_status: "Taxable", contribution_limit: null },
            cash: 0,
            cash_return_profile: null,
        });

        // Property position
        positions.push({
            account: propertyName,
            asset: "RealEstate",
            units: property.value,
            cost_basis: property.value,
            purchase_date: null,
        });

        // Create mortgage liability and payments
        if (property.mortgage && property.mortgage.balance > 0) {
            const mortgageName = `${propertyName}_Mortgage`;

            accounts.push({
                name: mortgageName,
                description: `${property.type} Mortgage`,
                account_type: { type: "Custom", tax_status: "Taxable", contribution_limit: null },
                cash: -property.mortgage.balance,
                cash_return_profile: null,
            });

            // Mortgage payment expense
            events.push({
                name: `${mortgageName}_Payment`,
                description: `Monthly mortgage payment for ${property.type}`,
                trigger: {
                    type: "Repeating",
                    interval: "Monthly",
                    start: null,
                    end: null,
                },
                effects: [{
                    type: "Expense",
                    amount: property.mortgage.monthlyPayment,
                    from_account: "Checking",
                    adjust_for_inflation: false,
                }],
                once: false,
            });
        }

        // Add rental income if applicable
        if (property.rentalIncome && property.rentalIncome > 0) {
            events.push({
                name: `${propertyName}_Rental`,
                description: `Rental income from ${property.type}`,
                trigger: {
                    type: "Repeating",
                    interval: "Monthly",
                    start: null,
                    end: null,
                },
                effects: [{
                    type: "Income",
                    amount: property.rentalIncome,
                    to_account: "Checking",
                    income_type: "Taxable",
                    gross: true,
                    adjust_for_inflation: true,
                }],
                once: false,
            });
        }
    }

    // =========================================================================
    // 5. Create Debt Accounts & Payment Events
    // =========================================================================

    for (let i = 0; i < state.debts.length; i++) {
        const debt = state.debts[i];
        const debtName = `Debt_${sanitizeName(debt.type)}_${i + 1}`;

        accounts.push({
            name: debtName,
            description: debt.description || `${debt.type} Debt`,
            account_type: { type: "Custom", tax_status: "Taxable", contribution_limit: null },
            cash: -debt.balance,
            cash_return_profile: null,
        });

        // Debt payment expense
        events.push({
            name: `${debtName}_Payment`,
            description: `Monthly payment for ${debt.type}`,
            trigger: {
                type: "Repeating",
                interval: "Monthly",
                start: null,
                end: null,
            },
            effects: [{
                type: "Expense",
                amount: debt.monthlyPayment,
                from_account: "Checking",
                adjust_for_inflation: false,
            }],
            once: false,
        });
    }

    // =========================================================================
    // 6. Create Retirement Spending Event
    // =========================================================================

    if (state.retirement.targetAge && state.retirement.targetIncome) {
        const monthlyIncome = state.retirement.targetIncome / 12;

        events.push({
            name: "RetirementSpending",
            description: "Monthly retirement spending",
            trigger: {
                type: "Repeating",
                interval: "Monthly",
                start: { type: "Age", years: state.retirement.targetAge, months: null },
                end: null,
            },
            effects: [{
                type: "Withdrawal",
                amount: { type: "Fixed", value: monthlyIncome },
                to_account: "Checking",
                source: { type: "Strategy", order: "TaxEfficientEarly", exclude: [] },
                gross: false,
                lot_method: "Fifo",
            }],
            once: false,
        });
    }

    // =========================================================================
    // 7. Add Social Security
    // =========================================================================

    if (state.retirement.socialSecurity.hasSSI &&
        state.retirement.socialSecurity.estimatedBenefit &&
        state.retirement.socialSecurity.claimingAge) {

        events.push({
            name: "SocialSecurity",
            description: "Social Security benefits",
            trigger: {
                type: "Repeating",
                interval: "Monthly",
                start: { type: "Age", years: state.retirement.socialSecurity.claimingAge, months: null },
                end: null,
            },
            effects: [{
                type: "Income",
                amount: state.retirement.socialSecurity.estimatedBenefit,
                to_account: "Checking",
                income_type: "Taxable",
                gross: true,
                adjust_for_inflation: true,
            }],
            once: false,
        });
    }

    // =========================================================================
    // 8. Add Pension
    // =========================================================================

    if (state.retirement.pension.hasPension &&
        state.retirement.pension.monthlyAmount &&
        state.retirement.pension.startAge) {

        events.push({
            name: "Pension",
            description: "Pension income",
            trigger: {
                type: "Repeating",
                interval: "Monthly",
                start: { type: "Age", years: state.retirement.pension.startAge, months: null },
                end: null,
            },
            effects: [{
                type: "Income",
                amount: state.retirement.pension.monthlyAmount,
                to_account: "Checking",
                income_type: "Taxable",
                gross: true,
                adjust_for_inflation: true,
            }],
            once: false,
        });
    }

    // =========================================================================
    // 9. Add Life Events
    // =========================================================================

    for (let i = 0; i < state.lifeEvents.length; i++) {
        const lifeEvent = state.lifeEvents[i];
        const eventAge = currentAge + lifeEvent.yearsFromNow;
        const eventName = `LifeEvent_${sanitizeName(lifeEvent.type)}_${i + 1}`;
        const isIncome = lifeEvent.type === "Inheritance";

        if (lifeEvent.recurring) {
            // Recurring expense/income
            const effect: EffectDef = isIncome ? {
                type: "Income",
                amount: Math.abs(lifeEvent.amount),
                to_account: "Checking",
                income_type: "TaxFree",
                gross: false,
                adjust_for_inflation: lifeEvent.recurring.inflationAdjusted,
            } : {
                type: "Expense",
                amount: Math.abs(lifeEvent.amount),
                from_account: "Checking",
                adjust_for_inflation: lifeEvent.recurring.inflationAdjusted,
            };

            events.push({
                name: eventName,
                description: lifeEvent.description,
                trigger: {
                    type: "Repeating",
                    interval: "Yearly",
                    start: { type: "Age", years: eventAge, months: null },
                    end: lifeEvent.recurring.duration > 0
                        ? { type: "Age", years: eventAge + lifeEvent.recurring.duration, months: null }
                        : null,
                },
                effects: [effect],
                once: false,
            });
        } else {
            // One-time expense or income
            const effect: EffectDef = isIncome ? {
                type: "Income",
                amount: Math.abs(lifeEvent.amount),
                to_account: "Checking",
                income_type: "TaxFree",
                gross: false,
                adjust_for_inflation: false,
            } : {
                type: "Expense",
                amount: Math.abs(lifeEvent.amount),
                from_account: "Checking",
                adjust_for_inflation: false,
            };

            events.push({
                name: eventName,
                description: lifeEvent.description,
                trigger: { type: "Age", years: eventAge, months: null },
                effects: [effect],
                once: true,
            });
        }
    }

    // =========================================================================
    // 10. Build Tax Config
    // =========================================================================

    const taxConfig: TaxConfigDef = {
        standard_deduction: state.personalInfo.filingStatus === "MarriedFilingJointly" ? 29200 : 14600,
        capital_gains_rate: 0.15,
    };

    // =========================================================================
    // 11. Build Inflation Profile
    // =========================================================================

    const inflationProfile: InflationProfile = { Normal: { mean: 0.03, std_dev: 0.02 } };

    // =========================================================================
    // 12. Assemble Final Request
    // =========================================================================

    const birthDate = state.personalInfo.birthDate?.toISOString().split('T')[0] || null;

    return {
        name: state.simulationName || "My Simulation",
        description: `${state.goal || "Financial planning"} simulation`,
        start_date: new Date().toISOString().split('T')[0],
        duration_years: state.retirement.targetAge
            ? (state.retirement.targetAge - currentAge + 30) // Run 30 years past retirement
            : 50,
        birth_date: birthDate,
        return_profiles: RETURN_PROFILES,
        inflation_profile: inflationProfile,
        tax_config: taxConfig,
        accounts,
        assets,
        positions,
        events,
    };
}
