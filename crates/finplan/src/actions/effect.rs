// Effect actions - managing effects on events

use crate::data::events_data::{
    AccountTag, AmountData, EffectData, EventTag, LotMethodData, WithdrawalStrategyData,
};
use crate::data::portfolio_data::AssetTag;
use crate::modals::parse_currency;
use crate::screens::events::EventsScreen;
use crate::state::context::{EffectContext, EffectTypeContext, ModalContext};
use crate::state::{
    AppState, ConfirmModal, FormField, FormModal, ModalAction, ModalState, PickerModal,
};

use super::{ActionContext, ActionResult};

/// Handle effect management picker selection
pub fn handle_manage_effects(state: &AppState, selected: &str) -> ActionResult {
    let event_idx = state.events_state.selected_event_index;

    if selected == "[ + Add New Effect ]" {
        // Show effect type picker with all 10 effect types
        let effect_types = vec![
            "Income".to_string(),
            "Expense".to_string(),
            "Asset Purchase".to_string(),
            "Asset Sale".to_string(),
            "Sweep".to_string(),
            "Trigger Event".to_string(),
            "Pause Event".to_string(),
            "Resume Event".to_string(),
            "Terminate Event".to_string(),
            "Apply RMD".to_string(),
            "Adjust Balance".to_string(),
            "Cash Transfer".to_string(),
        ];
        return ActionResult::modal(ModalState::Picker(PickerModal::new(
            "Select Effect Type",
            effect_types,
            ModalAction::PICK_EFFECT_TYPE_FOR_ADD,
        )));
    }

    // Parse effect index from "N. description" format
    if let Some(effect_idx) = selected
        .split('.')
        .next()
        .and_then(|s| s.parse::<usize>().ok())
    {
        let effect_idx = effect_idx - 1; // Convert to 0-based index
        if state.data().events.get(event_idx).is_some()
            && state
                .data()
                .events
                .get(event_idx)
                .map(|e| e.effects.get(effect_idx).is_some())
                .unwrap_or(false)
        {
            // Show Edit/Delete picker
            return ActionResult::modal(ModalState::Picker(
                PickerModal::new(
                    "Effect Action",
                    vec!["Edit Effect".to_string(), "Delete Effect".to_string()],
                    ModalAction::PICK_ACTION_FOR_EFFECT,
                )
                .with_typed_context(ModalContext::effect_existing(event_idx, effect_idx)),
            ));
        }
    }
    ActionResult::close()
}

/// Handle the Edit/Delete picker for effects
pub fn handle_action_for_effect_pick(
    state: &AppState,
    selected: &str,
    ctx: ActionContext,
) -> ActionResult {
    // Get typed effect context
    let effect_ctx = ctx
        .typed_context()
        .and_then(|c| c.as_effect())
        .and_then(|e| {
            if let EffectContext::Existing { event, effect } = e {
                Some((*event, *effect))
            } else {
                None
            }
        });

    let (event_idx, effect_idx) = match effect_ctx {
        Some(indices) => indices,
        None => return ActionResult::close(),
    };

    let Some(event) = state.data().events.get(event_idx) else {
        return ActionResult::close();
    };
    let Some(effect) = event.effects.get(effect_idx) else {
        return ActionResult::close();
    };

    match selected {
        "Edit Effect" => {
            // Build edit form based on effect type
            build_edit_form_for_effect(state, effect, event_idx, effect_idx)
        }
        "Delete Effect" => {
            let effect_desc = EventsScreen::format_effect(effect);
            ActionResult::modal(ModalState::Confirm(
                ConfirmModal::new(
                    "Delete Effect",
                    &format!("Delete effect: {}?", effect_desc),
                    ModalAction::DELETE_EFFECT,
                )
                .with_typed_context(ModalContext::effect_existing(event_idx, effect_idx)),
            ))
        }
        _ => ActionResult::close(),
    }
}

