import { useWizardStore } from '../hooks/useWizardStore';

export function useCalculations() {
    const state = useWizardStore();

    // Calculate net worth
    const netWorth = (() => {
        const liquidSavings = state.savings.checking + state.savings.savings + state.savings.hysa;
        const investmentValue = state.investments.reduce((sum, inv) => sum + inv.balance, 0);
        const realEstateEquity = state.realEstate.reduce((sum, prop) => {
            const mortgageBalance = prop.mortgage?.balance || 0;
            return sum + (prop.value - mortgageBalance);
        }, 0);
        const totalDebt = state.debts.reduce((sum, debt) => sum + debt.balance, 0);

        return liquidSavings + investmentValue + realEstateEquity - totalDebt;
    })();

    // Calculate monthly income (after tax - rough estimate)
    const monthlyIncome = (() => {
        if (!state.income.employed || state.income.salary === 0) return 0;

        const annualSalary = state.income.salary;
        const taxRate = 0.25; // Rough estimate, should be more sophisticated
        const afterTaxAnnual = annualSalary * (1 - taxRate);

        return afterTaxAnnual / 12;
    })();

    // Calculate monthly expenses
    const monthlyExpenses = (() => {
        let total = 0;

        // Housing costs
        state.realEstate.forEach((prop) => {
            if (prop.mortgage) {
                total += prop.mortgage.monthlyPayment;
            }
            if (prop.propertyTax && !prop.mortgage?.includesPropertyTax) {
                total += prop.propertyTax / 12;
            }
            if (prop.insurance && !prop.mortgage?.includesInsurance) {
                total += prop.insurance / 12;
            }
        });

        // Debt payments
        state.debts.forEach((debt) => {
            total += debt.monthlyPayment;
        });

        // Retirement contributions
        if (state.income.employer401k) {
            const contribution = (state.income.salary * state.income.employer401k.employeeContribution) / 100 / 12;
            total += contribution;
        }

        // Investment contributions
        state.investments.forEach((inv) => {
            if (inv.contributions) {
                let monthlyContribution = 0;
                switch (inv.contributions.frequency) {
                    case 'Weekly':
                        monthlyContribution = inv.contributions.amount * 52 / 12;
                        break;
                    case 'BiWeekly':
                        monthlyContribution = inv.contributions.amount * 26 / 12;
                        break;
                    case 'SemiMonthly':
                        monthlyContribution = inv.contributions.amount * 2;
                        break;
                    case 'Monthly':
                        monthlyContribution = inv.contributions.amount;
                        break;
                }
                total += monthlyContribution;
            }
        });

        return total;
    })();

    // Calculate age
    const currentAge = (() => {
        if (!state.personalInfo.birthDate) return null;
        const today = new Date();
        const birthDate = new Date(state.personalInfo.birthDate);
        let age = today.getFullYear() - birthDate.getFullYear();
        const monthDiff = today.getMonth() - birthDate.getMonth();
        if (monthDiff < 0 || (monthDiff === 0 && today.getDate() < birthDate.getDate())) {
            age--;
        }
        return age;
    })();

    // Calculate years to retirement
    const yearsToRetirement = (() => {
        if (!currentAge || !state.retirement.targetAge) return null;
        return Math.max(0, state.retirement.targetAge - currentAge);
    })();

    // Calculate total investment accounts by tax type
    const investmentsByTaxType = (() => {
        const taxDeferred = state.investments
            .filter((inv) => inv.type === 'Traditional401k' || inv.type === 'TraditionalIRA')
            .reduce((sum, inv) => sum + inv.balance, 0);

        const taxFree = state.investments
            .filter((inv) => inv.type === 'Roth401k' || inv.type === 'RothIRA' || inv.type === 'HSA')
            .reduce((sum, inv) => sum + inv.balance, 0);

        const taxable = state.investments
            .filter((inv) => inv.type === 'Brokerage')
            .reduce((sum, inv) => sum + inv.balance, 0);

        const other = state.investments
            .filter((inv) => inv.type === 'Other')
            .reduce((sum, inv) => sum + inv.balance, 0);

        return { taxDeferred, taxFree, taxable, other };
    })();

    // Format currency
    const formatCurrency = (amount: number) =>
        new Intl.NumberFormat('en-US', {
            style: 'currency',
            currency: 'USD',
            maximumFractionDigits: 0,
        }).format(amount);

    return {
        netWorth,
        monthlyIncome,
        monthlyExpenses,
        currentAge,
        yearsToRetirement,
        investmentsByTaxType,
        formatCurrency,
    };
}
