//! Parity: every in-memory edit leaves the graph exactly as the SQL route
//! leaves the database.
//!
//! Each test applies the same edit twice — through the route's SQL half, then
//! reloading, and through `domain::edit` on a graph loaded beforehand — and
//! compares the two graphs whole. With one scenario in the database, SQLite
//! hands out `max(id) + 1` for new rows exactly as the in-memory merge does, so
//! even the row ids agree and the comparison needs no normalizing.

use axum::response::IntoResponse;
use serde_json::{Value, json};

use super::*;
use crate::api::accounts::{self, CreateAccount, CreatePosition, UpdateAccount, UpdatePosition};
use crate::api::assets::{self, CreateAsset, UpdateAsset};
use crate::api::events::{self, EventBody, read_event};
use crate::api::expressions::validate_tree;
use crate::api::row_batch::RowBatch;
use crate::compile::{self, rows::ScenarioGraph};
use crate::db::Db;
use crate::error::ApiError;

struct Plan {
    db: Db,
    user: String,
    id: i64,
    ids: Ids,
}

/// Row ids of the fixture's accounts, assets, parameters and events.
#[derive(Default)]
struct Ids {
    usaa: i64,
    vanguard: i64,
    roth: i64,
    mortgage: i64,
    home: i64,
    vfiax: i64,
    bnd: i64,
    house: i64,
    equity: i64,
    bonds: i64,
    cash: i64,
    retire_date: i64,
    retire_age: i64,
    retirement: i64,
    salary: i64,
    expenses: i64,
    health: i64,
    home_purchase: i64,
    sweep: i64,
    paydown: i64,
}

async fn scalar(db: &Db, sql: &str, binds: &[Value]) -> i64 {
    let mut q = sqlx::query_scalar::<_, i64>(sql);
    for b in binds {
        q = match b {
            Value::Null => q.bind(None::<i64>),
            Value::String(s) => q.bind(s.clone()),
            Value::Number(n) if n.is_i64() => q.bind(n.as_i64()),
            Value::Number(n) => q.bind(n.as_f64()),
            other => panic!("unsupported bind {other}"),
        };
    }
    q.fetch_one(db).await.unwrap()
}

fn body(value: Value) -> EventBody {
    serde_json::from_value(value).expect("event body")
}

fn inflated(value: f64) -> Value {
    json!({"kind": "InflationAdjusted", "inner": {"kind": "Fixed", "value": value}})
}

impl Plan {
    /// A small plan shaped like the default fixture: a bank account, two
    /// investment accounts, a mortgage and a house, three parameters, and the
    /// fixture's seven events written through the SQL route.
    async fn new() -> Self {
        let db = crate::db::connect("sqlite::memory:", 1).await.unwrap();
        let user = uuid::Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO users(id,email,password_hash) VALUES (?,?,'unused')")
            .bind(&user)
            .bind(format!("{user}@example.test"))
            .execute(&db)
            .await
            .unwrap();
        let u = json!(user);
        let mut ids = Ids::default();

        let profile = |name: &'static str, dist: i64| {
            (
                "INSERT INTO return_profiles(user_id,name,distribution_id) VALUES (?,?,?) RETURNING id",
                vec![u.clone(), json!(name), json!(dist)],
            )
        };
        let eq = scalar(
            &db,
            "INSERT INTO distributions(user_id,kind,mean,std_dev) VALUES (?,'Normal',0.07,0.15) RETURNING id",
            std::slice::from_ref(&u),
        )
        .await;
        let bd = scalar(
            &db,
            "INSERT INTO distributions(user_id,kind,mean,std_dev) VALUES (?,'Normal',0.03,0.05) RETURNING id",
            std::slice::from_ref(&u),
        )
        .await;
        let cd = scalar(
            &db,
            "INSERT INTO distributions(user_id,kind,rate) VALUES (?,'Fixed',0.03) RETURNING id",
            std::slice::from_ref(&u),
        )
        .await;
        for (slot, (sql, binds)) in [
            (&mut ids.equity, profile("Equity", eq)),
            (&mut ids.bonds, profile("Bonds", bd)),
            (&mut ids.cash, profile("Cash", cd)),
        ] {
            *slot = scalar(&db, sql, &binds).await;
        }

        let id = scalar(
            &db,
            "INSERT INTO scenarios(user_id,name,start_date,birth_date,duration_years)
             VALUES (?,'Default','2026-01-01','1991-06-15',40) RETURNING id",
            std::slice::from_ref(&u),
        )
        .await;
        let s = json!(id);

        let asset =
            "INSERT INTO assets(scenario_id,name,initial_price,return_profile_id,sort_order)
                     VALUES (?,?,?,?,?) RETURNING id";
        ids.vfiax = scalar(
            &db,
            asset,
            &[
                s.clone(),
                json!("VFIAX"),
                json!(500.0),
                json!(ids.equity),
                json!(0),
            ],
        )
        .await;
        ids.bnd = scalar(
            &db,
            asset,
            &[
                s.clone(),
                json!("BND"),
                json!(70.0),
                json!(ids.bonds),
                json!(1),
            ],
        )
        .await;
        ids.house = scalar(
            &db,
            asset,
            &[
                s.clone(),
                json!("House"),
                json!(100.0),
                Value::Null,
                json!(2),
            ],
        )
        .await;

        let account = "INSERT INTO accounts(scenario_id,name,flavor,sort_order) VALUES (?,?,?,?) RETURNING id";
        ids.usaa = scalar(
            &db,
            account,
            &[s.clone(), json!("USAA"), json!("Bank"), json!(0)],
        )
        .await;
        ids.vanguard = scalar(
            &db,
            account,
            &[s.clone(), json!("Vanguard"), json!("Investment"), json!(1)],
        )
        .await;
        ids.roth = scalar(
            &db,
            account,
            &[s.clone(), json!("Roth"), json!("Investment"), json!(2)],
        )
        .await;
        ids.mortgage = scalar(
            &db,
            account,
            &[s.clone(), json!("Mortgage"), json!("Liability"), json!(3)],
        )
        .await;
        ids.home = scalar(
            &db,
            account,
            &[s.clone(), json!("Home"), json!("Property"), json!(4)],
        )
        .await;