/// Build an edit form for the given effect
fn build_edit_form_for_effect(
    state: &AppState,
    effect: &EffectData,
    event_idx: usize,
    effect_idx: usize,
) -> ActionResult {
    let accounts = EventsScreen::get_account_names(state);
    let events = EventsScreen::get_event_names(state);

    match effect {
        EffectData::Income {
            to,
            amount,
            gross,
            taxable,
        } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Income Effect",
                vec![
                    FormField::select("To Account", accounts, &to.0),
                    FormField::currency("Amount", amount_to_f64(amount)),
                    FormField::select("Gross", yes_no_options(), if *gross { "Yes" } else { "No" }),
                    FormField::select(
                        "Taxable",
                        yes_no_options(),
                        if *taxable { "Yes" } else { "No" },
                    ),
                ],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::Income,
            ))
            .start_editing(),
        )),

        EffectData::Expense { from, amount } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Expense Effect",
                vec![
                    FormField::select("From Account", accounts, &from.0),
                    FormField::currency("Amount", amount_to_f64(amount)),
                ],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::Expense,
            ))
            .start_editing(),
        )),

        EffectData::AssetPurchase {
            from,
            to_account,
            asset,
            amount,
        } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Asset Purchase",
                vec![
                    FormField::select("From Account", accounts.clone(), &from.0),
                    FormField::select("To Account", accounts, &to_account.0),
                    FormField::text("Asset", &asset.0),
                    FormField::currency("Amount", amount_to_f64(amount)),
                ],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::AssetPurchase,
            ))
            .start_editing(),
        )),

        EffectData::AssetSale {
            from,
            asset,
            amount,
            gross,
            lot_method,
        } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Asset Sale",
                vec![
                    FormField::select("From Account", accounts, &from.0),
                    FormField::text(
                        "Asset (blank=liquidate)",
                        asset.as_ref().map(|a| a.0.as_str()).unwrap_or(""),
                    ),
                    FormField::currency("Amount", amount_to_f64(amount)),
                    FormField::select("Gross", yes_no_options(), if *gross { "Yes" } else { "No" }),
                    FormField::select(
                        "Lot Method",
                        lot_method_options(),
                        lot_method_to_display(*lot_method),
                    ),
                ],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::AssetSale,
            ))
            .start_editing(),
        )),

        EffectData::Sweep {
            to,
            amount,
            strategy,
            gross,
            taxable,
            lot_method,
            ..
        } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Sweep",
                vec![
                    FormField::select("To Account", accounts, &to.0),
                    FormField::currency("Amount", amount_to_f64(amount)),
                    FormField::select(
                        "Strategy",
                        strategy_options(),
                        strategy_to_display(*strategy),
                    ),
                    FormField::select("Gross", yes_no_options(), if *gross { "Yes" } else { "No" }),
                    FormField::select(
                        "Taxable",
                        yes_no_options(),
                        if *taxable { "Yes" } else { "No" },
                    ),
                    FormField::select(
                        "Lot Method",
                        lot_method_options(),
                        lot_method_to_display(*lot_method),
                    ),
                ],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::Sweep,
            ))
            .start_editing(),
        )),

        EffectData::TriggerEvent { event } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Trigger Effect",
                vec![FormField::select("Event to Trigger", events, &event.0)],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::TriggerEvent,
            ))
            .start_editing(),
        )),

        EffectData::PauseEvent { event } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Pause Effect",
                vec![FormField::select("Event to Pause", events, &event.0)],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::PauseEvent,
            ))
            .start_editing(),
        )),

        EffectData::ResumeEvent { event } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Resume Effect",
                vec![FormField::select("Event to Resume", events, &event.0)],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::ResumeEvent,
            ))
            .start_editing(),
        )),

        EffectData::TerminateEvent { event } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Terminate Effect",
                vec![FormField::select("Event to Terminate", events, &event.0)],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::TerminateEvent,
            ))
            .start_editing(),
        )),

        EffectData::ApplyRmd {
            destination,
            lot_method,
        } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Apply RMD",
                vec![
                    FormField::select("Destination Account", accounts, &destination.0),
                    FormField::select(
                        "Lot Method",
                        lot_method_options(),
                        lot_method_to_display(*lot_method),
                    ),
                ],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::ApplyRmd,
            ))
            .start_editing(),
        )),

        EffectData::AdjustBalance { account, amount } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Adjust Balance",
                vec![
                    FormField::select("Account", accounts, &account.0),
                    FormField::currency("Amount (+/- to adjust)", amount_to_f64(amount)),
                ],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::AdjustBalance,
            ))
            .start_editing(),
        )),

        EffectData::CashTransfer { from, to, amount } => ActionResult::modal(ModalState::Form(
            FormModal::new(
                "Edit Cash Transfer",
                vec![
                    FormField::select("From Account", accounts.clone(), &from.0),
                    FormField::select("To Account", accounts, &to.0),
                    FormField::currency("Amount", amount_to_f64(amount)),
                ],
                ModalAction::EDIT_EFFECT,
            )
            .with_typed_context(ModalContext::effect_edit(
                event_idx,
                effect_idx,
                EffectTypeContext::CashTransfer,
            ))
            .start_editing(),
        )),
    }
}

