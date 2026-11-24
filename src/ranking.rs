//! 将已查到的订阅周期按配置排序，并生成可审阅的建议权重

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::gateway::{Account, Subscription};

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Order {
    StartsAt,
    ExpiresAt,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Status {
    Active,
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
    pub(crate) active_until: Option<DateTime<Utc>>,
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
) -> Vec<Row<'a>> {
    let mut rows: Vec<_> = results
        .iter()
        .map(|(account, period)| {
            let (status, key) = match period {
                Err(status) => (*status, None),
                Ok(period)
                    if period
                        .starts_at
                        .is_some_and(|start| start >= period.expires_at || start > now) =>
                {
                    (Status::Unknown, None)
                }
                Ok(period) if period.expires_at <= now => (Status::Expired, None),
                Ok(period) => match rank_key(period, order) {
                    Some(key) => (Status::Active, Some(key)),
                    None => (Status::Unknown, None),
                },
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
                suggested_weight: (status == Status::Expired).then_some(1),
                starts_at: period.and_then(|value| value.starts_at_display.as_deref()),
                expires_at: period.map(|value| value.expires_at_display.as_str()),
                status,
                active_until: period
                    .filter(|_| status == Status::Active)
                    .map(|value| value.expires_at),
            };
            (row, key)
        })
        .collect();

    rows.sort_unstable_by_key(|(row, key)| (row.status, *key, row.id));
    let mut weight = 100_u16;
    let mut previous = None;
    for (row, key) in &mut rows {
        if let Some(key) = key {
            if previous.is_some_and(|previous| previous != *key) {
                weight = weight.saturating_sub(1).max(2);
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
            rank(&records, order, date(10))
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
        let weights = rank(&records, Order::StartsAt, date(10))
            .into_iter()
            .map(|row| (row.id, row.suggested_weight))
            .collect::<Vec<_>>();
        assert_eq!(
            weights,
            [("expired", Some(1)), ("unknown", None), ("excluded", None)]
        );
    }
}
