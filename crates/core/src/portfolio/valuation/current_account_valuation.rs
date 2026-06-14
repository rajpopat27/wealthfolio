use chrono::{DateTime, NaiveDate, Utc};
use log::warn;
use rust_decimal::Decimal;
use std::collections::{HashMap, HashSet};

use crate::{
    accounts::{account_supports_purpose, Account, AccountPurpose, AccountServiceTrait},
    assets::{Asset, AssetServiceTrait, InstrumentType},
    errors::Result,
    fx::{currency::normalize_amount, FxServiceTrait},
    portfolio::{
        snapshot::{AccountStateSnapshot, SnapshotRepositoryTrait},
        valuation::{
            CurrentAccountValuation, CurrentValuationResponse, CurrentValuationSplit,
            CurrentValuationSummary,
        },
    },
    quotes::{LatestQuotePair, QuoteServiceTrait},
    utils::occ_symbol::parse_occ_symbol,
};

pub struct CurrentAccountValuationService<'a> {
    account_service: &'a dyn AccountServiceTrait,
    snapshot_repository: &'a dyn SnapshotRepositoryTrait,
    asset_service: &'a dyn AssetServiceTrait,
    quote_service: &'a dyn QuoteServiceTrait,
    fx_service: &'a dyn FxServiceTrait,
}

impl<'a> CurrentAccountValuationService<'a> {
    pub fn new(
        account_service: &'a dyn AccountServiceTrait,
        snapshot_repository: &'a dyn SnapshotRepositoryTrait,
        asset_service: &'a dyn AssetServiceTrait,
        quote_service: &'a dyn QuoteServiceTrait,
        fx_service: &'a dyn FxServiceTrait,
    ) -> Self {
        Self {
            account_service,
            snapshot_repository,
            asset_service,
            quote_service,
            fx_service,
        }
    }

    pub async fn get_current_account_valuations(
        &self,
        account_ids: &[String],
        base_currency: &str,
        latest_snapshot_cutoff: NaiveDate,
    ) -> Result<Vec<CurrentAccountValuation>> {
        self.get_current_account_valuations_at(
            account_ids,
            base_currency,
            latest_snapshot_cutoff,
            Utc::now(),
        )
        .await
    }

    pub async fn get_current_account_valuations_at(
        &self,
        account_ids: &[String],
        base_currency: &str,
        latest_snapshot_cutoff: NaiveDate,
        calculated_at: DateTime<Utc>,
    ) -> Result<Vec<CurrentAccountValuation>> {
        let accounts = self.resolve_accounts(account_ids)?;
        if accounts.is_empty() {
            return Ok(Vec::new());
        }

        let account_ids: Vec<String> = accounts.iter().map(|account| account.id.clone()).collect();
        let snapshots =
            self.latest_snapshots_with_positions(&account_ids, latest_snapshot_cutoff)?;
        let assets_by_id = self.load_assets_by_id(&snapshots).await?;
        let latest_quote_pairs = self.load_latest_quotes(&snapshots, &assets_by_id)?;
        let mut fx_cache = FxRateCache::new(self.fx_service);

        Ok(calculate_current_account_valuations_from_snapshots(
            &accounts,
            &snapshots,
            &assets_by_id,
            &latest_quote_pairs,
            base_currency,
            calculated_at,
            |from, to| fx_cache.get(from, to),
        ))
    }

    pub async fn get_current_valuation_for_scope(
        &self,
        scope_id: &str,
        account_ids: &[String],
        base_currency: &str,
        latest_snapshot_cutoff: NaiveDate,
        include_accounts: bool,
    ) -> Result<CurrentValuationResponse> {
        self.get_current_valuation_for_scope_at(
            scope_id,
            account_ids,
            base_currency,
            latest_snapshot_cutoff,
            include_accounts,
            Utc::now(),
        )
        .await
    }