/// Convert AmountData to f64 for form display
fn amount_to_f64(amount: &AmountData) -> f64 {
    match amount {
        AmountData::Fixed(v) => *v,
        AmountData::Special(_) => 0.0, // Special amounts shown as 0, user can edit
    }
}

/// Convert LotMethodData to display string (matching select options)
fn lot_method_to_display(method: LotMethodData) -> &'static str {
    match method {
        LotMethodData::Fifo => "FIFO",
        LotMethodData::Lifo => "LIFO",
        LotMethodData::HighestCost => "Highest Cost",
        LotMethodData::LowestCost => "Lowest Cost",
        LotMethodData::AverageCost => "Average Cost",
    }
}

/// Convert WithdrawalStrategyData to display string (matching select options)
fn strategy_to_display(strategy: WithdrawalStrategyData) -> &'static str {
    match strategy {
        WithdrawalStrategyData::TaxEfficient => "Tax Efficient",
        WithdrawalStrategyData::TaxDeferredFirst => "Tax-Deferred First",
        WithdrawalStrategyData::TaxFreeFirst => "Tax-Free First",
        WithdrawalStrategyData::ProRata => "Pro Rata",
        WithdrawalStrategyData::PenaltyAware => "Penalty Aware",
    }
}

/// Parse lot method from string
fn parse_lot_method(s: &str) -> LotMethodData {
    match s.to_lowercase().as_str() {
        "lifo" => LotMethodData::Lifo,
        "hc" | "highestcost" | "highest cost" => LotMethodData::HighestCost,
        "lc" | "lowestcost" | "lowest cost" => LotMethodData::LowestCost,
        "avg" | "averagecost" | "average cost" => LotMethodData::AverageCost,
        _ => LotMethodData::Fifo, // Default to FIFO
    }
}

/// Parse withdrawal strategy from string
fn parse_strategy(s: &str) -> WithdrawalStrategyData {
    match s.to_lowercase().as_str() {
        "tdf" | "taxdeferredfirst" | "tax-deferred first" => {
            WithdrawalStrategyData::TaxDeferredFirst
        }
        "tff" | "taxfreefirst" | "tax-free first" => WithdrawalStrategyData::TaxFreeFirst,
        "pr" | "prorata" | "pro rata" => WithdrawalStrategyData::ProRata,
        "pa" | "penaltyaware" | "penalty aware" => WithdrawalStrategyData::PenaltyAware,
        _ => WithdrawalStrategyData::TaxEfficient, // Default to TaxEfficient
    }
}

/// Parse yes/no field to bool
fn parse_yes_no(s: &str) -> bool {
    matches!(s.to_lowercase().as_str(), "yes" | "y" | "true" | "1")
}