        for (sql, binds) in [
            (
                "INSERT INTO account_bank(account_id,cash_value,return_profile_id) VALUES (?,50000,?) RETURNING account_id",
                vec![json!(ids.usaa), json!(ids.cash)],
            ),
            (
                "INSERT INTO account_investment(account_id,tax_status,cash_value,cash_return_profile_id)
                 VALUES (?,'Taxable',1000,?) RETURNING account_id",
                vec![json!(ids.vanguard), json!(ids.cash)],
            ),
            (
                "INSERT INTO account_investment(account_id,tax_status,cash_value,cash_return_profile_id,
                                                contribution_limit,contribution_period)
                 VALUES (?,'TaxFree',0,?,7000,'Yearly') RETURNING account_id",
                vec![json!(ids.roth), json!(ids.cash)],
            ),
            (
                "INSERT INTO account_liability(account_id,principal,interest_rate) VALUES (?,0,0.06) RETURNING account_id",
                vec![json!(ids.mortgage)],
            ),
            (
                "INSERT INTO account_property(account_id,asset_id,value) VALUES (?,?,0) RETURNING account_id",
                vec![json!(ids.home), json!(ids.house)],
            ),
            (
                "INSERT INTO positions(account_id,asset_id,purchase_date,units,cost_basis)
                 VALUES (?,?,'2026-01-01',100,50000) RETURNING id",
                vec![json!(ids.vanguard), json!(ids.vfiax)],
            ),
            (
                "INSERT INTO positions(account_id,asset_id,purchase_date,units,cost_basis)
                 VALUES (?,?,'2026-01-01',50,3500) RETURNING id",
                vec![json!(ids.roth), json!(ids.bnd)],
            ),
        ] {
            scalar(&db, sql, &binds).await;
        }

        let param = "INSERT INTO named_parameters(scenario_id,name,kind,number_value,date_value,age_years,age_months)
                     VALUES (?,?,?,?,?,?,?) RETURNING id";
        scalar(
            &db,
            param,
            &[
                s.clone(),
                json!("Spending"),
                json!("Money"),
                json!(60000.0),
                Value::Null,
                Value::Null,
                Value::Null,
            ],
        )
        .await;
        ids.retire_date = scalar(
            &db,
            param,
            &[
                s.clone(),
                json!("RetireDate"),
                json!("Date"),
                Value::Null,
                json!("2050-01-01"),
                Value::Null,
                Value::Null,
            ],
        )
        .await;
        ids.retire_age = scalar(
            &db,
            param,
            &[
                s.clone(),
                json!("RetireAge"),
                json!("Age"),
                Value::Null,
                Value::Null,
                json!(60),
                json!(0),
            ],
        )
        .await;

        let mut plan = Plan { db, user, id, ids };
        plan.ids.retirement = plan.create(&plan.retirement()).await.unwrap();
        plan.ids.salary = plan.create(&plan.salary()).await.unwrap();
        plan.ids.expenses = plan.create(&plan.expenses()).await.unwrap();
        plan.ids.health = plan.create(&plan.health_care()).await.unwrap();
        plan.ids.home_purchase = plan.create(&plan.home_purchase(true)).await.unwrap();
        plan.ids.sweep = plan.create(&plan.sweep(20000.0)).await.unwrap();
        plan.ids.paydown = plan.create(&plan.paydown()).await.unwrap();
        plan
    }

    async fn load(&self) -> ScenarioGraph {
        ScenarioGraph::load(&self.db, self.id, &self.user)
            .await
            .unwrap()
    }

    // ── the SQL halves of the routes ────────────────────────────────────────

    async fn create(&self, body: &EventBody) -> ApiResult<i64> {
        validate_tree(&self.load().await, &body.effects)?;
        let mut tx = self.db.begin().await?;
        let id = events::create_in(&mut tx, self.id, body).await?;
        tx.commit().await?;
        Ok(id)
    }

    async fn replace(&self, id: i64, body: &EventBody) -> ApiResult<()> {
        validate_tree(&self.load().await, &body.effects)?;
        let mut tx = self.db.begin().await?;
        events::replace_in(&mut tx, self.id, id, body).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn destroy(&self, id: i64) -> ApiResult<()> {
        let mut tx = self.db.begin().await?;
        events::destroy_in(&mut tx, self.id, id).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn update_asset(&self, id: i64, body: &UpdateAsset) -> ApiResult<()> {
        let graph = self.load().await;
        let mut tx = self.db.begin().await?;
        assets::update_in(&mut tx, Some(&graph), self.id, id, body).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn update_account(&self, id: i64, body: &UpdateAccount) -> ApiResult<()> {
        let graph = self.load().await;
        let mut tx = self.db.begin().await?;
        accounts::update_in(&mut tx, Some(&graph), self.id, id, body).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn add_position(&self, account: i64, body: &CreatePosition) -> ApiResult<i64> {
        let mut tx = self.db.begin().await?;
        let position = accounts::add_position_in(&mut tx, self.id, account, body).await?;
        tx.commit().await?;
        Ok(position.id)
    }

    async fn update_position(
        &self,
        account: i64,
        position: i64,
        body: &UpdatePosition,
    ) -> ApiResult<()> {
        let mut tx = self.db.begin().await?;
        accounts::update_position_in(&mut tx, self.id, account, position, body).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_position(&self, account: i64, position: i64) -> ApiResult<()> {
        let mut tx = self.db.begin().await?;
        accounts::delete_position_in(&mut tx, self.id, account, position).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── event bodies ────────────────────────────────────────────────────────

    fn retirement(&self) -> EventBody {
        body(json!({
            "name": "Retirement", "fires_once": true,
            "trigger": {"kind": "Age", "years": 40},
        }))
    }

    fn salary(&self) -> EventBody {
        body(json!({
            "name": "Salary",
            "trigger": {"kind": "Repeating", "interval": "BiWeekly",
                "end_condition": {"kind": "RelativeToEvent", "event_id": self.ids.retirement,
                                  "unit": "Years", "value": 0}},
            "effects": [{"kind": "Income", "to_account_id": self.ids.usaa, "amount": inflated(8000.0),
                         "amount_mode": "Net", "income_type": "Taxable"}],
        }))
    }

    fn expenses(&self) -> EventBody {
        body(json!({
            "name": "Expenses",
            "trigger": {"kind": "Repeating", "interval": "Monthly"},
            "effects": [{"kind": "Expense", "from_account_id": self.ids.usaa, "amount": inflated(8000.0)}],
        }))
    }

    fn health_care(&self) -> EventBody {
        body(json!({
            "name": "Health Care",
            "trigger": {"kind": "Repeating", "interval": "Monthly",
                "start_condition": {"kind": "RelativeToEvent", "event_id": self.ids.retirement,
                                    "unit": "Days", "value": 0}},
            "effects": [{"kind": "Expense", "from_account_id": self.ids.usaa, "amount": inflated(1000.0)}],
        }))
    }

    fn home_purchase(&self, with_sweep: bool) -> EventBody {
        let mut effects = vec![];
        if with_sweep {
            effects.push(json!({"kind": "Sweep", "to_account_id": self.ids.usaa, "amount": inflated(200000.0),
                "sources": {"mode": "Strategy", "strategy": "PenaltyAware", "exclude_accounts": []},
                "amount_mode": "Net", "lot_method": "Fifo", "income_type": "Taxable"}));
        }
        effects.extend([
            json!({"kind": "Expense", "from_account_id": self.ids.usaa, "amount": inflated(200000.0)}),
            json!({"kind": "AdjustBalance", "account_id": self.ids.mortgage, "amount": inflated(1_000_000.0)}),
            json!({"kind": "AdjustBalance", "account_id": self.ids.home, "amount": inflated(1_200_000.0)}),
        ]);
        body(json!({
            "name": "Home Purchase", "fires_once": true,
            "trigger": {"kind": "Age", "years": 35},
            "effects": effects,
        }))
    }

    fn sweep(&self, amount: f64) -> EventBody {
        body(json!({
            "name": "Sweep",
            "trigger": {"kind": "AccountBalance", "account_id": self.ids.usaa,
                        "comparison": "LessThanOrEqual", "threshold": 5000.0},
            "effects": [{"kind": "Sweep", "to_account_id": self.ids.usaa, "amount": inflated(amount),
                "sources": {"mode": "Strategy", "strategy": "PenaltyAware", "exclude_accounts": []},
                "amount_mode": "Net", "lot_method": "Fifo", "income_type": "Taxable"}],
        }))
    }

    fn paydown(&self) -> EventBody {
        body(json!({
            "name": "Mortgage Paydown",
            "trigger": {"kind": "Repeating", "interval": "Monthly",
                "start_condition": {"kind": "RelativeToEvent", "event_id": self.ids.home_purchase,
                                    "unit": "Months", "value": 1},
                "end_condition": {"kind": "AccountBalance", "account_id": self.ids.mortgage,
                                  "comparison": "GreaterThanOrEqual", "threshold": 0.0}},
            "effects": [{"kind": "CashTransfer", "from_account_id": self.ids.usaa,
                         "to_account_id": self.ids.mortgage, "amount": inflated(6000.0)}],
        }))
    }

    /// Nested And/Or conditions, a parameter trigger, nested `Random`
    /// branches, a custom withdrawal list and an amount expression.
    fn branching(&self) -> EventBody {
        body(json!({
            "name": "Windfall",
            "trigger": {"kind": "Or", "children": [
                {"kind": "And", "children": [
                    {"kind": "AccountBalance", "account_id": self.ids.usaa,
                     "comparison": "GreaterThanOrEqual", "threshold": 100000.0},
                    {"kind": "NetWorth", "comparison": "LessThanOrEqual", "threshold": 5e6},
                ]},
                {"kind": "AgeParameter", "parameter_id": self.ids.retire_age},
            ]},
            "effects": [
                {"kind": "Random", "probability": 0.5,
                 "on_true": {"kind": "Sweep", "to_account_id": self.ids.usaa,
                     "amount": {"kind": "Min", "left": {"kind": "Fixed", "value": 1000.0},
                                "right": {"kind": "AccountCashBalance", "account_id": self.ids.vanguard}},
                     "sources": {"mode": "Custom", "entries": [
                         {"account_id": self.ids.vanguard, "asset_id": self.ids.vfiax},
                         {"account_id": self.ids.roth, "asset_id": self.ids.bnd}]},
                     "amount_mode": "Gross", "lot_method": "HighestCost", "income_type": "TaxFree"},
                 "on_false": {"kind": "Random", "probability": 0.25,
                     "on_true": {"kind": "CashTransfer", "from_account_id": self.ids.usaa,
                                 "to_account_id": self.ids.vanguard,
                                 "amount": {"kind": "Fixed", "value": 10.0}}}},
                {"kind": "Expense", "from_account_id": self.ids.usaa,
                 "amount": {"kind": "Expression",
                            "source": "$Spending / 12 + cash(\"USAA\") / 1000 + holding(\"Vanguard\", \"VFIAX\") * 0"}},
            ],
        }))
    }

    /// Repeating with both sub-conditions (one an Or), a bracket-filling
    /// sweep with exclusions, and most of the remaining effect kinds.
    fn drawdown(&self) -> EventBody {
        body(json!({
            "name": "Drawdown",
            "trigger": {"kind": "Repeating", "interval": "Yearly", "max_occurrences": 10,
                "start_condition": {"kind": "DateParameter", "parameter_id": self.ids.retire_date},
                "end_condition": {"kind": "Or", "children": [
                    {"kind": "Date", "on_date": "2060-01-01"},
                    {"kind": "RelativeToEvent", "event_id": self.ids.retirement,
                     "unit": "Months", "value": 6},
                ]}},
            "effects": [
                {"kind": "Sweep", "to_account_id": self.ids.usaa,
                 "amount": {"kind": "Max",
                     "left": {"kind": "Sub", "left": {"kind": "Fixed", "value": 50000.0},
                              "right": {"kind": "AccountTotalBalance", "account_id": self.ids.usaa}},
                     "right": {"kind": "Fixed", "value": 0.0}},
                 "sources": {"mode": "Strategy", "strategy": "BracketFilling",
                             "exclude_accounts": [self.ids.roth], "bracket_ceiling": 0.22},
                 "amount_mode": "Net", "lot_method": "Fifo", "income_type": "Taxable"},
                {"kind": "AssetPurchase", "from_account_id": self.ids.vanguard,
                 "to_account_id": self.ids.vanguard, "asset_id": self.ids.vfiax,
                 "amount": {"kind": "Scale", "factor": 0.5, "inner": {"kind": "Fixed", "value": 1000.0}}},
                {"kind": "AssetSale", "from_account_id": self.ids.vanguard, "asset_id": self.ids.vfiax,
                 "amount": {"kind": "Fixed", "value": 100.0}, "amount_mode": "Gross", "lot_method": "Lifo"},
                {"kind": "AdjustBalance", "account_id": self.ids.usaa,
                 "amount": {"kind": "Mul", "left": {"kind": "Fixed", "value": 0.0},
                            "right": {"kind": "AssetBalance", "account_id": self.ids.vanguard,
                                      "asset_id": self.ids.vfiax}}},
                {"kind": "TriggerEvent", "target_event_id": self.ids.expenses},
                {"kind": "PauseEvent", "target_event_id": self.ids.sweep},
                {"kind": "ResumeEvent", "target_event_id": self.ids.sweep},
                {"kind": "ApplyRmd", "to_account_id": self.ids.usaa},
                {"kind": "RsuVesting", "to_account_id": self.ids.vanguard, "asset_id": self.ids.vfiax,
                 "units": 10.0, "sell_to_cover": true},
                {"kind": "MarketShock", "drop": 0.3},
            ],
        }))
    }

    /// A financed purchase and a sale, on a fixed date.
    fn move_house(&self) -> EventBody {
        body(json!({
            "name": "Move", "fires_once": true, "sort_order": 3,
            "trigger": {"kind": "Date", "on_date": "2040-05-01"},
            "effects": [
                {"kind": "SellProperty", "property_account_id": self.ids.home,
                 "to_account_id": self.ids.usaa, "selling_cost_rate": 0.06,
                 "gain_exclusion": 250000.0, "payoff_account_id": self.ids.mortgage},
                {"kind": "BuyProperty", "property_account_id": self.ids.home,
                 "from_account_id": self.ids.usaa, "price": {"kind": "Fixed", "value": 500000.0},
                 "financing": {"loan_account_id": self.ids.mortgage,
                               "down_payment": {"kind": "Fixed", "value": 100000.0},
                               "term_months": 360}},
                {"kind": "TerminateEvent", "target_event_id": self.ids.paydown},
            ],
        }))
    }
}

// ── comparison ──────────────────────────────────────────────────────────────

/// Assert the stored plan and `mem` are the same graph, and compile the same.
async fn assert_same(plan: &Plan, mem: &ScenarioGraph, what: &str) {
    let stored = plan.load().await;
    let a = serde_json::to_value(&stored).unwrap();
    let b = serde_json::to_value(mem).unwrap();
    if a != b {
        let differing: Vec<&String> = a
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, v)| b.get(k.as_str()) != Some(v))
            .map(|(k, _)| k)
            .collect();
        panic!(
            "{what}: graphs differ in {differing:?}\nsql: {}\nmem: {}",
            serde_json::to_string(&a[differing[0].as_str()]).unwrap(),
            serde_json::to_string(&b[differing[0].as_str()]).unwrap(),
        );
    }
    // Same graph, so this is really checking the edit produced a valid plan.
    let config = |g: &ScenarioGraph| {
        canonical(
            &compile::compile(g)
                .unwrap_or_else(|e| panic!("{what}: compile: {e:?}"))
                .config,
        )
    };
    assert_eq!(config(&stored), config(mem), "{what}: configs differ");
    for event in &stored.events {
        if stored.event_trigger.contains_key(&event.id) {
            assert_eq!(
                serde_json::to_value(read_event(&stored, event.id).unwrap()).unwrap(),
                serde_json::to_value(read_event(mem, event.id).unwrap()).unwrap(),
                "{what}: event {} reads back differently",
                event.name
            );
        }
    }
}

/// A compiled config as text that does not depend on hash-map order. (It does
/// not serialize to JSON: a `ReturnProfile::Fixed` is a tagged newtype.)
fn canonical(config: &finplan_core::config::SimulationConfig) -> Vec<String> {
    fn sorted<K: std::fmt::Debug, V: std::fmt::Debug>(
        map: &std::collections::HashMap<K, V>,
    ) -> String {
        let mut entries: Vec<String> = map.iter().map(|kv| format!("{kv:?}")).collect();
        entries.sort();
        entries.join(", ")
    }
    vec![
        sorted(&config.return_profiles),
        format!("{:?}", config.inflation_profile),
        sorted(&config.asset_returns),
        sorted(&config.asset_prices),
        format!("{:?}", config.tax_config),
        format!("{:?} {:?}", config.start_date, config.birth_date),
        format!("{:?}", config.accounts),
        format!("{:?}", config.duration_years),
        format!("{:?}", config.events),
        sorted(&config.asset_tracking_errors),
        sorted(&config.parameters),
        format!("{:?}", config.collect_ledger),
    ]
}

fn status(err: ApiError) -> axum::http::StatusCode {
    err.into_response().status()
}

// ── events ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn created_events_match_the_route() {
    let plan = Plan::new().await;
    let mut mem = plan.load().await;
    for body in [plan.branching(), plan.drawdown(), plan.move_house()] {
        let sql_id = plan.create(&body).await.unwrap();
        let mem_id = create_event(&mut mem, &body).unwrap();
        assert_eq!(sql_id, mem_id, "{}", body.name);
        assert_same(&plan, &mem, &body.name).await;
    }
}

#[tokio::test]
async fn replaced_events_match_the_route() {
    let plan = Plan::new().await;
    let mut mem = plan.load().await;
    let ids = &plan.ids;
    // The review note from the fixture run, the sweep's cash target, then
    // whole trees swapped for ones of a different shape and back again.
    let edits = [
        (ids.home_purchase, plan.home_purchase(false)),
        (ids.sweep, plan.sweep(172000.0)),
        (
            ids.expenses,
            EventBody {
                name: "Expenses".into(),
                ..plan.branching()
            },
        ),
        (
            ids.paydown,
            EventBody {
                name: "Mortgage Paydown".into(),
                ..plan.drawdown()
            },
        ),
        (ids.paydown, plan.paydown()),
        (ids.expenses, plan.expenses()),
        (
            ids.salary,
            EventBody {
                name: "  Pay  ".into(),
                enabled: false,
                sort_order: Some(9),
                ..plan.salary()
            },
        ),
    ];
    for (id, body) in edits {
        plan.replace(id, &body).await.unwrap();
        replace_event(&mut mem, id, &body).unwrap();
        assert_same(&plan, &mem, &format!("replace {}", body.name)).await;
    }
}

#[tokio::test]
async fn deleted_events_match_the_route() {
    let plan = Plan::new().await;
    let mut mem = plan.load().await;
    let ids = &plan.ids;

    // Paydown's root trigger names Home Purchase only inside its start
    // condition, so it is no referrer; Sweep is unreferenced.
    for id in [ids.sweep, ids.paydown] {
        plan.destroy(id).await.unwrap();
        delete_event(&mut mem, id).unwrap();
        assert_same(&plan, &mem, &format!("delete {id}")).await;
    }

    // A root-level reference is refused by both.
    let windfall = plan
        .create(&body(json!({
            "name": "After retiring",
            "trigger": {"kind": "RelativeToEvent", "event_id": ids.retirement,
                        "unit": "Days", "value": 1},
        })))
        .await
        .unwrap();
    let mut mem = plan.load().await;
    let sql = plan.destroy(ids.retirement).await.unwrap_err();
    let err = delete_event(&mut mem, ids.retirement).unwrap_err();
    assert_eq!(err.to_string(), sql.to_string());
    assert_same(&plan, &mem, "refused delete").await;

    // Missing ids are not found by both.
    assert_eq!(
        status(plan.destroy(9999).await.unwrap_err()),
        status(delete_event(&mut mem, 9999).unwrap_err())
    );

    plan.destroy(windfall).await.unwrap();
    delete_event(&mut mem, windfall).unwrap();
    assert_same(&plan, &mem, "delete referrer").await;
}

/// The referrer check only looks at root triggers, so deleting an event named
/// inside another event's `Repeating` sub-condition cascades that condition
/// away — and, through `end_trigger_id`'s cascade, the root that linked it.
/// That is a hole in the route, not in the mirror; the mirror reproduces it.
#[tokio::test]
async fn deleting_an_event_named_in_a_sub_condition_cascades_the_same_way() {
    let plan = Plan::new().await;
    let mut mem = plan.load().await;
    let ids = &plan.ids;
    plan.destroy(ids.retirement).await.unwrap();
    delete_event(&mut mem, ids.retirement).unwrap();

    let stored = plan.load().await;
    assert_eq!(
        serde_json::to_value(&stored).unwrap(),
        serde_json::to_value(&mem).unwrap()
    );
    assert!(!stored.event_trigger.contains_key(&ids.salary));
    assert!(!stored.event_trigger.contains_key(&ids.health));
}

#[tokio::test]
async fn failed_edits_fail_alike_and_leave_the_graph_alone() {
    let plan = Plan::new().await;
    let mut mem = plan.load().await;
    let before = serde_json::to_value(&mem).unwrap();
    let ids = &plan.ids;

    let bad = [
        // Lowering: an empty And, found after the rows before it were queued.
        body(
            json!({"name": "Empty", "trigger": {"kind": "And", "children": []},
                    "effects": [{"kind": "Expense", "from_account_id": ids.usaa,
                                 "amount": {"kind": "Fixed", "value": 1.0}}]}),
        ),
        // Lowering: the wrong parameter kind.
        body(json!({"name": "Wrong kind",
                    "trigger": {"kind": "DateParameter", "parameter_id": ids.retire_age}})),
        // Lowering: a nested Expression.
        body(json!({"name": "Nested", "trigger": {"kind": "Manual"},
                    "effects": [{"kind": "Expense", "from_account_id": ids.usaa,
                                 "amount": {"kind": "InflationAdjusted",
                                            "inner": {"kind": "Expression", "source": "1"}}}]})),
        // References: an account that does not exist.
        body(json!({"name": "Dangling", "trigger": {"kind": "Manual"},
                    "effects": [{"kind": "Expense", "from_account_id": 9999,
                                 "amount": {"kind": "Fixed", "value": 1.0}}]})),
        // Uniqueness: the name is taken.
        EventBody {
            name: " Salary ".into(),
            ..plan.expenses()
        },
    ];
    for body in bad {
        let sql = plan.create(&body).await.unwrap_err();
        let err = create_event(&mut mem, &body).unwrap_err();
        assert_eq!(status(sql), status(err), "{}", body.name);
        assert_eq!(serde_json::to_value(&mem).unwrap(), before, "{}", body.name);
    }
    assert_same(&plan, &mem, "after failures").await;

    let taken = EventBody {
        name: "Expenses".into(),
        ..plan.sweep(1.0)
    };
    let sql = plan.replace(ids.sweep, &taken).await.unwrap_err();
    let err = replace_event(&mut mem, ids.sweep, &taken).unwrap_err();
    assert_eq!(status(sql), status(err));
    let sql = plan.replace(9999, &plan.sweep(1.0)).await.unwrap_err();
    let err = replace_event(&mut mem, 9999, &plan.sweep(1.0)).unwrap_err();
    assert_eq!(status(sql), status(err));
    assert_eq!(serde_json::to_value(&mem).unwrap(), before);
}

// ── assets and accounts ─────────────────────────────────────────────────────

fn asset_update(value: Value) -> UpdateAsset {
    serde_json::from_value(value).expect("asset update")
}

fn account_update(value: Value) -> UpdateAccount {
    serde_json::from_value(value).expect("account update")
}

#[tokio::test]
async fn asset_updates_match_the_route() {
    let plan = Plan::new().await;
    plan.create(&plan.branching()).await.unwrap();
    let mut mem = plan.load().await;
    let ids = &plan.ids;

    let edits = [
        // A rename rewrites the expression that names the asset.
        (ids.vfiax, json!({"name": " VTI "})),
        (
            ids.vfiax,
            json!({"return_profile_id": ids.bonds, "tracking_error": 0.02, "sort_order": 7}),
        ),
        // An explicit null clears the tracking error just set.
        (ids.vfiax, json!({"tracking_error": null})),
        // An explicit null unmaps; an absent field is left alone.
        (
            ids.bnd,
            json!({"return_profile_id": null, "initial_price": 80.0}),
        ),
        (
            ids.house,
            json!({"return_profile_id": ids.equity, "description": "Primary home"}),
        ),
    ];
    for (id, edit) in edits {
        plan.update_asset(id, &asset_update(edit.clone()))
            .await
            .unwrap();
        update_asset(&mut mem, id, &asset_update(edit.clone())).unwrap();
        assert_same(&plan, &mem, &edit.to_string()).await;
    }
    let source = mem
        .amounts
        .values()
        .find_map(|a| a.expression_source.clone())
        .unwrap();
    assert!(source.contains("\"VTI\""), "{source}");

    let taken = asset_update(json!({"name": "BND"}));
    let sql = plan.update_asset(ids.vfiax, &taken).await.unwrap_err();
    let err = update_asset(&mut mem, ids.vfiax, &taken).unwrap_err();
    assert_eq!(status(sql), status(err));
    let sql = plan
        .update_asset(9999, &asset_update(json!({"description": "x"})))
        .await
        .unwrap_err();
    let err = update_asset(&mut mem, 9999, &asset_update(json!({"description": "x"}))).unwrap_err();
    assert_eq!(status(sql), status(err));
    assert_same(&plan, &mem, "after refused asset edits").await;
}

#[tokio::test]
async fn account_updates_match_the_route() {
    let plan = Plan::new().await;
    plan.create(&plan.branching()).await.unwrap();
    let mut mem = plan.load().await;
    let ids = &plan.ids;

    let edits = [
        // A rename rewrites the expression that names the account.
        (ids.usaa, json!({"name": "Checking", "sort_order": 5})),
        (
            ids.usaa,
            json!({"flavor": "Bank", "cash_value": 90000.0, "return_profile_id": ids.bonds}),
        ),
        (
            ids.vanguard,
            json!({"flavor": "Investment", "tax_status": "TaxDeferred", "cash_value": 5.0,
                   "cash_return_profile_id": ids.cash, "contribution_limit": 24500.0,
                   "contribution_period": "Yearly", "description": "401(k)"}),
        ),
        (
            ids.mortgage,
            json!({"flavor": "Liability", "principal": 400000.0, "interest_rate": 0.055,
                   "repayment": {"from_account_id": ids.usaa, "term_months": 360}}),
        ),
        (
            ids.home,
            json!({"flavor": "Property", "asset_id": ids.house, "value": 650000.0}),
        ),
    ];
    for (id, edit) in edits {
        plan.update_account(id, &account_update(edit.clone()))
            .await
            .unwrap();
        update_account(&mut mem, id, &account_update(edit.clone())).unwrap();
        assert_same(&plan, &mem, &edit.to_string()).await;
    }
    let source = mem
        .amounts
        .values()
        .find_map(|a| a.expression_source.clone())
        .unwrap();
    assert!(source.contains("\"Checking\""), "{source}");

    let refused = [
        (ids.usaa, json!({"flavor": "Liability", "principal": 1.0})),
        (ids.vanguard, json!({"name": "   "})),
        (ids.vanguard, json!({"name": "Roth"})),
        (
            ids.mortgage,
            json!({"flavor": "Liability", "repayment": {"from_account_id": ids.home, "term_months": 12}}),
        ),
        (
            ids.roth,
            json!({"flavor": "Investment", "tax_status": "TaxFree", "cash_return_profile_id": ids.cash,
                   "contribution_limit": 7000.0}),
        ),
        (9999, json!({"description": "x"})),
    ];
    for (id, edit) in refused {
        let sql = plan
            .update_account(id, &account_update(edit.clone()))
            .await
            .unwrap_err();
        let err = update_account(&mut mem, id, &account_update(edit.clone())).unwrap_err();
        assert_eq!(status(sql), status(err), "{edit}");
    }
    assert_same(&plan, &mem, "after refused account edits").await;
}

fn new_position(value: Value) -> CreatePosition {
    serde_json::from_value(value).unwrap()
}

fn position_update(value: Value) -> UpdatePosition {
    serde_json::from_value(value).unwrap()
}

#[tokio::test]
async fn position_edits_match_the_route() {
    let plan = Plan::new().await;
    let mut mem = plan.load().await;
    let ids = &plan.ids;
    let lot = |g: &ScenarioGraph, account: i64| g.positions[&account][0].id;

    // An opening holding (dated at the plan start) and a dated purchase.
    for body in [
        json!({"asset_id": ids.bnd, "units": 10.0, "cost_basis": 900.0}),
        json!({"asset_id": ids.vfiax, "purchase_date": "2030-02-03", "units": 4.0,
               "cost_basis": 2000.0}),
    ] {
        let sql = plan
            .add_position(ids.vanguard, &new_position(body.clone()))
            .await
            .unwrap();
        let id = create_position(&mut mem, ids.vanguard, &new_position(body.clone())).unwrap();
        assert_eq!(sql, id, "{body}");
        assert_same(&plan, &mem, &body.to_string()).await;
    }

    let vanguard_lot = lot(&mem, ids.vanguard);
    for edit in [
        json!({"cost_basis": 20000.0}),
        json!({"units": 120.5, "purchase_date": "2025-12-31"}),
        json!({"asset_id": ids.bnd}),
    ] {
        plan.update_position(ids.vanguard, vanguard_lot, &position_update(edit.clone()))
            .await
            .unwrap();
        update_position(
            &mut mem,
            ids.vanguard,
            vanguard_lot,
            &position_update(edit.clone()),
        )
        .unwrap();
        assert_same(&plan, &mem, &edit.to_string()).await;
    }

    // Emptying an account drops its key, as `load` never makes one.
    let roth_lot = lot(&mem, ids.roth);
    plan.delete_position(ids.roth, roth_lot).await.unwrap();
    delete_position(&mut mem, ids.roth, roth_lot).unwrap();
    assert!(!mem.positions.contains_key(&ids.roth));
    assert_same(&plan, &mem, "delete the Roth's only lot").await;

    // Refusals fail alike and leave the graph alone.
    let refused_creates = [
        (
            ids.usaa,
            json!({"asset_id": ids.bnd, "units": 1.0, "cost_basis": 1.0}),
        ),
        (
            9999,
            json!({"asset_id": ids.bnd, "units": 1.0, "cost_basis": 1.0}),
        ),
        (
            ids.vanguard,
            json!({"asset_id": ids.bnd, "units": -1.0, "cost_basis": 1.0}),
        ),
        (
            ids.vanguard,
            json!({"asset_id": ids.bnd, "purchase_date": "someday", "units": 1.0,
                   "cost_basis": 1.0}),
        ),
    ];
    for (account, body) in refused_creates {
        let sql = plan
            .add_position(account, &new_position(body.clone()))
            .await
            .unwrap_err();
        let err = create_position(&mut mem, account, &new_position(body.clone())).unwrap_err();
        assert_eq!(status(sql), status(err), "{body}");
    }
    let refused_updates = [
        (ids.vanguard, vanguard_lot, json!({"cost_basis": -5.0})),
        (
            ids.vanguard,
            vanguard_lot,
            json!({"purchase_date": "2030-13-40"}),
        ),
        (ids.vanguard, 9999, json!({"units": 1.0})),
        (ids.roth, vanguard_lot, json!({"units": 1.0})),
    ];
    for (account, position, edit) in refused_updates {
        let sql = plan
            .update_position(account, position, &position_update(edit.clone()))
            .await
            .unwrap_err();
        let err = update_position(&mut mem, account, position, &position_update(edit.clone()))
            .unwrap_err();
        assert_eq!(status(sql), status(err), "{edit}");
    }
    for (account, position) in [(ids.vanguard, 9999), (ids.roth, vanguard_lot)] {
        let sql = plan.delete_position(account, position).await.unwrap_err();
        let err = delete_position(&mut mem, account, position).unwrap_err();
        assert_eq!(status(sql), status(err));
    }
    assert_same(&plan, &mem, "after refused position edits").await;
}

// ── the batch itself ────────────────────────────────────────────────────────

#[tokio::test]
async fn orphan_rows_do_not_change_what_compiles() {
    let plan = Plan::new().await;
    let mut graph = plan.load().await;
    let before = canonical(&compile::compile(&graph).unwrap().config);

    // Rows no event reaches: an amount tree, and a detached condition with a
    // child.
    let mut batch = RowBatch::for_graph(&graph);
    crate::api::specs::AmountSpec::Max {
        left: Box::new(crate::api::specs::AmountSpec::Fixed { value: 1.0 }),
        right: Box::new(crate::api::specs::AmountSpec::SourceBalance),
    }
    .lower(&mut batch, 0)
    .unwrap();
    serde_json::from_value::<crate::api::specs::TriggerSpec>(json!({"kind": "Or", "children": [
        {"kind": "Age", "years": 50}]}))
    .unwrap()
    .lower(&mut batch, crate::api::specs::TriggerParent::Detached, 0)
    .unwrap();
    batch.merge_into(&mut graph).unwrap();

    let after = canonical(&compile::compile(&graph).unwrap().config);
    assert_eq!(before, after);
}

#[tokio::test]
async fn the_batch_writes_rows_in_lowering_order_with_local_links_resolved() {
    let plan = Plan::new().await;
    let graph = plan.load().await;
    let mut batch = RowBatch::for_graph(&graph);
    let mut body = plan.drawdown();
    body.name = "Batch".into();
    let mut tx = plan.db.begin().await.unwrap();
    let event: i64 =
        sqlx::query_scalar("INSERT INTO events(scenario_id,name) VALUES (?1,'Batch') RETURNING id")
            .bind(plan.id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    events::lower_tree(&mut batch, event, &body).unwrap();
    let placed = batch.insert(&mut tx, plan.id).await.unwrap();
    tx.commit().await.unwrap();

    // Within each table, ids ascend in batch order: rows are written in the
    // order they were lowered. Withdrawal rows have no id of their own.
    use crate::api::row_batch::BatchRow;
    let mut last = [0_i64; 3];
    for (i, row) in batch.rows().iter().enumerate() {
        let id = placed.id(i as i64 + 1);
        let table = match row {
            BatchRow::Amount(_) => 0,
            BatchRow::Trigger(_) => 1,
            BatchRow::Effect(_) => 2,
            BatchRow::WithdrawalSource(_) | BatchRow::WithdrawalItem(_) => {
                assert_eq!(id, None);
                continue;
            }
        };
        let id = id.expect("placed row has an id");
        assert!(id > last[table], "row {i} placed out of order");
        last[table] = id;
    }
    let stored = plan.load().await;
    assert_eq!(
        serde_json::to_value(read_event(&stored, event).unwrap().trigger).unwrap(),
        serde_json::to_value(&body.trigger).unwrap()
    );
}

#[tokio::test]
async fn reindex_rebuilds_what_load_built() {
    let plan = Plan::new().await;
    plan.create(&plan.branching()).await.unwrap();
    plan.create(&plan.drawdown()).await.unwrap();
    let loaded = plan.load().await;
    let mut rebuilt = loaded.clone();
    rebuilt.trigger_children.clear();
    rebuilt.event_trigger.clear();
    rebuilt.event_effects.clear();
    rebuilt.effect_children.clear();
    rebuilt.reindex();
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(&rebuilt).unwrap()
    );
}

// ── creating assets and accounts ────────────────────────────────────────────

impl Plan {
    async fn create_asset(&self, body: &CreateAsset) -> ApiResult<i64> {
        let mut tx = self.db.begin().await?;
        let id = assets::create_in(&mut tx, self.id, body).await?;
        tx.commit().await?;
        Ok(id)
    }

    /// The account, then its lots, as a client creating one would.
    async fn create_account(
        &self,
        body: &CreateAccount,
        lots: &[CreatePosition],
    ) -> ApiResult<i64> {
        let mut tx = self.db.begin().await?;
        let id = accounts::create_in(&mut tx, self.id, body).await?;
        for lot in lots {
            accounts::add_position_in(&mut tx, self.id, id, lot).await?;
        }
        tx.commit().await?;
        Ok(id)
    }
}

fn new_asset(value: Value) -> CreateAsset {
    serde_json::from_value(value).expect("asset body")
}

fn new_account(value: Value) -> CreateAccount {
    serde_json::from_value(value).expect("account body")
}

#[tokio::test]
async fn created_assets_and_accounts_match_the_route() {
    let plan = Plan::new().await;
    let mut mem = plan.load().await;
    let ids = &plan.ids;

    for body in [
        json!({"name": "VTI", "description": "Total market", "initial_price": 250.0,
               "return_profile_id": ids.equity, "tracking_error": 0.02}),
        json!({"name": "Gold", "initial_price": 1900.0}),
        json!({"name": "Pinned", "initial_price": 5.0, "return_profile_id": ids.bonds,
               "sort_order": 0}),
    ] {
        let sql = plan.create_asset(&new_asset(body.clone())).await.unwrap();
        let id = create_asset(&mut mem, &new_asset(body.clone())).unwrap();
        assert_eq!(sql, id, "{body}");
        assert_same(&plan, &mem, &body.to_string()).await;
    }
    let vti = mem.assets.iter().find(|a| a.name == "VTI").unwrap().id;

    for (body, lots) in [
        (
            json!({"name": "Brokerage", "flavor": "Investment", "tax_status": "Taxable",
                   "cash_value": 5000.0, "cash_return_profile_id": ids.cash,
                   "contribution_limit": 7000.0, "contribution_period": "Yearly"}),
            vec![
                json!({"asset_id": vti, "units": 10.0, "cost_basis": 2000.0}),
                json!({"asset_id": ids.bnd, "purchase_date": "2027-03-01", "units": 4.0,
                       "cost_basis": 300.0}),
            ],
        ),
        (
            json!({"name": "Savings", "flavor": "Bank", "cash_value": 12000.0,
                   "return_profile_id": ids.cash, "sort_order": 1}),
            vec![],
        ),
        (
            json!({"name": "Cabin", "flavor": "Property", "asset_id": ids.house,
                   "value": 300000.0}),
            vec![],
        ),
        (
            json!({"name": "Cabin loan", "flavor": "Liability", "principal": 200000.0,
                   "interest_rate": 0.065,
                   "repayment": {"from_account_id": ids.usaa, "term_months": 360}}),
            vec![],
        ),
    ] {
        let lots: Vec<CreatePosition> = lots.into_iter().map(new_position).collect();
        let sql = plan
            .create_account(&new_account(body.clone()), &lots)
            .await
            .unwrap();
        let id = create_account(&mut mem, &new_account(body.clone())).unwrap();
        for lot in &lots {
            create_position(&mut mem, id, lot).unwrap();
        }
        assert_eq!(sql, id, "{body}");
        assert_same(&plan, &mem, &body.to_string()).await;
    }

    // Refusals agree, and leave the graph alone.
    let before = serde_json::to_value(&mem).unwrap();
    for (asset, account) in [
        (Some(json!({"name": "VTI", "initial_price": 1.0})), None),
        (Some(json!({"name": "Free", "initial_price": 0.0})), None),
        (
            None,
            Some(
                json!({"name": "Savings", "flavor": "Bank", "cash_value": 0.0,
                        "return_profile_id": ids.cash}),
            ),
        ),
        (
            None,
            Some(
                json!({"name": "Odd", "flavor": "Investment", "tax_status": "Taxable",
                        "cash_return_profile_id": ids.cash, "contribution_limit": 1.0}),
            ),
        ),
        (
            None,
            Some(
                json!({"name": "Bad loan", "flavor": "Liability", "principal": 1.0,
                        "repayment": {"from_account_id": ids.home, "term_months": 12}}),
            ),
        ),
    ] {
        let (sql, mem_result) = match (&asset, &account) {
            (Some(body), _) => (
                plan.create_asset(&new_asset(body.clone())).await.map(drop),
                create_asset(&mut mem, &new_asset(body.clone())).map(drop),
            ),
            (_, Some(body)) => (
                plan.create_account(&new_account(body.clone()), &[])
                    .await
                    .map(drop),
                create_account(&mut mem, &new_account(body.clone())).map(drop),
            ),
            _ => unreachable!(),
        };
        let what = format!("{asset:?} {account:?}");
        assert_eq!(
            status(sql.unwrap_err()),
            status(mem_result.unwrap_err()),
            "{what}"
        );
        assert_eq!(serde_json::to_value(&mem).unwrap(), before, "{what}");
    }
    assert_same(&plan, &mem, "after refusals").await;
}

/// A batch creating an asset, an account holding it, and an event paying into
/// the account, written through SQL and into memory, over two steps: the
/// second edits what the first created by its key.
#[tokio::test]
async fn a_batch_with_references_writes_the_same_rows_to_sql_and_memory() {
    use crate::suggest::{Change, Created, apply_steps_sql, resolve_steps};

    let plan = Plan::new().await;
    let ids = &plan.ids;
    let steps: Vec<Vec<Change>> = serde_json::from_value(json!([
        [
            {"op": "add", "target": {"new_event": "fund"}, "path": "", "value": {
                "name": "Fund the brokerage", "fires_once": true,
                "trigger": {"kind": "Date", "on_date": "2027-01-01"},
                "effects": [{"kind": "CashTransfer", "from_account_id": ids.usaa,
                             "to_account_id": {"$new": "brokerage"},
                             "amount": {"kind": "Fixed", "value": 1000.0}}]}},
            {"op": "add", "target": {"new_account": "brokerage"}, "path": "", "value": {
                "name": "Brokerage", "flavor": "Investment", "tax_status": "Taxable",
                "cash_value": 0.0, "cash_return_profile_id": ids.cash,
                "positions": [{"asset_id": {"$new": "vti"}, "units": 10.0,
                               "cost_basis": 2500.0}]}},
            {"op": "add", "target": {"new_asset": "vti"}, "path": "", "value": {
                "name": "VTI", "initial_price": 250.0, "return_profile_id": ids.equity}}
        ],
        [
            {"op": "replace", "target": {"new_account": "brokerage"}, "path": "/cash_value",
             "expect": 0.0, "value": 750.0},
            {"op": "replace", "target": {"new_event": "fund"},
             "path": "/effects/0/amount/value", "expect": 1000.0, "value": 1500.0}
        ]
    ]))
    .unwrap();

    let stepped = resolve_steps(&plan.load().await, &steps, &Created::new())
        .unwrap()
        .unwrap();

    let mut tx = plan.db.begin().await.unwrap();
    let created = apply_steps_sql(&mut tx, plan.id, &plan.user, &steps, &Created::new())
        .await
        .unwrap()
        .unwrap();
    tx.commit().await.unwrap();

    assert_eq!(created, stepped.created);
    assert_same(&plan, &stepped.graph, "two steps").await;
    let stored = plan.load().await;
    let brokerage = created["brokerage"].id;
    assert_eq!(stored.investment[&brokerage].cash_value, 750.0);
    assert_eq!(stored.positions[&brokerage][0].asset_id, created["vti"].id);
}

// ── parameters ──────────────────────────────────────────────────────────────

fn parameter(value: Value) -> crate::api::parameters::ParameterBody {
    serde_json::from_value(value).expect("parameter body")
}

impl Plan {
    async fn create_parameter(
        &self,
        body: &crate::api::parameters::ParameterBody,
    ) -> ApiResult<i64> {
        let mut conn = self.db.acquire().await?;
        crate::api::parameters::create_in(&mut conn, self.id, body).await
    }

    async fn update_parameter(
        &self,
        id: i64,
        body: &crate::api::parameters::ParameterBody,
    ) -> ApiResult<()> {
        let live = self.load().await;
        let mut tx = self.db.begin().await?;
        crate::api::parameters::update_in(&mut tx, &live, self.id, id, body).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_parameter(&self, id: i64) -> ApiResult<()> {
        let live = self.load().await;
        let mut conn = self.db.acquire().await?;
        crate::api::parameters::destroy_in(&mut conn, &live, self.id, id).await
    }
}

#[tokio::test]
async fn parameter_edits_match_the_route() {
    let plan = Plan::new().await;
    // Uses `$Spending` in an expression and RetireAge in its schedule.
    plan.create(&plan.branching()).await.unwrap();
    let mut mem = plan.load().await;
    let ids = &plan.ids;

    let floor = parameter(json!({"name": " Floor ", "value": {"kind": "Money", "value": 5000.0}}));
    let sql_id = plan.create_parameter(&floor).await.unwrap();
    let mem_id = create_parameter(&mut mem, &floor).unwrap();
    assert_eq!(sql_id, mem_id);
    assert_same(&plan, &mem, "create parameter").await;

    // Renaming rewrites the expression that names it; an unused parameter may
    // change type freely.
    let spending = plan.load().await.parameters[0].id;
    for (id, body) in [
        (
            spending,
            json!({"name": "Living", "value": {"kind": "Money", "value": 65000.0}}),
        ),
        (
            sql_id,
            json!({"name": "Floor", "value": {"kind": "Rate", "value": 0.04}}),
        ),
        (
            ids.retire_date,
            json!({"name": "RetireDate", "value": {"kind": "Date", "value": "2052-06-01"}}),
        ),
        (
            ids.retire_age,
            json!({"name": "RetireAge", "value": {"kind": "Age", "years": 62, "months": 6}}),
        ),
    ] {
        let body = parameter(body);
        plan.update_parameter(id, &body).await.unwrap();
        update_parameter(&mut mem, id, &body).unwrap();
        assert_same(&plan, &mem, &format!("update {id}")).await;
    }
    let source = mem
        .amounts
        .values()
        .find_map(|a| a.expression_source.clone())
        .unwrap();
    assert!(source.contains("$Living"), "{source}");

    // Refused alike: a duplicate name, a type change under a user, deleting a
    // parameter that is used, an unknown id, an invalid value.
    let refused: Vec<(i64, Value)> = vec![
        (
            sql_id,
            json!({"name": "Living", "value": {"kind": "Money", "value": 1.0}}),
        ),
        (
            ids.retire_age,
            json!({"name": "RetireAge", "value": {"kind": "Money", "value": 1.0}}),
        ),
        (
            spending,
            json!({"name": "Living", "value": {"kind": "Age", "years": 1, "months": 0}}),
        ),
        (
            9999,
            json!({"name": "Ghost", "value": {"kind": "Money", "value": 1.0}}),
        ),
        (
            sql_id,
            json!({"name": "Floor", "value": {"kind": "Age", "years": 1, "months": 12}}),
        ),
        (
            sql_id,
            json!({"name": "  ", "value": {"kind": "Rate", "value": 0.1}}),
        ),
    ];
    for (id, body) in refused {
        let body = parameter(body);
        let sql = plan.update_parameter(id, &body).await.unwrap_err();
        let err = update_parameter(&mut mem, id, &body).unwrap_err();
        assert_eq!(status(sql), status(err), "{id}");
    }
    let dup = parameter(json!({"name": "Living", "value": {"kind": "Money", "value": 1.0}}));
    assert_eq!(
        status(plan.create_parameter(&dup).await.unwrap_err()),
        status(create_parameter(&mut mem, &dup).unwrap_err())
    );
    for id in [spending, ids.retire_age, 9999] {
        let sql = plan.delete_parameter(id).await.unwrap_err();
        let err = delete_parameter(&mut mem, id).unwrap_err();
        assert_eq!(sql.to_string(), err.to_string(), "{id}");
    }
    assert_same(&plan, &mem, "after refusals").await;

    plan.delete_parameter(sql_id).await.unwrap();
    delete_parameter(&mut mem, sql_id).unwrap();
    assert_same(&plan, &mem, "delete unused").await;
}

// ── deleting assets and accounts ────────────────────────────────────────────

#[tokio::test]
async fn deleting_an_unused_asset_or_account_matches_the_route() {
    let plan = Plan::new().await;
    plan.create(&plan.branching()).await.unwrap();
    let ids = &plan.ids;
    let mut mem = plan.load().await;

    let asset: CreateAsset =
        serde_json::from_value(json!({"name": "Spare", "initial_price": 10.0})).unwrap();
    let account: CreateAccount = serde_json::from_value(
        json!({"name": "Spare bank", "flavor": "Bank", "cash_value": 5.0,
               "return_profile_id": ids.cash}),
    )
    .unwrap();
    let (asset_id, account_id) = {
        let mut tx = plan.db.begin().await.unwrap();
        let a = assets::create_in(&mut tx, plan.id, &asset).await.unwrap();
        let b = accounts::create_in(&mut tx, plan.id, &account)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        (a, b)
    };
    assert_eq!(asset_id, create_asset(&mut mem, &asset).unwrap());
    assert_eq!(account_id, create_account(&mut mem, &account).unwrap());
    assert_same(&plan, &mem, "created").await;

    let live = plan.load().await;
    {
        let mut conn = plan.db.acquire().await.unwrap();
        assets::destroy_in(&mut conn, &live, plan.id, asset_id)
            .await
            .unwrap();
        accounts::destroy_in(&mut conn, &live, plan.id, account_id)
            .await
            .unwrap();
    }
    delete_asset(&mut mem, asset_id).unwrap();
    delete_account(&mut mem, account_id).unwrap();
    assert_same(&plan, &mem, "deleted").await;

    // A loan loses the payer that goes, as `ON DELETE SET NULL` does.
    let payer: CreateAccount = serde_json::from_value(
        json!({"name": "Payer", "flavor": "Bank", "cash_value": 0.0, "return_profile_id": ids.cash}),
    )
    .unwrap();
    let payer_id = {
        let mut tx = plan.db.begin().await.unwrap();
        let id = accounts::create_in(&mut tx, plan.id, &payer).await.unwrap();
        tx.commit().await.unwrap();
        id
    };
    create_account(&mut mem, &payer).unwrap();
    let repaid = account_update(json!({"flavor": "Liability", "principal": 1000.0,
        "interest_rate": 0.05, "repayment": {"from_account_id": payer_id, "term_months": 60}}));
    plan.update_account(ids.mortgage, &repaid).await.unwrap();
    update_account(&mut mem, ids.mortgage, &repaid).unwrap();
    assert_same(&plan, &mem, "loan repaid from the payer").await;
    let live = plan.load().await;
    {
        let mut conn = plan.db.acquire().await.unwrap();
        accounts::destroy_in(&mut conn, &live, plan.id, payer_id)
            .await
            .unwrap();
    }
    delete_account(&mut mem, payer_id).unwrap();
    assert_same(&plan, &mem, "payer deleted").await;
    assert_eq!(mem.liability[&ids.mortgage].repay_from_account_id, None);

    // Unknown ids are not found by both.
    let mut conn = plan.db.acquire().await.unwrap();
    assert_eq!(
        status(
            assets::destroy_in(&mut conn, &live, plan.id, 9999)
                .await
                .unwrap_err()
        ),
        status(delete_asset(&mut mem, 9999).unwrap_err())
    );
    assert_eq!(
        status(
            accounts::destroy_in(&mut conn, &live, plan.id, 9999)
                .await
                .unwrap_err()
        ),
        status(delete_account(&mut mem, 9999).unwrap_err())
    );
}

/// The in-memory delete is stricter than the route, which lets the schema's
/// cascades take events, lots and amounts with the row: a change batch may
/// only delete what nothing points at, so nothing disappears unseen.
#[tokio::test]
async fn deleting_something_still_in_use_is_refused_and_says_what() {
    let plan = Plan::new().await;
    plan.create(&plan.branching()).await.unwrap();
    let ids = &plan.ids;
    let mut mem = plan.load().await;
    let before = serde_json::to_value(&mem).unwrap();

    for (result, needle) in [
        // An expression names them, as the route also refuses.
        (delete_account(&mut mem, ids.usaa), "amount expression"),
        (delete_asset(&mut mem, ids.vfiax), "amount expression"),
        // The route would cascade these away; the mirror refuses and names them.
        (delete_account(&mut mem, ids.roth), "event Windfall"),
        (delete_account(&mut mem, ids.home), "event Home Purchase"),
        (delete_asset(&mut mem, ids.bnd), "account Roth"),
        (delete_asset(&mut mem, ids.house), "account Home"),
    ] {
        let err = result.unwrap_err();
        assert_eq!(status_of(&err), axum::http::StatusCode::CONFLICT, "{err}");
        assert!(err.to_string().contains(needle), "{needle}: {err}");
    }
    assert_eq!(serde_json::to_value(&mem).unwrap(), before);
}

fn status_of(err: &ApiError) -> axum::http::StatusCode {
    match err {
        ApiError::Conflict(_) => axum::http::StatusCode::CONFLICT,
        ApiError::NotFound(_) => axum::http::StatusCode::NOT_FOUND,
        _ => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
    }
}

// ── scenario settings ───────────────────────────────────────────────────────

#[tokio::test]
async fn scenario_settings_match_the_route() {
    let plan = Plan::new().await;
    let u = json!(plan.user);
    let dist = scalar(
        &plan.db,
        "INSERT INTO distributions(user_id,kind,rate) VALUES (?,'Fixed',0.025) RETURNING id",
        std::slice::from_ref(&u),
    )
    .await;
    let inflation = scalar(
        &plan.db,
        "INSERT INTO inflation_profiles(user_id,name,distribution_id) VALUES (?,'Steady',?) RETURNING id",
        &[u.clone(), json!(dist)],
    )
    .await;
    let tax = scalar(
        &plan.db,
        "INSERT INTO tax_configs(user_id,name,state_rate) VALUES (?,'Flat',0.03) RETURNING id",
        std::slice::from_ref(&u),
    )
    .await;
    scalar(
        &plan.db,
        "INSERT INTO tax_brackets(tax_config_id,threshold,rate) VALUES (?,0,0.10) RETURNING id",
        &[json!(tax)],
    )
    .await;
    scalar(
        &plan.db,
        "INSERT INTO tax_brackets(tax_config_id,threshold,rate) VALUES (?,20000,0.22) RETURNING id",
        &[json!(tax)],
    )
    .await;
    let mut mem = plan.load().await;
    assert!(mem.tax_configs.contains_key(&tax) && mem.inflation_profiles.contains_key(&inflation));

    let update = |value: Value| -> crate::api::scenarios::UpdateScenario {
        serde_json::from_value(value).unwrap()
    };
    for edit in [
        json!({"birth_date": "1990-02-03", "duration_years": 45}),
        json!({"tax_config_id": tax}),
        json!({"inflation_profile_id": inflation, "description": "Steady prices"}),
        json!({"collect_ledger": false}),
        json!({"name": " Renamed ", "start_date": "2026-03-01"}),
    ] {
        let body = update(edit.clone());
        {
            let mut conn = plan.db.acquire().await.unwrap();
            crate::api::scenarios::update_in(&mut conn, &plan.user, plan.id, &body)
                .await
                .unwrap();
        }
        update_scenario(&mut mem, &body).unwrap();
        // Bookkeeping is not modelled.
        mem.scenario.updated_at = plan.load().await.scenario.updated_at;
        assert_same(&plan, &mem, &edit.to_string()).await;
    }
    assert_eq!(mem.tax_brackets.len(), 2);
    assert_eq!(mem.inflation_profile_name.as_deref(), Some("Steady"));

    // Refused alike (the assumption case differs in kind, not in outcome).
    for edit in [
        json!({"birth_date": "the fourth"}),
        json!({"duration_years": 0}),
        json!({"duration_years": 500}),
        json!({"name": "  "}),
        json!({"tax_config_id": 9999}),
        json!({"inflation_profile_id": 9999}),
    ] {
        let body = update(edit.clone());
        let mut conn = plan.db.acquire().await.unwrap();
        crate::api::scenarios::update_in(&mut conn, &plan.user, plan.id, &body)
            .await
            .unwrap_err();
        let before = serde_json::to_value(&mem).unwrap();
        update_scenario(&mut mem, &body).unwrap_err();
        assert_eq!(serde_json::to_value(&mem).unwrap(), before, "{edit}");
    }
}

// ── the caller's return profiles and tax configs ────────────────────────────

/// A distribution's parameters as a tree without ids, to compare across the
/// database's ids and the in-memory ones.
fn shape(graph: &ScenarioGraph, id: i64) -> Value {
    let row = &graph.distributions[&id];
    let mut out = json!({
        "kind": row.kind, "rate": row.rate, "mean": row.mean, "std_dev": row.std_dev,
        "scale": row.scale, "df": row.df, "up": row.bull_to_bear_prob,
        "down": row.bear_to_bull_prob, "preset": row.history_preset, "block": row.block_size,
    });
    if let (Some(bull), Some(bear)) = (row.bull_id, row.bear_id) {
        out["bull"] = shape(graph, bull);
        out["bear"] = shape(graph, bear);
    }
    out
}

#[tokio::test]
async fn created_return_profiles_match_the_route() {
    use crate::api::profiles::{self, CreateProfile};
    let plan = Plan::new().await;
    let mut mem = plan.load().await;
    let profile = |value: Value| -> CreateProfile { serde_json::from_value(value).unwrap() };

    for body in [
        json!({"name": " US broad ", "asset_class": "UsEquity", "description": "index funds",
               "distribution": {"kind": "Bootstrap", "preset": "sp500", "block_size": 3}}),
        json!({"name": "Regimes",
               "distribution": {"kind": "RegimeSwitching", "bull_to_bear_prob": 0.1,
                   "bear_to_bull_prob": 0.3,
                   "bull": {"kind": "Normal", "mean": 0.1, "std_dev": 0.12},
                   "bear": {"kind": "StudentT", "mean": -0.05, "scale": 0.2, "df": 4.0}}}),
        json!({"name": "Savings", "asset_class": "Cash", "distribution": {"kind": "Fixed", "rate": 0.02}}),
    ] {
        let body = profile(body);
        let sql_id = {
            let mut tx = plan.db.begin().await.unwrap();
            let id = profiles::create_return_in(&mut tx, &plan.user, &body)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            id
        };
        let mem_id = create_return_profile(&mut mem, &body).unwrap();
        let stored = plan.load().await;
        let (a, b) = (
            &stored.return_profiles[&sql_id],
            &mem.return_profiles[&mem_id],
        );
        assert_eq!(
            (&a.name, &a.description, &a.asset_class),
            (&b.name, &b.description, &b.asset_class)
        );
        assert_eq!(
            shape(&stored, a.distribution_id),
            shape(&mem, b.distribution_id),
            "{}",
            b.name
        );
        assert!(
            mem_id > 1_000_000_000,
            "in-memory ids stay clear of real ones"
        );
    }

    // A profile made in memory can be held by what the batch creates.
    let id = mem
        .return_profiles
        .values()
        .find(|p| p.name == "Savings")
        .unwrap()
        .id;
    let bank: CreateAccount = serde_json::from_value(
        json!({"name": "Held", "flavor": "Bank", "cash_value": 1.0, "return_profile_id": id}),
    )
    .unwrap();
    create_account(&mut mem, &bank).unwrap();
    compile::compile(&mem).expect("compiles with the created profile");

    // Refused alike: a taken name, a bad preset, a negative spread, an empty name.
    for body in [
        json!({"name": "Cash", "distribution": {"kind": "None"}}),
        json!({"name": "Bad", "distribution": {"kind": "Bootstrap", "preset": "nonsense"}}),
        json!({"name": "Bad", "distribution": {"kind": "Normal", "mean": 0.1, "std_dev": -1.0}}),
    ] {
        let body = profile(body);
        let mut tx = plan.db.begin().await.unwrap();
        let sql = profiles::create_return_in(&mut tx, &plan.user, &body)
            .await
            .unwrap_err();
        drop(tx);
        let before = serde_json::to_value(&mem).unwrap();
        let err = create_return_profile(&mut mem, &body).unwrap_err();
        assert_eq!(serde_json::to_value(&mem).unwrap(), before);
        assert_eq!(status(sql), status(err), "{}", body.name);
    }

    // The route takes a blank name; a change batch may not.
    let blank = profile(json!({"name": "  ", "distribution": {"kind": "None"}}));
    assert!(matches!(
        create_return_profile(&mut mem, &blank),
        Err(ApiError::BadRequest(_))
    ));
}

#[tokio::test]
async fn created_tax_configs_match_the_route() {
    use crate::api::taxes::{self, CreateTaxConfig};
    let plan = Plan::new().await;
    let mut mem = plan.load().await;
    let config = |value: Value| -> CreateTaxConfig { serde_json::from_value(value).unwrap() };

    let body = config(json!({
        "name": " Single, CO ", "state_rate": 0.044,
        "federal_brackets": [{"threshold": 11000.0, "rate": 0.12}, {"threshold": 0.0, "rate": 0.10}]
    }));
    let sql_id = {
        let mut tx = plan.db.begin().await.unwrap();
        let id = taxes::create_in(&mut tx, &plan.user, &body).await.unwrap();
        tx.commit().await.unwrap();
        id
    };
    let mem_id = create_tax_config(&mut mem, &body).unwrap();
    let stored = plan.load().await;
    let (a, b) = (&stored.tax_configs[&sql_id], &mem.tax_configs[&mem_id]);
    assert_eq!(
        serde_json::to_value((
            &a.config.name,
            a.config.state_rate,
            a.config.capital_gains_rate,
            a.config.early_withdrawal_penalty_rate
        ))
        .unwrap(),
        serde_json::to_value((
            &b.config.name,
            b.config.state_rate,
            b.config.capital_gains_rate,
            b.config.early_withdrawal_penalty_rate
        ))
        .unwrap(),
    );
    assert_eq!(
        serde_json::to_value(&a.brackets).unwrap(),
        serde_json::to_value(&b.brackets).unwrap()
    );
    assert_eq!(a.config.name, "Single, CO");
    assert_eq!(a.config.capital_gains_rate, 0.15);

    for body in [
        json!({"name": "Single, CO", "federal_brackets": [{"threshold": 0.0, "rate": 0.1}]}),
        json!({"name": "None", "federal_brackets": []}),
        json!({"name": "Gap", "federal_brackets": [{"threshold": 5.0, "rate": 0.1}]}),
        json!({"name": "Percent", "federal_brackets": [{"threshold": 0.0, "rate": 10.0}]}),
        json!({"name": "Rates", "state_rate": 4.4, "federal_brackets": [{"threshold": 0.0, "rate": 0.1}]}),
    ] {
        let body = config(body);
        let mut tx = plan.db.begin().await.unwrap();
        let sql = taxes::create_in(&mut tx, &plan.user, &body)
            .await
            .unwrap_err();
        drop(tx);
        let err = create_tax_config(&mut mem, &body).unwrap_err();
        assert_eq!(status(sql), status(err), "{}", body.name);
    }
}
