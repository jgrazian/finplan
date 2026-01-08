# Wizard V1 vs V2 Comparison

## High-Level Differences

| Aspect | V1 (Current) | V2 (New) |
|--------|-------------|----------|
| **Steps** | 7 technical steps | 10 conversational sections |
| **State Management** | React useState (prop drilling) | Zustand (global store) |
| **User Flow** | Starts with portfolio selection | Starts with personal context |
| **Language** | Technical (tax-deferred, profiles) | Conversational (savings, retirement) |
| **Guidance** | Minimal explanations | Contextual help throughout |
| **Validation** | Limited | Real-time with warnings |
| **Progress Tracking** | Basic step counter | Live financial summary |
| **Mobile** | Desktop-first | Mobile-first design |

## Step-by-Step Comparison

### V1 Step 1: Basics
**What it asks:**
- Simulation name
- Start date and birth date (raw date pickers)
- Portfolio selection (dropdown)
- Duration years

**Problems:**
- Asks for portfolio before context
- No explanation of why these matter
- Technical date inputs

### V2 Steps 0-1: Welcome & About You
**What it asks:**
- Simulation name with explanation
- Goal selection (retire early, savings target, etc.)
- Birth date with age calculation
- Filing status (affects taxes)
- State (with tax rate info)

**Improvements:**
- Sets context first
- Explains why each field matters
- Shows immediate feedback (age, tax rates)
- More intuitive date picker

---

### V1 Step 2: Profiles
**What it asks:**
- Inflation profile (mean, std dev)
- Return profiles for each asset class
- Advanced statistical parameters

**Problems:**
- Too technical for most users
- No guidance on reasonable values
- Exposed early in process

### V2 Approach
**Deferred to Review step:**
- Smart defaults based on historical data
- Hidden by default in "Advanced Options"
- Editable but not required

---

### V1 Step 3: Asset Linking
**What it asks:**
- Map each asset to a return profile index
- Map portfolio to inflation profile

**Problems:**
- Confusing for non-experts
- Manual ID management
- No preview of effects

### V2 Approach
**Automatic:**
- Asset allocations automatically mapped
- Based on account type and user selections
- No manual linking required

---

### V1 Step 4: Cash Flows
**What it asks:**
- Manual income/expense entries
- Account/asset ID selection
- Repeat intervals
- Inflation adjustment flags

**Problems:**
- Requires knowing account structure
- No guidance on common scenarios
- Manual ID entry prone to errors

### V2 Steps 2-3: Income & Savings
**What it asks:**
- Employment status (conversational)
- Salary with monthly equivalents
- 401(k) details with match calculator
- Bank accounts with balances
- Emergency fund guidance

**Improvements:**
- Conversational questions
- Auto-creates accounts behind scenes
- Built-in calculators (match, emergency fund)
- Shows months of expenses covered
- Validates IRS limits

---

### V1 Step 5: Events
**Status:** Shows "coming soon" placeholder

**Problems:**
- Not implemented
- Most powerful feature unavailable

### V2 Steps 4-8: Full Financial Picture
**What it asks:**
- Investment accounts (401k, IRA, brokerage)
- Real estate (home, rentals, mortgages)
- Debts (student loans, car loans, cards)
- Retirement goals (age, income, SS)
- Life events (purchases, education, etc.)

**Improvements:**
- All major financial components covered
- Event system fully integrated
- Visual timeline of events
- Smart suggestions based on age/income

---

### V1 Step 6: Spending
**What it asks:**
- Retirement spending targets
- Withdrawal strategy dropdown
- Start age

**Problems:**
- No context on reasonable amounts
- Strategies not explained
- Disconnect from overall picture

### V2 Step 7: Retirement Goals
**What it asks:**
- Target retirement age
- Income needs (with calculator)
- Social Security claiming strategy
- Pension income
- 4% rule guidance

**Improvements:**
- Built-in spending calculator
- Social Security optimization
- Shows if savings are sufficient
- Explains withdrawal strategies

---

### V1 Step 7: Review
**What it shows:**
- JSON dump of parameters
- No summary or insights
- Just a submit button

**Problems:**
- Hard to verify correctness
- No actionable feedback
- Doesn't highlight issues

### V2 Step 9: Review & Refine
**What it shows:**
- Complete financial snapshot
- Monthly cash flow summary
- Retirement readiness score
- Editable assumptions
- Potential issues highlighted
- "Quick fixes" suggestions

**Improvements:**
- Human-readable summary
- Interactive adjustments
- Warning about problems
- PDF export option
- Confidence score

---

## Code Architecture Comparison

### V1: Props & State
```tsx
// Props drilled through wizard
const [parameters, setParameters] = useState<SimulationParameters>({...});
const [name, setName] = useState("");
const [description, setDescription] = useState("");
// ... 10+ more state variables

// Passed to each step
<BasicsStep 
  parameters={parameters}
  updateParameters={updateParameters}
  name={name}
  setName={setName}
  // ... 5+ more props
/>
```

### V2: Zustand Store
```tsx
// Global store, accessed anywhere
const simulationName = useWizardStore(state => state.simulationName);
const setSimulationName = useWizardStore(state => state.setSimulationName);

// Steps are self-contained
<WelcomeStep />  // No props needed!
```

---

## State Shape Comparison

### V1: Backend-First
```typescript
interface SimulationParameters {
  birth_date: string;  // ISO date string
  duration_years: number;
  accounts: Account[];  // Raw account structure
  events: Event[];      // Complex event objects
  // ... backend structure
}
```

### V2: User-First (converts to backend)
```typescript
interface WizardState {
  personalInfo: {
    birthDate: Date;    // Native Date object
    filingStatus: FilingStatus;
    state: UsState;
  };
  income: {
    employed: boolean;
    salary: number;     // Just a number, not an event
    // ... human-friendly
  };
  // ... converts to SimulationParameters later
}
```

---

## Migration Path

### For Users
1. V2 can import from V1 simulations
2. V1 remains available during transition
3. Feature flag to enable V2 per user

### For Developers
1. V2 is completely separate (no breaking changes)
2. Shares backend API (SimulationParameters format)
3. Parameter builder converts V2 → V1 format
4. Can sunset V1 once V2 is feature complete

---

## Success Metrics

| Metric | V1 Baseline | V2 Target |
|--------|-------------|-----------|
| Completion Rate | ~40% | 70%+ |
| Time to Complete | 20+ minutes | <10 minutes |
| User Satisfaction (NPS) | 20 | 50+ |
| Error Rate | 15% | <5% |
| Return Usage | 30% | 60%+ |
| Mobile Usage | 10% | 40%+ |

---

## Next Steps for V2

**Immediate (Week 3-4):**
- [ ] Implement Investments step
- [ ] Implement Real Estate step
- [ ] Implement Debts step

**Near-term (Week 5-6):**
- [ ] Implement Retirement Goals step
- [ ] Implement Life Events step
- [ ] Build parameter conversion utility

**Before Launch:**
- [ ] Review & Refine step
- [ ] Draft autosave
- [ ] Backend API additions (filing status, etc.)
- [ ] Mobile testing
- [ ] Accessibility audit
- [ ] User testing