// Common select options
fn yes_no_options() -> Vec<String> {
    vec!["No".to_string(), "Yes".to_string()]
}

fn lot_method_options() -> Vec<String> {
    vec![
        "FIFO".to_string(),
        "LIFO".to_string(),
        "Highest Cost".to_string(),
        "Lowest Cost".to_string(),
    ]
}

fn strategy_options() -> Vec<String> {
    vec![
        "Tax Efficient".to_string(),
        "Tax-Deferred First".to_string(),
        "Tax-Free First".to_string(),
        "Pro Rata".to_string(),
        "Penalty Aware".to_string(),
    ]
}

/// Handle effect type selection for adding new effect
pub fn handle_effect_type_for_add(state: &AppState, effect_type: &str) -> ActionResult {
    let event_idx = state.events_state.selected_event_index;
    let accounts = EventsScreen::get_account_names(state);
    let events = EventsScreen::get_event_names(state);

    let first_account = accounts.first().cloned().unwrap_or_default();
    let first_event = events.first().cloned().unwrap_or_default();

    match effect_type {
        "Income" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Income Effect",
                    vec![
                        FormField::select("To Account", accounts, &first_account),
                        FormField::currency("Amount", 0.0),
                        FormField::select("Gross", yes_no_options(), "No"),
                        FormField::select("Taxable", yes_no_options(), "Yes"),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::Income,
                ))
                .start_editing(),
            ))
        }
        "Expense" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Expense Effect",
                    vec![
                        FormField::select("From Account", accounts, &first_account),
                        FormField::currency("Amount", 0.0),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::Expense,
                ))
                .start_editing(),
            ))
        }
        "Trigger Event" => {
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Trigger Effect",
                    vec![FormField::select("Event to Trigger", events, &first_event)],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::TriggerEvent,
                ))
                .start_editing(),
            ))
        }
        "Pause Event" => {
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Pause Effect",
                    vec![FormField::select("Event to Pause", events, &first_event)],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::PauseEvent,
                ))
                .start_editing(),
            ))
        }
        "Resume Event" => {
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Resume Effect",
                    vec![FormField::select("Event to Resume", events, &first_event)],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::ResumeEvent,
                ))
                .start_editing(),
            ))
        }
        "Terminate Event" => {
            if events.is_empty() {
                return ActionResult::error("No events available. Create an event first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Terminate Effect",
                    vec![FormField::select(
                        "Event to Terminate",
                        events,
                        &first_event,
                    )],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::TerminateEvent,
                ))
                .start_editing(),
            ))
        }
        "Asset Purchase" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Asset Purchase",
                    vec![
                        FormField::select("From Account", accounts.clone(), &first_account),
                        FormField::select("To Account", accounts, &first_account),
                        FormField::text("Asset", ""),
                        FormField::currency("Amount", 0.0),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::AssetPurchase,
                ))
                .start_editing(),
            ))
        }
        "Asset Sale" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Asset Sale",
                    vec![
                        FormField::select("From Account", accounts, &first_account),
                        FormField::text("Asset (blank=liquidate)", ""),
                        FormField::currency("Amount", 0.0),
                        FormField::select("Gross", yes_no_options(), "No"),
                        FormField::select("Lot Method", lot_method_options(), "FIFO"),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::AssetSale,
                ))
                .start_editing(),
            ))
        }
        "Sweep" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Sweep",
                    vec![
                        FormField::select("To Account", accounts, &first_account),
                        FormField::currency("Amount", 0.0),
                        FormField::select("Strategy", strategy_options(), "Tax Efficient"),
                        FormField::select("Gross", yes_no_options(), "No"),
                        FormField::select("Taxable", yes_no_options(), "Yes"),
                        FormField::select("Lot Method", lot_method_options(), "FIFO"),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::Sweep,
                ))
                .start_editing(),
            ))
        }
        "Apply RMD" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Apply RMD",
                    vec![
                        FormField::select("Destination Account", accounts, &first_account),
                        FormField::select("Lot Method", lot_method_options(), "FIFO"),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::ApplyRmd,
                ))
                .start_editing(),
            ))
        }
        "Adjust Balance" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Adjust Balance",
                    vec![
                        FormField::select("Account", accounts, &first_account),
                        FormField::currency("Amount (+/- to adjust)", 0.0),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::AdjustBalance,
                ))
                .start_editing(),
            ))
        }
        "Cash Transfer" => {
            if accounts.is_empty() {
                return ActionResult::error("No accounts available. Create an account first.");
            }
            ActionResult::modal(ModalState::Form(
                FormModal::new(
                    "New Cash Transfer",
                    vec![
                        FormField::select("From Account", accounts.clone(), &first_account),
                        FormField::select("To Account", accounts, &first_account),
                        FormField::currency("Amount", 0.0),
                    ],
                    ModalAction::ADD_EFFECT,
                )
                .with_typed_context(ModalContext::effect_add(
                    event_idx,
                    EffectTypeContext::CashTransfer,
                ))
                .start_editing(),
            ))
        }
        _ => ActionResult::close(),
    }
}

