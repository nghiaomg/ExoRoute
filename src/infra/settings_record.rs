use crate::config::OperationalSettings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OperationalSettingsRecord {
    pub settings: OperationalSettings,
    pub revision: i64,
    pub overridden: bool,
}
