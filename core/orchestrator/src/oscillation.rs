//! Pure computation: how many times a ride's price direction reversed
//! across an ordered sequence of `set_ride_price` actions. Used by the
//! watchdog binary to detect the "unstable ride" pattern
//! config/constitution-0.1.yaml's `oscillation_max_reversals` guards
//! against -- a price that keeps flipping up-down-up-down is a sign of a
//! feedback loop between the operator rule and its own effects, not a
//! healthy response to demand.

/// `prices` must already be ordered by tick ascending. A reversal is a
/// direction change (price went up, then down, or down then up) -- equal
/// consecutive prices aren't a direction at all and don't reset or count
/// toward one.
pub fn count_reversals(prices: &[i64]) -> u32 {
    let mut reversals = 0;
    let mut last_direction: Option<std::cmp::Ordering> = None;
    for pair in prices.windows(2) {
        let direction = pair[0].cmp(&pair[1]);
        if direction == std::cmp::Ordering::Equal {
            continue;
        }
        if let Some(last) = last_direction {
            if last != direction {
                reversals += 1;
            }
        }
        last_direction = Some(direction);
    }
    reversals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_reversals_when_monotonic() {
        assert_eq!(count_reversals(&[5, 6, 7, 8]), 0);
        assert_eq!(count_reversals(&[8, 7, 6, 5]), 0);
    }

    #[test]
    fn counts_each_direction_flip() {
        assert_eq!(count_reversals(&[5, 6, 5, 6, 5]), 3);
    }

    #[test]
    fn equal_prices_do_not_count_as_a_direction() {
        assert_eq!(count_reversals(&[5, 5, 6, 6, 5]), 1);
    }

    #[test]
    fn empty_or_single_price_has_no_reversals() {
        assert_eq!(count_reversals(&[]), 0);
        assert_eq!(count_reversals(&[5]), 0);
    }
}