/// Handle adding an effect to an event
pub fn handle_add_effect(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    // Get typed effect context
    let (event_idx, effect_type) = match ctx.typed_context().and_then(|c| c.as_effect()) {
        Some(EffectContext::Add { event, effect_type }) => (*event, effect_type.clone()),
        _ => return ActionResult::close(),
    };

    let form_parts = ctx.value_parts();

    let effect = match effect_type {
        EffectTypeContext::Income => {
            let to_account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);
            let gross = form_parts.get(2).map(|s| parse_yes_no(s)).unwrap_or(false);
            let taxable = form_parts.get(3).map(|s| parse_yes_no(s)).unwrap_or(true);

            EffectData::Income {
                to: AccountTag(to_account),
                amount: AmountData::Fixed(amount),
                gross,
                taxable,
            }
        }
        EffectTypeContext::Expense => {
            let from_account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);

            EffectData::Expense {
                from: AccountTag(from_account),
                amount: AmountData::Fixed(amount),
            }
        }
        EffectTypeContext::TriggerEvent => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            EffectData::TriggerEvent {
                event: EventTag(event_name),
            }
        }
        EffectTypeContext::PauseEvent => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            EffectData::PauseEvent {
                event: EventTag(event_name),
            }
        }
        EffectTypeContext::ResumeEvent => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            EffectData::ResumeEvent {
                event: EventTag(event_name),
            }
        }
        EffectTypeContext::TerminateEvent => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            EffectData::TerminateEvent {
                event: EventTag(event_name),
            }
        }
        EffectTypeContext::AssetPurchase => {
            let from_account = form_parts.first().unwrap_or(&"").to_string();
            let to_account = form_parts.get(1).unwrap_or(&"").to_string();
            let asset = form_parts.get(2).unwrap_or(&"").to_string();
            let amount = form_parts
                .get(3)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);

            EffectData::AssetPurchase {
                from: AccountTag(from_account),
                to_account: AccountTag(to_account),
                asset: AssetTag(asset),
                amount: AmountData::Fixed(amount),
            }
        }
        EffectTypeContext::AssetSale => {
            let from_account = form_parts.first().unwrap_or(&"").to_string();
            let asset_str = form_parts.get(1).unwrap_or(&"").trim();
            let asset = if asset_str.is_empty() {
                None
            } else {
                Some(AssetTag(asset_str.to_string()))
            };
            let amount = form_parts
                .get(2)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);
            let gross = form_parts.get(3).map(|s| parse_yes_no(s)).unwrap_or(false);
            let lot_method = form_parts
                .get(4)
                .map(|s| parse_lot_method(s))
                .unwrap_or_default();

            EffectData::AssetSale {
                from: AccountTag(from_account),
                asset,
                amount: AmountData::Fixed(amount),
                gross,
                lot_method,
            }
        }
        EffectTypeContext::Sweep => {
            let to_account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);
            let strategy = form_parts
                .get(2)
                .map(|s| parse_strategy(s))
                .unwrap_or_default();
            let gross = form_parts.get(3).map(|s| parse_yes_no(s)).unwrap_or(false);
            let taxable = form_parts.get(4).map(|s| parse_yes_no(s)).unwrap_or(true);
            let lot_method = form_parts
                .get(5)
                .map(|s| parse_lot_method(s))
                .unwrap_or_default();

            EffectData::Sweep {
                to: AccountTag(to_account),
                amount: AmountData::Fixed(amount),
                strategy,
                gross,
                taxable,
                lot_method,
                exclude_accounts: vec![],
            }
        }
        EffectTypeContext::ApplyRmd => {
            let destination = form_parts.first().unwrap_or(&"").to_string();
            let lot_method = form_parts
                .get(1)
                .map(|s| parse_lot_method(s))
                .unwrap_or_default();

            EffectData::ApplyRmd {
                destination: AccountTag(destination),
                lot_method,
            }
        }
        EffectTypeContext::AdjustBalance => {
            let account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);

            EffectData::AdjustBalance {
                account: AccountTag(account),
                amount: AmountData::Fixed(amount),
            }
        }
        EffectTypeContext::CashTransfer => {
            let from_account = form_parts.first().unwrap_or(&"").to_string();
            let to_account = form_parts.get(1).unwrap_or(&"").to_string();
            let amount = form_parts
                .get(2)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);

            EffectData::CashTransfer {
                from: AccountTag(from_account),
                to: AccountTag(to_account),
                amount: AmountData::Fixed(amount),
            }
        }
    };

    if let Some(event) = state.data_mut().events.get_mut(event_idx) {
        event.effects.push(effect);
        ActionResult::modified()
    } else {
        ActionResult::close()
    }
}