    pub async fn get_current_valuation_for_scope_at(
        &self,
        scope_id: &str,
        account_ids: &[String],
        base_currency: &str,
        latest_snapshot_cutoff: NaiveDate,
        include_accounts: bool,
        calculated_at: DateTime<Utc>,
    ) -> Result<CurrentValuationResponse> {
        let accounts = self.resolve_requested_accounts(account_ids)?;
        if accounts.is_empty() {
            return Ok(empty_current_valuation_response(
                scope_id,
                base_currency,
                calculated_at,
            ));
        }

        let account_ids: Vec<String> = accounts.iter().map(|account| account.id.clone()).collect();
        let snapshots =
            self.latest_snapshots_with_positions(&account_ids, latest_snapshot_cutoff)?;
        let assets_by_id = self.load_assets_by_id(&snapshots).await?;
        let latest_quote_pairs = self.load_latest_quotes(&snapshots, &assets_by_id)?;
        let mut fx_cache = FxRateCache::new(self.fx_service);

        Ok(calculate_current_valuation_response_from_snapshots(
            scope_id,
            &accounts,
            &snapshots,
            &assets_by_id,
            &latest_quote_pairs,
            base_currency,
            calculated_at,
            include_accounts,
            |from, to| fx_cache.get(from, to),
        ))
    }

    fn resolve_accounts(&self, account_ids: &[String]) -> Result<Vec<Account>> {
        if account_ids.is_empty() {
            let accounts = self.account_service.get_active_accounts()?;
            return Ok(filter_current_valuation_accounts(None, accounts));
        }

        let requested_ids = unique_account_ids(account_ids.iter().cloned());
        let accounts = self.account_service.get_accounts_by_ids(&requested_ids)?;
        Ok(filter_current_valuation_accounts(
            Some(&requested_ids),
            accounts,
        ))
    }

    fn resolve_requested_accounts(&self, account_ids: &[String]) -> Result<Vec<Account>> {
        if account_ids.is_empty() {
            return Ok(Vec::new());
        }

        let requested_ids = unique_account_ids(account_ids.iter().cloned());
        let accounts = self.account_service.get_accounts_by_ids(&requested_ids)?;
        Ok(filter_current_valuation_accounts(
            Some(&requested_ids),
            accounts,
        ))
    }

    fn latest_snapshots_with_positions(
        &self,
        account_ids: &[String],
        latest_snapshot_cutoff: NaiveDate,
    ) -> Result<HashMap<String, AccountStateSnapshot>> {
        let mut snapshots = self
            .snapshot_repository
            .get_latest_snapshots_before_date(account_ids, latest_snapshot_cutoff)?;
        if snapshots.is_empty() {
            return Ok(snapshots);
        }

        let snapshot_ids: Vec<String> = snapshots
            .values()
            .map(|snapshot| snapshot.id.clone())
            .collect();
        let positions_by_snapshot_id = self
            .snapshot_repository
            .get_snapshot_positions_batch(&snapshot_ids)?;

        for snapshot in snapshots.values_mut() {
            if let Some(positions) = positions_by_snapshot_id.get(&snapshot.id) {
                snapshot.positions = positions.clone();
            }
        }

        Ok(snapshots)
    }

    async fn load_assets_by_id(
        &self,
        snapshots: &HashMap<String, AccountStateSnapshot>,
    ) -> Result<HashMap<String, Asset>> {
        let asset_ids = snapshot_asset_ids(snapshots);
        if asset_ids.is_empty() {
            return Ok(HashMap::new());
        }

        Ok(self
            .asset_service
            .get_assets_by_asset_ids(&asset_ids)
            .await?
            .into_iter()
            .map(|asset| (asset.id.clone(), asset))
            .collect())
    }

    fn load_latest_quotes(
        &self,
        snapshots: &HashMap<String, AccountStateSnapshot>,
        assets_by_id: &HashMap<String, Asset>,
    ) -> Result<HashMap<String, LatestQuotePair>> {
        let asset_ids = quoted_asset_ids(snapshots, assets_by_id);
        if asset_ids.is_empty() {
            return Ok(HashMap::new());
        }

        self.quote_service.get_latest_quotes_pair(&asset_ids)
    }
}

