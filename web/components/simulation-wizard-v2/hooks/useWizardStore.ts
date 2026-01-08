import { create } from 'zustand';
import { WizardState, PayFrequency, FilingStatus, UsState } from '../types';

const initialState: Omit<WizardState, 'setPersonalInfo' | 'setIncome' | 'setSavings' | 'setRetirement' | 'addInvestment' | 'updateInvestment' | 'removeInvestment' | 'addRealEstate' | 'updateRealEstate' | 'removeRealEstate' | 'addDebt' | 'updateDebt' | 'removeDebt' | 'addLifeEvent' | 'updateLifeEvent' | 'removeLifeEvent' | 'nextStep' | 'prevStep' | 'goToStep' | 'markStepComplete' | 'resetWizard'> = {
    simulationName: '',
    goal: null,

    personalInfo: {
        birthDate: null,
        filingStatus: null,
        state: null,
    },

    income: {
        employed: false,
        salary: 0,
        payFrequency: 'Monthly' as PayFrequency,
        employer401k: null,
        otherIncome: [],
    },

    savings: {
        checking: 0,
        savings: 0,
        hysa: 0,
        hysaRate: 4.5,
        emergencyFund: 0,
    },

    investments: [],
    realEstate: [],
    debts: [],

    retirement: {
        targetAge: null,
        targetIncome: null,
        socialSecurity: {
            hasSSI: true,
            estimatedBenefit: undefined,
            claimingAge: 67,
        },
        pension: {
            hasPension: false,
        },
    },

    lifeEvents: [],

    currentStep: 0,
    completedSteps: new Set<number>(),
};

interface WizardActions {
    // Basic info
    setSimulationName: (name: string) => void;
    setGoal: (goal: WizardState['goal']) => void;

    // Personal info
    setPersonalInfo: (info: Partial<WizardState['personalInfo']>) => void;

    // Income
    setIncome: (income: Partial<WizardState['income']>) => void;
    addOtherIncome: (source: WizardState['income']['otherIncome'][0]) => void;
    updateOtherIncome: (id: string, updates: Partial<WizardState['income']['otherIncome'][0]>) => void;
    removeOtherIncome: (id: string) => void;

    // Savings
    setSavings: (savings: Partial<WizardState['savings']>) => void;

    // Investments
    addInvestment: (investment: WizardState['investments'][0]) => void;
    updateInvestment: (id: string, updates: Partial<WizardState['investments'][0]>) => void;
    removeInvestment: (id: string) => void;

    // Real Estate
    addRealEstate: (property: WizardState['realEstate'][0]) => void;
    updateRealEstate: (id: string, updates: Partial<WizardState['realEstate'][0]>) => void;
    removeRealEstate: (id: string) => void;

    // Debts
    addDebt: (debt: WizardState['debts'][0]) => void;
    updateDebt: (id: string, updates: Partial<WizardState['debts'][0]>) => void;
    removeDebt: (id: string) => void;

    // Retirement
    setRetirement: (retirement: Partial<WizardState['retirement']>) => void;

    // Life Events
    addLifeEvent: (event: WizardState['lifeEvents'][0]) => void;
    updateLifeEvent: (id: string, updates: Partial<WizardState['lifeEvents'][0]>) => void;
    removeLifeEvent: (id: string) => void;

    // Navigation
    nextStep: () => void;
    prevStep: () => void;
    goToStep: (step: number) => void;
    markStepComplete: (step: number) => void;

    // Reset
    resetWizard: () => void;
}

export const useWizardStore = create<WizardState & WizardActions>((set) => ({
    ...initialState,

    // Basic info
    setSimulationName: (name) => set({ simulationName: name }),
    setGoal: (goal) => set({ goal }),

    // Personal info
    setPersonalInfo: (info) =>
        set((state) => ({
            personalInfo: { ...state.personalInfo, ...info },
        })),

    // Income
    setIncome: (income) =>
        set((state) => ({
            income: { ...state.income, ...income },
        })),

    addOtherIncome: (source) =>
        set((state) => ({
            income: {
                ...state.income,
                otherIncome: [...state.income.otherIncome, source],
            },
        })),

    updateOtherIncome: (id, updates) =>
        set((state) => ({
            income: {
                ...state.income,
                otherIncome: state.income.otherIncome.map((s) =>
                    s.id === id ? { ...s, ...updates } : s
                ),
            },
        })),

    removeOtherIncome: (id) =>
        set((state) => ({
            income: {
                ...state.income,
                otherIncome: state.income.otherIncome.filter((s) => s.id !== id),
            },
        })),

    // Savings
    setSavings: (savings) =>
        set((state) => ({
            savings: { ...state.savings, ...savings },
        })),

    // Investments
    addInvestment: (investment) =>
        set((state) => ({
            investments: [...state.investments, investment],
        })),

    updateInvestment: (id, updates) =>
        set((state) => ({
            investments: state.investments.map((inv) =>
                inv.id === id ? { ...inv, ...updates } : inv
            ),
        })),

    removeInvestment: (id) =>
        set((state) => ({
            investments: state.investments.filter((inv) => inv.id !== id),
        })),

    // Real Estate
    addRealEstate: (property) =>
        set((state) => ({
            realEstate: [...state.realEstate, property],
        })),

    updateRealEstate: (id, updates) =>
        set((state) => ({
            realEstate: state.realEstate.map((prop) =>
                prop.id === id ? { ...prop, ...updates } : prop
            ),
        })),

    removeRealEstate: (id) =>
        set((state) => ({
            realEstate: state.realEstate.filter((prop) => prop.id !== id),
        })),

    // Debts
    addDebt: (debt) =>
        set((state) => ({
            debts: [...state.debts, debt],
        })),

    updateDebt: (id, updates) =>
        set((state) => ({
            debts: state.debts.map((debt) =>
                debt.id === id ? { ...debt, ...updates } : debt
            ),
        })),

    removeDebt: (id) =>
        set((state) => ({
            debts: state.debts.filter((debt) => debt.id !== id),
        })),

    // Retirement
    setRetirement: (retirement) =>
        set((state) => ({
            retirement: { ...state.retirement, ...retirement },
        })),

    // Life Events
    addLifeEvent: (event) =>
        set((state) => ({
            lifeEvents: [...state.lifeEvents, event],
        })),

    updateLifeEvent: (id, updates) =>
        set((state) => ({
            lifeEvents: state.lifeEvents.map((event) =>
                event.id === id ? { ...event, ...updates } : event
            ),
        })),

    removeLifeEvent: (id) =>
        set((state) => ({
            lifeEvents: state.lifeEvents.filter((event) => event.id !== id),
        })),

    // Navigation
    nextStep: () =>
        set((state) => ({
            currentStep: Math.min(state.currentStep + 1, 9), // 10 steps (0-9)
        })),

    prevStep: () =>
        set((state) => ({
            currentStep: Math.max(state.currentStep - 1, 0),
        })),

    goToStep: (step) =>
        set(() => ({
            currentStep: Math.max(0, Math.min(step, 9)),
        })),

    markStepComplete: (step) =>
        set((state) => ({
            completedSteps: new Set([...state.completedSteps, step]),
        })),

    // Reset
    resetWizard: () => set(initialState),
}));
