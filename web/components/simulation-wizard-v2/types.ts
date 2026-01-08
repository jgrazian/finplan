// ============================================================================
// Wizard-specific Types
// ============================================================================

export type FilingStatus =
    | "Single"
    | "MarriedFilingJointly"
    | "MarriedFilingSeparately"
    | "HeadOfHousehold";

export type UsState =
    | "AL" | "AK" | "AZ" | "AR" | "CA" | "CO" | "CT" | "DE" | "FL" | "GA"
    | "HI" | "ID" | "IL" | "IN" | "IA" | "KS" | "KY" | "LA" | "ME" | "MD"
    | "MA" | "MI" | "MN" | "MS" | "MO" | "MT" | "NE" | "NV" | "NH" | "NJ"
    | "NM" | "NY" | "NC" | "ND" | "OH" | "OK" | "OR" | "PA" | "RI" | "SC"
    | "SD" | "TN" | "TX" | "UT" | "VT" | "VA" | "WA" | "WV" | "WI" | "WY";

export type PayFrequency = "Weekly" | "BiWeekly" | "SemiMonthly" | "Monthly";

export type RetirementGoalType =
    | "CanIRetireEarly"
    | "HowMuchToSave"
    | "WillMoneyLast"
    | "JustExploring";

export interface PersonalInfo {
    birthDate: Date | null;
    filingStatus: FilingStatus | null;
    state: UsState | null;
}

export interface Employer401kInfo {
    hasMatch: boolean;
    matchPercentage: number;
    matchUpTo: number;
    employeeContribution: number;
}

export interface IncomeSource {
    id: string;
    description: string;
    amount: number;
    frequency: PayFrequency;
}

export interface IncomeInfo {
    employed: boolean;
    salary: number;
    payFrequency: PayFrequency;
    employer401k: Employer401kInfo | null;
    otherIncome: IncomeSource[];
}

export interface SavingsInfo {
    checking: number;
    savings: number;
    hysa: number;
    hysaRate: number;
    emergencyFund: number;
}

export interface InvestmentAccount {
    id: string;
    type: "Brokerage" | "Traditional401k" | "Roth401k" | "TraditionalIRA" | "RothIRA" | "HSA" | "Other";
    balance: number;
    contributions?: {
        amount: number;
        frequency: PayFrequency;
    };
    allocation?: {
        stocks: number;
        bonds: number;
        international: number;
        cash: number;
    };
}

export interface RealEstateProperty {
    id: string;
    type: "PrimaryResidence" | "Investment" | "Vacation";
    value: number;
    mortgage?: {
        balance: number;
        monthlyPayment: number;
        interestRate: number;
        yearsRemaining: number;
        includesPropertyTax: boolean;
        includesInsurance: boolean;
    };
    propertyTax?: number;
    insurance?: number;
    rentalIncome?: number;
    plannedSale?: {
        trigger: "Retirement" | "SpecificAge" | "SpecificValue";
        value?: number;
        age?: number;
    };
}

export interface Debt {
    id: string;
    type: "StudentLoan" | "CarLoan" | "CreditCard" | "Personal" | "Medical" | "Other";
    balance: number;
    monthlyPayment: number;
    interestRate: number;
    description?: string;
}

export interface SocialSecurityPlan {
    hasSSI: boolean;
    estimatedBenefit?: number;
    claimingAge?: number;
}

export interface PensionPlan {
    hasPension: boolean;
    monthlyAmount?: number;
    startAge?: number;
}

export interface RetirementInfo {
    targetAge: number | null;
    targetIncome: number | null;
    socialSecurity: SocialSecurityPlan;
    pension: PensionPlan;
}

export type LifeEventType =
    | "CareerChange"
    | "MajorPurchase"
    | "ChildEducation"
    | "Wedding"
    | "HomeRenovation"
    | "Inheritance"
    | "Downsizing"
    | "StartBusiness"
    | "Healthcare"
    | "Custom";

export interface LifeEvent {
    id: string;
    type: LifeEventType;
    description: string;
    yearsFromNow: number;
    amount: number;
    recurring?: {
        duration: number;
        inflationAdjusted: boolean;
    };
}

export interface WizardState {
    // Basic info
    simulationName: string;
    goal: RetirementGoalType | null;

    // Personal
    personalInfo: PersonalInfo;

    // Financial state
    income: IncomeInfo;
    savings: SavingsInfo;
    investments: InvestmentAccount[];
    realEstate: RealEstateProperty[];
    debts: Debt[];

    // Goals
    retirement: RetirementInfo;
    lifeEvents: LifeEvent[];

    // Wizard navigation
    currentStep: number;
    completedSteps: Set<number>;
}