pub fn calculate_current_account_valuations_from_snapshots<F>(
    accounts: &[Account],
    snapshots: &HashMap<String, AccountStateSnapshot>,
    assets_by_id: &HashMap<String, Asset>,
    latest_quote_pairs: &HashMap<String, LatestQuotePair>,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
    mut latest_rate: F,
) -> Vec<CurrentAccountValuation>
where
    F: FnMut(&str, &str) -> Decimal,
{
    accounts
        .iter()
        .map(|account| {
            let Some(snapshot) = snapshots.get(&account.id) else {
                return zero_current_account_valuation(account, base_currency, calculated_at);
            };

            calculate_account_snapshot_valuation(
                account,
                snapshot,
                assets_by_id,
                latest_quote_pairs,
                base_currency,
                calculated_at,
                &mut latest_rate,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub fn calculate_current_valuation_response_from_snapshots<F>(
    scope_id: &str,
    accounts: &[Account],
    snapshots: &HashMap<String, AccountStateSnapshot>,
    assets_by_id: &HashMap<String, Asset>,
    latest_quote_pairs: &HashMap<String, LatestQuotePair>,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
    include_accounts: bool,
    mut latest_rate: F,
) -> CurrentValuationResponse
where
    F: FnMut(&str, &str) -> Decimal,
{
    let mut summary = CurrentValuationSummary {
        scope_id: scope_id.to_string(),
        base_currency: base_currency.to_string(),
        cash_balance_base: Decimal::ZERO,
        investment_market_value_base: Decimal::ZERO,
        total_value_base: Decimal::ZERO,
        holdings_count: 0,
        account_count: accounts.len(),
        currency_split: Vec::new(),
        cash_currency_split: Vec::new(),
        source_data_as_of: None,
        calculated_at,
    };
    let mut currency_base_totals: HashMap<String, Decimal> = HashMap::new();
    let mut cash_base_totals: HashMap<String, Decimal> = HashMap::new();
    let mut cash_local_totals: HashMap<String, Decimal> = HashMap::new();
    let mut account_valuations = Vec::new();

    for account in accounts {
        let computation = match snapshots.get(&account.id) {
            Some(snapshot) => calculate_account_snapshot_valuation_with_contributions(
                account,
                snapshot,
                assets_by_id,
                latest_quote_pairs,
                base_currency,
                calculated_at,
                &mut latest_rate,
            ),
            None => AccountValuationComputation {
                valuation: zero_current_account_valuation(account, base_currency, calculated_at),
                holdings_count: 0,
                currency_base_totals: HashMap::new(),
                cash_base_totals: HashMap::new(),
                cash_local_totals: HashMap::new(),
            },
        };

        summary.cash_balance_base += computation.valuation.cash_balance_base;
        summary.investment_market_value_base += computation.valuation.investment_market_value_base;
        summary.total_value_base += computation.valuation.total_value_base;
        summary.holdings_count += computation.holdings_count;
        summary.source_data_as_of = latest_datetime(
            summary.source_data_as_of,
            computation.valuation.source_data_as_of,
        );
        merge_totals(&mut currency_base_totals, computation.currency_base_totals);
        merge_totals(&mut cash_base_totals, computation.cash_base_totals);
        merge_totals(&mut cash_local_totals, computation.cash_local_totals);

        if include_accounts {
            account_valuations.push(computation.valuation);
        }
    }

    summary.currency_split = build_currency_split(currency_base_totals, summary.total_value_base);
    summary.cash_currency_split = build_cash_currency_split(
        cash_local_totals,
        cash_base_totals,
        summary.cash_balance_base,
    );

    CurrentValuationResponse {
        summary,
        accounts: account_valuations,
    }
}

pub fn filter_current_valuation_accounts(
    requested_ids: Option<&[String]>,
    accounts: Vec<Account>,
) -> Vec<Account> {
    match requested_ids {
        None => accounts
            .into_iter()
            .filter(|account| {
                account.is_active
                    && !account.is_archived
                    && account_supports_purpose(&account.account_type, AccountPurpose::Performance)
                    && account_supports_purpose(&account.account_type, AccountPurpose::Holdings)
            })
            .collect(),
        Some(ids) => {
            let accounts_by_id: HashMap<String, Account> = accounts
                .into_iter()
                .map(|account| (account.id.clone(), account))
                .collect();

            ids.iter()
                .filter_map(|account_id| accounts_by_id.get(account_id).cloned())
                .filter(|account| {
                    account.is_active
                        && !account.is_archived
                        && account_supports_purpose(
                            &account.account_type,
                            AccountPurpose::Performance,
                        )
                        && account_supports_purpose(&account.account_type, AccountPurpose::Holdings)
                })
                .collect()
        }
    }
}

pub fn unique_account_ids(account_ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    account_ids
        .into_iter()
        .filter(|account_id| seen.insert(account_id.clone()))
        .collect()
}

fn calculate_account_snapshot_valuation<F>(
    account: &Account,
    snapshot: &AccountStateSnapshot,
    assets_by_id: &HashMap<String, Asset>,
    latest_quote_pairs: &HashMap<String, LatestQuotePair>,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
    latest_rate: &mut F,
) -> CurrentAccountValuation
where
    F: FnMut(&str, &str) -> Decimal,
{
    calculate_account_snapshot_valuation_with_contributions(
        account,
        snapshot,
        assets_by_id,
        latest_quote_pairs,
        base_currency,
        calculated_at,
        latest_rate,
    )
    .valuation
}

struct AccountValuationComputation {
    valuation: CurrentAccountValuation,
    holdings_count: usize,
    currency_base_totals: HashMap<String, Decimal>,
    cash_base_totals: HashMap<String, Decimal>,
    cash_local_totals: HashMap<String, Decimal>,
}

fn calculate_account_snapshot_valuation_with_contributions<F>(
    account: &Account,
    snapshot: &AccountStateSnapshot,
    assets_by_id: &HashMap<String, Asset>,
    latest_quote_pairs: &HashMap<String, LatestQuotePair>,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
    latest_rate: &mut F,
) -> AccountValuationComputation
where
    F: FnMut(&str, &str) -> Decimal,
{
    let mut valuation = zero_current_account_valuation(account, base_currency, calculated_at);
    valuation.source_data_as_of = Some(snapshot_source_data_as_of(snapshot));
    let mut holdings_count = 0;
    let mut currency_base_totals: HashMap<String, Decimal> = HashMap::new();
    let mut cash_base_totals: HashMap<String, Decimal> = HashMap::new();
    let mut cash_local_totals: HashMap<String, Decimal> = HashMap::new();

    for (cash_currency, amount) in &snapshot.cash_balances {
        let account_value = *amount * latest_rate(cash_currency, &account.currency);
        let base_value = *amount * latest_rate(cash_currency, base_currency);
        valuation.cash_balance += account_value;
        valuation.cash_balance_base += base_value;

        if !amount.is_zero() {
            holdings_count += 1;
            add_total(&mut currency_base_totals, cash_currency, base_value);
            add_total(&mut cash_base_totals, cash_currency, base_value);
            add_total(&mut cash_local_totals, cash_currency, *amount);
        }
    }

    for (asset_id, position) in &snapshot.positions {
        if position.quantity == Decimal::ZERO || position.is_alternative {
            continue;
        }

        let Some(asset) = assets_by_id.get(asset_id) else {
            warn!(
                "Skipping current valuation for position {} in account {} because asset metadata is missing",
                asset_id, account.id
            );
            continue;
        };
        if asset.is_alternative() {
            continue;
        }
        if is_expired_option_asset(asset, calculated_at.date_naive()) {
            continue;
        }

        holdings_count += 1;

        let Some(quote_pair) = latest_quote_pairs.get(asset_id) else {
            warn!(
                "Missing latest quote for asset {} in account {}. Current market value treated as zero.",
                asset_id, account.id
            );
            continue;
        };
        valuation.source_data_as_of = latest_datetime(
            valuation.source_data_as_of,
            Some(quote_pair.latest.timestamp),
        );

        let (normalized_price, normalized_quote_currency) =
            normalize_amount(quote_pair.latest.close, &quote_pair.latest.currency);
        let market_value_quote =
            normalized_price * position.quantity * position.contract_multiplier;
        let market_value_base =
            market_value_quote * latest_rate(normalized_quote_currency, base_currency);

        valuation.investment_market_value +=
            market_value_quote * latest_rate(normalized_quote_currency, &account.currency);
        valuation.investment_market_value_base += market_value_base;
        add_total(
            &mut currency_base_totals,
            &position.currency,
            market_value_base,
        );
    }

    valuation.total_value = valuation.cash_balance + valuation.investment_market_value;
    valuation.total_value_base =
        valuation.cash_balance_base + valuation.investment_market_value_base;
    AccountValuationComputation {
        valuation,
        holdings_count,
        currency_base_totals,
        cash_base_totals,
        cash_local_totals,
    }
}

fn zero_current_account_valuation(
    account: &Account,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
) -> CurrentAccountValuation {
    CurrentAccountValuation {
        account_id: account.id.clone(),
        account_currency: account.currency.clone(),
        base_currency: base_currency.to_string(),
        cash_balance: Decimal::ZERO,
        investment_market_value: Decimal::ZERO,
        total_value: Decimal::ZERO,
        cash_balance_base: Decimal::ZERO,
        investment_market_value_base: Decimal::ZERO,
        total_value_base: Decimal::ZERO,
        source_data_as_of: None,
        calculated_at,
    }
}

fn snapshot_asset_ids(snapshots: &HashMap<String, AccountStateSnapshot>) -> Vec<String> {
    unique_asset_ids(
        snapshots
            .values()
            .flat_map(|snapshot| snapshot.positions.keys().cloned()),
    )
}

fn quoted_asset_ids(
    snapshots: &HashMap<String, AccountStateSnapshot>,
    assets_by_id: &HashMap<String, Asset>,
) -> Vec<String> {
    unique_asset_ids(snapshots.values().flat_map(|snapshot| {
        snapshot
            .positions
            .iter()
            .filter_map(|(asset_id, position)| {
                if position.quantity == Decimal::ZERO || position.is_alternative {
                    return None;
                }
                let asset = assets_by_id.get(asset_id)?;
                (!asset.is_alternative()).then(|| asset_id.clone())
            })
    }))
}

fn unique_asset_ids(asset_ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    asset_ids
        .into_iter()
        .filter(|asset_id| seen.insert(asset_id.clone()))
        .collect()
}

fn empty_current_valuation_response(
    scope_id: &str,
    base_currency: &str,
    calculated_at: DateTime<Utc>,
) -> CurrentValuationResponse {
    CurrentValuationResponse {
        summary: CurrentValuationSummary {
            scope_id: scope_id.to_string(),
            base_currency: base_currency.to_string(),
            cash_balance_base: Decimal::ZERO,
            investment_market_value_base: Decimal::ZERO,
            total_value_base: Decimal::ZERO,
            holdings_count: 0,
            account_count: 0,
            currency_split: Vec::new(),
            cash_currency_split: Vec::new(),
            source_data_as_of: None,
            calculated_at,
        },
        accounts: Vec::new(),
    }
}

fn add_total(totals: &mut HashMap<String, Decimal>, currency: &str, value: Decimal) {
    totals
        .entry(currency.to_string())
        .and_modify(|total| *total += value)
        .or_insert(value);
}

fn merge_totals(target: &mut HashMap<String, Decimal>, source: HashMap<String, Decimal>) {
    for (currency, value) in source {
        add_total(target, &currency, value);
    }
}

fn build_currency_split(
    totals: HashMap<String, Decimal>,
    total_value_base: Decimal,
) -> Vec<CurrentValuationSplit> {
    let mut rows: Vec<CurrentValuationSplit> = totals
        .into_iter()
        .filter(|(_, value)| !value.is_zero())
        .map(|(currency, value)| CurrentValuationSplit {
            currency,
            value_base: value,
            value_local: None,
            percentage: percentage(value, total_value_base),
        })
        .collect();
    rows.sort_by(|a, b| {
        b.value_base
            .cmp(&a.value_base)
            .then_with(|| a.currency.cmp(&b.currency))
    });
    rows
}

fn build_cash_currency_split(
    cash_local_totals: HashMap<String, Decimal>,
    cash_base_totals: HashMap<String, Decimal>,
    cash_balance_base: Decimal,
) -> Vec<CurrentValuationSplit> {
    let mut rows: Vec<CurrentValuationSplit> = cash_local_totals
        .into_iter()
        .filter(|(_, value)| !value.is_zero())
        .map(|(currency, value)| {
            let base_value = cash_base_totals
                .get(&currency)
                .copied()
                .unwrap_or(Decimal::ZERO);
            CurrentValuationSplit {
                currency,
                value_base: base_value,
                value_local: Some(value),
                percentage: percentage(base_value, cash_balance_base),
            }
        })
        .collect();
    rows.sort_by(|a, b| {
        b.percentage
            .cmp(&a.percentage)
            .then_with(|| a.currency.cmp(&b.currency))
    });
    rows
}

fn percentage(value: Decimal, total: Decimal) -> Decimal {
    if total > Decimal::ZERO {
        ((value / total) * Decimal::from(100)).round_dp(4)
    } else {
        Decimal::ZERO
    }
}

fn latest_datetime(
    current: Option<DateTime<Utc>>,
    candidate: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    match (current, candidate) {
        (Some(current), Some(candidate)) => Some(current.max(candidate)),
        (Some(current), None) => Some(current),
        (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

fn snapshot_source_data_as_of(snapshot: &AccountStateSnapshot) -> DateTime<Utc> {
    DateTime::<Utc>::from_naive_utc_and_offset(snapshot.calculated_at, Utc)
}

fn is_expired_option_asset(asset: &Asset, today: NaiveDate) -> bool {
    if asset.instrument_type.as_ref() != Some(&InstrumentType::Option) {
        return false;
    }

    let expiration = asset
        .metadata
        .as_ref()
        .and_then(|m| m.get("option"))
        .and_then(|o| o.get("expiration"))
        .and_then(|v| v.as_str())
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .or_else(|| {
            [
                asset.instrument_symbol.as_deref().unwrap_or_default(),
                asset.display_code.as_deref().unwrap_or_default(),
            ]
            .iter()
            .find_map(|symbol| {
                parse_occ_symbol(symbol)
                    .ok()
                    .map(|parsed| parsed.expiration)
            })
        });

    matches!(expiration, Some(expiration) if expiration < today)
}

struct FxRateCache<'a> {
    fx_service: &'a dyn FxServiceTrait,
    rates: HashMap<(String, String), Decimal>,
}

impl<'a> FxRateCache<'a> {
    fn new(fx_service: &'a dyn FxServiceTrait) -> Self {
        Self {
            fx_service,
            rates: HashMap::new(),
        }
    }

    fn get(&mut self, from: &str, to: &str) -> Decimal {
        if from == to {
            return Decimal::ONE;
        }

        let key = (from.to_string(), to.to_string());
        if let Some(rate) = self.rates.get(&key) {
            return *rate;
        }

        let rate = match self.fx_service.get_latest_exchange_rate(from, to) {
            Ok(rate) => rate,
            Err(error) => {
                warn!(
                    "Falling back to FX rate 1 while calculating current valuation from {} to {}: {}",
                    from, to, error
                );
                Decimal::ONE
            }
        };
        self.rates.insert(key, rate);
        rate
    }
}