/// Handle effect deletion
pub fn handle_delete_effect(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    // Get typed effect context
    let (event_idx, effect_idx) = match ctx.typed_context().and_then(|c| c.as_effect()) {
        Some(EffectContext::Existing { event, effect }) => (*event, *effect),
        _ => return ActionResult::close(),
    };

    if let Some(event) = state.data_mut().events.get_mut(event_idx)
        && effect_idx < event.effects.len()
    {
        event.effects.remove(effect_idx);
        ActionResult::modified()
    } else {
        ActionResult::close()
    }
}

/// Handle editing an effect
pub fn handle_edit_effect(state: &mut AppState, ctx: ActionContext) -> ActionResult {
    // Get typed effect context
    let (event_idx, effect_idx, effect_type) = match ctx.typed_context().and_then(|c| c.as_effect())
    {
        Some(EffectContext::Edit {
            event,
            effect,
            effect_type,
        }) => (*event, *effect, effect_type.clone()),
        _ => return ActionResult::close(),
    };

    let form_parts = ctx.value_parts();

    let new_effect = match effect_type {
        EffectTypeContext::Income => {
            let to_account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);
            let gross = form_parts.get(2).map(|s| parse_yes_no(s)).unwrap_or(false);
            let taxable = form_parts.get(3).map(|s| parse_yes_no(s)).unwrap_or(true);

            Some(EffectData::Income {
                to: AccountTag(to_account),
                amount: AmountData::Fixed(amount),
                gross,
                taxable,
            })
        }
        EffectTypeContext::Expense => {
            let from_account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);

            Some(EffectData::Expense {
                from: AccountTag(from_account),
                amount: AmountData::Fixed(amount),
            })
        }
        EffectTypeContext::AssetPurchase => {
            let from_account = form_parts.first().unwrap_or(&"").to_string();
            let to_account = form_parts.get(1).unwrap_or(&"").to_string();
            let asset = form_parts.get(2).unwrap_or(&"").to_string();
            let amount = form_parts
                .get(3)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);

            Some(EffectData::AssetPurchase {
                from: AccountTag(from_account),
                to_account: AccountTag(to_account),
                asset: AssetTag(asset),
                amount: AmountData::Fixed(amount),
            })
        }
        EffectTypeContext::AssetSale => {
            let from_account = form_parts.first().unwrap_or(&"").to_string();
            let asset_str = form_parts.get(1).unwrap_or(&"").trim();
            let asset = if asset_str.is_empty() {
                None
            } else {
                Some(AssetTag(asset_str.to_string()))
            };
            let amount = form_parts
                .get(2)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);
            let gross = form_parts.get(3).map(|s| parse_yes_no(s)).unwrap_or(false);
            let lot_method = form_parts
                .get(4)
                .map(|s| parse_lot_method(s))
                .unwrap_or_default();

            Some(EffectData::AssetSale {
                from: AccountTag(from_account),
                asset,
                amount: AmountData::Fixed(amount),
                gross,
                lot_method,
            })
        }
        EffectTypeContext::Sweep => {
            let to_account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);
            let strategy = form_parts
                .get(2)
                .map(|s| parse_strategy(s))
                .unwrap_or_default();
            let gross = form_parts.get(3).map(|s| parse_yes_no(s)).unwrap_or(false);
            let taxable = form_parts.get(4).map(|s| parse_yes_no(s)).unwrap_or(true);
            let lot_method = form_parts
                .get(5)
                .map(|s| parse_lot_method(s))
                .unwrap_or_default();

            Some(EffectData::Sweep {
                to: AccountTag(to_account),
                amount: AmountData::Fixed(amount),
                strategy,
                gross,
                taxable,
                lot_method,
                exclude_accounts: vec![],
            })
        }
        EffectTypeContext::TriggerEvent => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            Some(EffectData::TriggerEvent {
                event: EventTag(event_name),
            })
        }
        EffectTypeContext::PauseEvent => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            Some(EffectData::PauseEvent {
                event: EventTag(event_name),
            })
        }
        EffectTypeContext::ResumeEvent => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            Some(EffectData::ResumeEvent {
                event: EventTag(event_name),
            })
        }
        EffectTypeContext::TerminateEvent => {
            let event_name = form_parts.first().unwrap_or(&"").to_string();
            Some(EffectData::TerminateEvent {
                event: EventTag(event_name),
            })
        }
        EffectTypeContext::ApplyRmd => {
            let destination = form_parts.first().unwrap_or(&"").to_string();
            let lot_method = form_parts
                .get(1)
                .map(|s| parse_lot_method(s))
                .unwrap_or_default();

            Some(EffectData::ApplyRmd {
                destination: AccountTag(destination),
                lot_method,
            })
        }
        EffectTypeContext::AdjustBalance => {
            let account = form_parts.first().unwrap_or(&"").to_string();
            let amount = form_parts
                .get(1)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);

            Some(EffectData::AdjustBalance {
                account: AccountTag(account),
                amount: AmountData::Fixed(amount),
            })
        }
        EffectTypeContext::CashTransfer => {
            let from_account = form_parts.first().unwrap_or(&"").to_string();
            let to_account = form_parts.get(1).unwrap_or(&"").to_string();
            let amount = form_parts
                .get(2)
                .and_then(|s| parse_currency(s).ok())
                .unwrap_or(0.0);

            Some(EffectData::CashTransfer {
                from: AccountTag(from_account),
                to: AccountTag(to_account),
                amount: AmountData::Fixed(amount),
            })
        }
    };

    if let Some(effect) = new_effect
        && let Some(event) = state.data_mut().events.get_mut(event_idx)
        && effect_idx < event.effects.len()
    {
        event.effects[effect_idx] = effect;
        ActionResult::modified()
    } else {
        ActionResult::close()
    }
}
