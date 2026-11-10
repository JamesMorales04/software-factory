//! Streaming, checked aggregation of durable token and cost columns.
//!
//! SQLite's `SUM()` over a signed `INTEGER` column raises `integer overflow`
//! once a scope contains enough legacy large reservations. Reading individual
//! rows and folding them in `u128` keeps those rows readable, but a whole
//! history must not be materialized in memory. These helpers consume the row
//! stream one row at a time and preserve the fail-closed projection semantics:
//! a total above `u64` saturates at `u64::MAX`, which can only make a budget
//! look exhausted, never more available.

use super::Error;
use futures_util::TryStreamExt;
use sqlx::row::Row;
use sqlx_sqlite::SqliteConnection;

/// Fold non-negative `INTEGER` columns from a row stream in checked `u128`
/// arithmetic. `binds` are bound in order; `columns` are read by name from each
/// row and returned in the same order. Values above `u64::MAX` saturate.
pub(crate) async fn checked_stream_totals(
    connection: &mut SqliteConnection,
    sql: &str,
    binds: &[&str],
    columns: &[&str],
    field: &'static str,
) -> Result<Vec<u64>, Error> {
    if columns.is_empty() {
        return Err(Error::Invalid(field));
    }
    let mut query = sqlx::query::query(sql);
    for value in binds {
        query = query.bind(*value);
    }
    let mut rows = query.fetch(&mut *connection);
    let mut totals = vec![0u128; columns.len()];
    while let Some(row) = rows.try_next().await? {
        for (index, column) in columns.iter().enumerate() {
            let value: i64 = row.try_get(*column)?;
            totals[index] = totals[index]
                .checked_add(u128::try_from(value).map_err(|_| Error::Invalid(field))?)
                .ok_or(Error::Invalid(field))?;
        }
    }
    Ok(totals
        .into_iter()
        .map(|total| u64::try_from(total).unwrap_or(u64::MAX))
        .collect())
}
