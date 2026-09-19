//! Admin-auth singleton reads.
//!
//! Password hashes and the forced-change flag live behind these two queries
//! so session handlers never touch the `AdminAuth` table directly.

use super::SINGLETON_KEY;
use crate::infra::storage::{Database, Record, StorageError, Table};

pub async fn has_admin_auth(database: &Database) -> Result<bool, StorageError> {
    database
        .read(|transaction| transaction.get::<Record>(Table::AdminAuth, SINGLETON_KEY))
        .await
        .map(|record| record.is_some())
}

pub async fn admin_password_change_required(
    database: &Database,
) -> Result<Option<bool>, StorageError> {
    database
        .read(|transaction| {
            transaction
                .get::<Record>(Table::AdminAuth, SINGLETON_KEY)?
                .map(|record| record.boolean("must_change_password"))
                .transpose()
        })
        .await
}
