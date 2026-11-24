//! 将已查到的订阅周期按配置排序，并生成可审阅的建议权重

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::gateway::{Account, Subscription};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Order {
    StartsAt,
    ExpiresAt,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WeightPolicy {
    min_weight: u16,
    max_weight: u16,
    ranking_after_days: u16,
    initial_weight: u16,
}

impl WeightPolicy {
    pub(crate) fn new(
        min_weight: u16,
        max_weight: u16,
        ranking_after_days: u16,
        initial_weight: Option<u16>,
    ) -> Result<Self, &'static str> {
        if min_weight == 0 || max_weight > 100 || min_weight > max_weight {
            return Err("请在插件设置中将权重配置为 1 到 100 的整数，且最低权重不能高于最高权重");
        }
        let initial_weight = initial_weight.unwrap_or(min_weight);
        if ranking_after_days > 0 && !(min_weight..=max_weight).contains(&initial_weight) {
            return Err("请在插件设置中将未满排序天数时的权重设在最低和最高权重之间");
        }
        Ok(Self {
            min_weight,
            max_weight,
            ranking_after_days,
            initial_weight,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Status {
    Active,
    Initial,
    Expired,
    Unknown,
    Excluded,
    Disabled,
    Unsupported,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row<'a> {
    pub(crate) id: &'a str,
    pub(crate) label: &'a str,
    pub(crate) current_weight: u16,
    pub(crate) suggested_weight: Option<u16>,
    pub(crate) starts_at: Option<&'a str>,
    pub(crate) expires_at: Option<&'a str>,
    pub(crate) status: Status,
    #[serde(skip)]
    pub(crate) valid_until: Option<DateTime<Utc>>,
}

type RankKey = (DateTime<Utc>, Option<DateTime<Utc>>);

fn rank_key(period: &Subscription, order: Order) -> Option<RankKey> {
    match order {
        Order::StartsAt => Some((period.starts_at?, Some(period.expires_at))),
        Order::ExpiresAt => Some((
            period.expires_at,
            period.starts_at.or(Some(period.expires_at)),
        )),
    }
}

pub(crate) fn rank<'a>(
    results: &'a [(&'a Account, Result<Subscription, Status>)],
    order: Order,
    now: DateTime<Utc>,
    policy: WeightPolicy,
) -> Vec<Row<'a>> {
    let delay = TimeDelta::days(i64::from(policy.ranking_after_days));
    let mut rows: Vec<_> = results
        .iter()
        .map(|(account, period)| {
            let (status, key, valid_until) = match period {
                Err(status) => (*status, None, None),
                Ok(period)
                    if period
                        .starts_at
                        .is_some_and(|start| start >= period.expires_at || start > now) =>
                {
                    (Status::Unknown, None, None)
                }
                Ok(period) if period.expires_at <= now => (Status::Expired, None, None),
                Ok(period) => {
                    let initial_start = period
                        .starts_at
                        .filter(|start| now.signed_duration_since(*start) < delay);
                    if let Some(start) = initial_start {
                        let next_change = start
                            .checked_add_signed(delay)
                            .map_or(period.expires_at, |end| end.min(period.expires_at));
                        (Status::Initial, None, Some(next_change))
                    } else if !delay.is_zero() && period.starts_at.is_none() {
                        (Status::Unknown, None, None)
                    } else if let Some(key) = rank_key(period, order) {
                        (Status::Active, Some(key), Some(period.expires_at))
                    } else {
                        (Status::Unknown, None, None)
                    }
                }
            };
            let period = period.as_ref().ok();
            let row = Row {
                id: &account.id,
                label: if account.name.is_empty() {
                    account.email.as_deref().unwrap_or(&account.id)
                } else {
                    &account.name
                },
                current_weight: account.weight,
                suggested_weight: match status {
                    Status::Expired => Some(policy.min_weight),
                    Status::Initial => Some(policy.initial_weight),
                    _ => None,
                },
                starts_at: period.and_then(|value| value.starts_at_display.as_deref()),
                expires_at: period.map(|value| value.expires_at_display.as_str()),
                status,
                valid_until,
            };
            (row, key)
        })
        .collect();

    rows.sort_unstable_by_key(|(row, key)| (row.status, *key, row.id));
    let mut weight = policy.max_weight;
    let active_min = (policy.min_weight + 1).min(policy.max_weight);
    let mut previous = None;
    for (row, key) in &mut rows {
        if let Some(key) = key {
            if previous.is_some_and(|previous| previous != *key) {
                weight = weight.saturating_sub(1).max(active_min);
            }
            row.suggested_weight = Some(weight);
            previous = Some(*key);
        }
    }
    rows.into_iter().map(|(row, _)| row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::Capabilities;

    fn account(id: &str) -> Account {
        Account {
            id: id.into(),
            name: id.into(),
            email: None,
            account_id: None,
            plan_type: Some("plus".into()),
            capabilities: Capabilities { subscription: true },
            enabled: true,
            weight: 40,
            updated_at: String::new(),
        }
    }

    fn date(day: u32) -> DateTime<Utc> {
        format!("2026-11-{day:02}T00:00:00Z").parse().unwrap()
    }

    fn period(start: Option<u32>, end: u32) -> Result<Subscription, Status> {
        Ok(Subscription {
            starts_at: start.map(date),
            starts_at_display: None,
            expires_at: date(end),
            expires_at_display: String::new(),
        })
    }

    #[test]
    fn sorts_by_first_date_and_shares_weights_for_identical_periods() {
        let first = account("first");
        let second = account("second");
        let tied = account("tied");
        let records = [
            (&second, period(Some(4), 22)),
            (&tied, period(Some(2), 23)),
            (&first, period(Some(2), 23)),
        ];
        let weights = |order| {
            let policy = WeightPolicy::new(1, 100, 0, None).unwrap();
            rank(&records, order, date(10), policy)
                .into_iter()
                .map(|row| (row.id, row.suggested_weight))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            weights(Order::StartsAt),
            [
                ("first", Some(100)),
                ("tied", Some(100)),
                ("second", Some(99))
            ]
        );
        assert_eq!(
            weights(Order::ExpiresAt),
            [
                ("second", Some(100)),
                ("first", Some(99)),
                ("tied", Some(99))
            ]
        );
    }

    #[test]
    fn only_verified_periods_receive_weights() {
        let expired = account("expired");
        let unknown = account("unknown");
        let excluded = account("excluded");
        let records = [
            (&expired, period(Some(1), 10)),
            (&unknown, period(None, 23)),
            (&excluded, Err(Status::Excluded)),
        ];
        let policy = WeightPolicy::new(1, 100, 0, None).unwrap();
        let weights = rank(&records, Order::StartsAt, date(10), policy)
            .into_iter()
            .map(|row| (row.id, row.suggested_weight))
            .collect::<Vec<_>>();
        assert_eq!(
            weights,
            [("expired", Some(1)), ("unknown", None), ("excluded", None)]
        );
    }

    #[test]
    fn applies_configured_bounds_to_active_and_expired_accounts() {
        let first = account("acct_first");
        let second = account("acct_second");
        let third = account("acct_third");
        let expired = account("acct_expired");
        let records = [
            (&first, period(Some(1), 20)),
            (&second, period(Some(2), 20)),
            (&third, period(Some(3), 20)),
            (&expired, period(Some(1), 8)),
        ];
        for (max_weight, expected) in [(12, [12, 11, 11, 10]), (10, [10, 10, 10, 10])] {
            let policy = WeightPolicy::new(10, max_weight, 0, None).unwrap();
            let weights: Vec<_> = rank(&records, Order::StartsAt, date(10), policy)
                .into_iter()
                .map(|row| row.suggested_weight.unwrap())
                .collect();
            assert_eq!(weights, expected);
        }
        assert!(WeightPolicy::new(20, 10, 0, None).is_err());
        assert!(WeightPolicy::new(10, 20, 3, Some(21)).is_err());
    }

    #[test]
    fn changes_from_uniform_weight_to_date_order_when_the_interval_elapses() {
        let older = account("acct_older");
        let newer = account("acct_newer");
        let youngest = account("acct_youngest");
        let short = account("acct_short");
        let unknown = account("acct_unknown_start");
        let records = [
            (&older, period(Some(6), 21)),
            (&newer, period(Some(7), 20)),
            (&youngest, period(Some(8), 22)),
            (&short, period(Some(8), 10)),
            (&unknown, period(None, 20)),
        ];
        let policy = WeightPolicy::new(10, 30, 3, Some(17)).unwrap();
        let states = |now| {
            rank(&records, Order::ExpiresAt, now, policy)
                .into_iter()
                .map(|row| (row.id, row.suggested_weight, row.valid_until))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            states(date(10) - TimeDelta::seconds(1)),
            [
                ("acct_older", Some(30), Some(date(21))),
                ("acct_newer", Some(17), Some(date(10))),
                ("acct_short", Some(17), Some(date(10))),
                ("acct_youngest", Some(17), Some(date(11))),
                ("acct_unknown_start", None, None),
            ]
        );
        assert_eq!(
            states(date(10)),
            [
                ("acct_newer", Some(30), Some(date(20))),
                ("acct_older", Some(29), Some(date(21))),
                ("acct_youngest", Some(17), Some(date(11))),
                ("acct_short", Some(10), None),
                ("acct_unknown_start", None, None),
            ]
        );
    }
}
