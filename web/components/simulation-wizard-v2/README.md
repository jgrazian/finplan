# Simulation Wizard V2 - Implementation Progress

## Overview

This is the new conversational simulation wizard designed to provide a user-friendly experience for building comprehensive financial simulations. The wizard guides users through their financial situation like a friendly financial advisor.

## Implementation Status

### Phase 1: Foundation ✅ COMPLETE
- [x] Directory structure created
- [x] Zustand state management implemented
- [x] Type definitions created
- [x] Progress sidebar with live financial summary
- [x] Welcome step with goal selection
- [x] About You step with birth date, filing status, and state selection
- [x] State tax rate reference data

### Phase 2: Income & Savings ✅ COMPLETE
- [x] Current Income step with salary, pay frequency, and 401(k) details
- [x] Employer 401(k) match calculator
- [x] IRS contribution limit validation
- [x] Current Savings step with checking, savings, and HYSA
- [x] Emergency fund calculator with months of expenses
- [x] MoneyInput component with smart formatting
- [x] Smart income calculations and conversions

### Phase 3: Investments & Property ✅ COMPLETE
- [x] Step 4: Investments with all account types (401k, IRA, Roth, HSA, Brokerage)
- [x] Asset allocation specification (stocks, bonds, international, cash)
- [x] IRS contribution limit validation per account type
- [x] Investment summary by tax treatment
- [x] Step 5: Real Estate with property types (primary, investment, vacation)
- [x] Mortgage details with escrow tracking
- [x] Property tax and insurance calculations
- [x] Rental income tracking
- [x] Planned sale scenarios
- [x] Real estate equity calculations

### Phase 4: Debts & Retirement ✅ COMPLETE
- [x] Step 6: Debts with all debt types (student, car, credit card, etc.)
- [x] Payoff date calculator
- [x] High-interest debt warnings
- [x] Debt summary with total obligations
- [x] Step 7: Retirement Goals with target age and income
- [x] Social Security claiming strategy optimizer
- [x] Pension income integration
- [x] 4% rule calculator
- [x] Income replacement suggestions
- [x] Retirement income gap analysis

### Remaining Steps (To Be Implemented)
- [ ] Step 8: Life Events (Major purchases, education, inheritance)
- [ ] Step 9: Review & Refine (Complete summary and simulation execution)

## Architecture

### File Structure
```
web/components/simulation-wizard-v2/
├── index.tsx                        # Main wizard container
├── types.ts                         # TypeScript type definitions
├── hooks/
│   ├── useWizardStore.ts           # Zustand state management
│   └── useCalculations.ts          # Derived calculations (net worth, etc.)
├── components/
│   ├── WizardProgress.tsx          # Progress sidebar
│   └── MoneyInput.tsx              # Currency input component
├── steps/
│   ├── WelcomeStep.tsx             # Step 0: Welcome
│   ├── AboutYouStep.tsx            # Step 1: Personal info
│   ├── CurrentIncomeStep.tsx       # Step 2: Income sources
│   ├── CurrentSavingsStep.tsx      # Step 3: Liquid savings
│   ├── InvestmentsStep.tsx         # Step 4: Investment accounts
│   ├── RealEstateStep.tsx          # Step 5: Property & mortgages
│   ├── DebtsStep.tsx               # Step 6: Loans & debts
│   └── RetirementGoalsStep.tsx     # Step 7: Retirement planning
└── utils/
    └── taxData.ts                  # State tax rates reference
```

### State Management

Using Zustand for global state management with separate slices for:
- Personal information (birth date, filing status, state)
- Income (salary, frequency, 401k contributions)
- Savings (checking, savings, HYSA, emergency fund)
- Investments (to be implemented)
- Real estate (to be implemented)
- Debts (to be implemented)
- Retirement goals (to be implemented)
- Life events (to be implemented)

### Key Features Implemented

1. **Conversational Design**: Questions feel natural and guide users through the process
2. **Smart Defaults**: Pre-fills sensible values and provides context
3. **Live Calculations**: Progress sidebar shows running totals as user enters data
4. **Validation**: Warns about IRS limits, inadequate emergency funds, etc.
5. **Contextual Help**: Explanatory text for each question
6. **Progressive Disclosure**: Shows additional fields only when relevant

## Usage

### Accessing the Wizard

Navigate to `/simulations/new-v2` to see the new wizard.

### Example Flow

1. **Welcome**: Enter plan name and select goal
2. **About You**: Enter birth date, filing status, and state
3. **Income**: Enter salary, pay frequency, and 401(k) details
4. **Savings**: Enter checking, savings, HYSA, and emergency fund

## Next Steps

1. Implement remaining step components (Investments through Review)
2. Build parameter builder utility to convert wizard state to SimulationParameters
3. Implement draft saving functionality
4. Add backend support for new fields (filing status, state taxes)
5. Implement simulation execution from Review step
6. Add validation and error handling
7. Implement edit mode for existing simulations
8. Add accessibility features (keyboard navigation, ARIA labels)
9. Mobile responsive design improvements

## Testing

To test the wizard:
```bash
cd web
pnpm run dev
```

Then navigate to `http://localhost:3000/simulations/new-v2`

## Design Principles

1. **Conversational Tone**: Friendly, advisor-like language
2. **Progressive Disclosure**: Don't overwhelm with complexity
3. **Smart Defaults**: Pre-populate sensible values
4. **Visual Feedback**: Show running summary in sidebar
5. **Mobile-First**: Touch-friendly interface
6. **Validation**: Prevent errors with helpful warnings

## Technical Notes

- Uses Zustand for state management (lightweight, performant)
- Server components where possible, client components for interactivity
- Tailwind CSS for styling with shadcn/ui components
- TypeScript for type safety
- Date handling with date-fns
- Form validation with inline feedback

## Contributing

When adding new steps:
1. Create step component in `steps/` directory
2. Add to switch statement in main wizard component
3. Update state types in `types.ts`
4. Add actions to Zustand store if needed
5. Update calculations hook if new derived values needed
6. Test validation logic

## Known Issues

- Emergency fund months calculation uses rough expense estimate
- Tax calculations are simplified (should use actual brackets)
- No draft autosave yet (coming soon)
- Parameter builder not yet implemented
