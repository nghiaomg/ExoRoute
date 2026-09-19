use super::*;
use std::io::{Read, Seek, SeekFrom};

pub(crate) fn preflight_backup_reader(
    file: &mut fs::File,
    payload_len: u64,
) -> Result<(), &'static str> {
    fn read_u32(
        file: &mut fs::File,
        offset: &mut u64,
        payload_len: u64,
    ) -> Result<u32, &'static str> {
        if offset.checked_add(4).is_none_or(|end| end > payload_len) {
            return Err("The backup payload is truncated.");
        }
        let mut bytes = [0_u8; 4];
        file.read_exact(&mut bytes)
            .map_err(|_| "The backup payload is truncated.")?;
        *offset += 4;
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_u64(
        file: &mut fs::File,
        offset: &mut u64,
        payload_len: u64,
    ) -> Result<u64, &'static str> {
        if offset.checked_add(8).is_none_or(|end| end > payload_len) {
            return Err("The backup payload is truncated.");
        }
        let mut bytes = [0_u8; 8];
        file.read_exact(&mut bytes)
            .map_err(|_| "The backup payload is truncated.")?;
        *offset += 8;
        Ok(u64::from_le_bytes(bytes))
    }

    fn skip(
        file: &mut fs::File,
        offset: &mut u64,
        payload_len: u64,
        amount: u64,
    ) -> Result<(), &'static str> {
        let end = offset
            .checked_add(amount)
            .ok_or("The backup payload length is invalid.")?;
        if end > payload_len {
            return Err("The backup payload is truncated.");
        }
        let amount = i64::try_from(amount).map_err(|_| "The backup payload is too large.")?;
        file.seek(SeekFrom::Current(amount))
            .map_err(|_| "The backup payload is truncated.")?;
        *offset = end;
        Ok(())
    }

    fn preflight_record(
        file: &mut fs::File,
        offset: &mut u64,
        value_end: u64,
    ) -> Result<(), &'static str> {
        let field_count = read_u64(file, offset, value_end)?;
        if field_count > crate::infra::storage::MAX_RECORD_FIELDS as u64 {
            return Err("The backup contains a record with too many fields.");
        }

        let mut field_name = [0_u8; crate::infra::storage::MAX_RECORD_FIELD_NAME_BYTES];
        for _ in 0..field_count {
            let name_len = read_u64(file, offset, value_end)?;
            if name_len == 0 || name_len > field_name.len() as u64 {
                return Err("The backup contains a record with an invalid field name.");
            }
            let name_len = usize::try_from(name_len)
                .map_err(|_| "The backup contains a record with an invalid field name.")?;
            let name_end = offset
                .checked_add(name_len as u64)
                .ok_or("The backup payload length is invalid.")?;
            if name_end > value_end {
                return Err("The backup payload is truncated.");
            }
            file.read_exact(&mut field_name[..name_len])
                .map_err(|_| "The backup payload is truncated.")?;
            *offset = name_end;
            std::str::from_utf8(&field_name[..name_len])
                .map_err(|_| "The backup contains a record with an invalid field name.")?;

            match read_u32(file, offset, value_end)? {
                0 => {}
                1 => {
                    if offset.checked_add(1).is_none_or(|end| end > value_end) {
                        return Err("The backup payload is truncated.");
                    }
                    let mut value = [0_u8; 1];
                    file.read_exact(&mut value)
                        .map_err(|_| "The backup payload is truncated.")?;
                    *offset += 1;
                    if value[0] > 1 {
                        return Err("The backup contains an invalid record encoding.");
                    }
                }
                2 => skip(file, offset, value_end, 8)?,
                3 | 4 => {
                    let value_len = read_u64(file, offset, value_end)?;
                    if value_len > crate::infra::storage::MAX_RECORD_FIELD_BYTES as u64 {
                        return Err("The backup contains an oversized record field.");
                    }
                    skip(file, offset, value_end, value_len)?;
                }
                _ => return Err("The backup contains an invalid record encoding."),
            }
        }
        if *offset != value_end {
            return Err("The backup contains an invalid record encoding.");
        }
        Ok(())
    }

    fn preflight_index_string(
        file: &mut fs::File,
        offset: &mut u64,
        value_end: u64,
    ) -> Result<(), &'static str> {
        let string_len = read_u64(file, offset, value_end)?;
        if string_len > 500 {
            return Err("The backup contains an oversized index value.");
        }
        skip(file, offset, value_end, string_len)?;
        if *offset != value_end {
            return Err("The backup contains an invalid index value.");
        }
        Ok(())
    }

    fn table_has_index_string_value(table: Table) -> bool {
        matches!(
            table,
            Table::Indexes
                | Table::RequestLogIndex
                | Table::StatisticsIndex
                | Table::ApiKeyIndex
                | Table::ApiKeyTokenIndex
                | Table::StreamRunApiKeyIndex
                | Table::ProviderNameIndex
                | Table::ProviderApiKeyIndex
                | Table::ProviderApiKeyAvailabilityIndex
                | Table::ProviderApiKeyCreatedIndex
                | Table::ProviderModelIndex
                | Table::RouteNameIndex
                | Table::RouteTargetProviderIndex
        )
    }

    let mut offset = 0_u64;
    let storage_version = read_u32(file, &mut offset, payload_len)?;
    if storage_version == 0 || storage_version > crate::infra::storage::STORAGE_FORMAT_VERSION {
        return Err("This LMDB storage format version is not supported by this ExoRoute build.");
    }
    let entry_count = read_u64(file, &mut offset, payload_len)?;
    if entry_count > crate::infra::storage::MAX_SNAPSHOT_ENTRIES as u64 {
        return Err("The backup contains too many records.");
    }
    for _ in 0..entry_count {
        let table_index = read_u32(file, &mut offset, payload_len)? as usize;
        let Some(table) = Table::ALL.get(table_index).copied() else {
            return Err("The backup contains an unknown record type.");
        };
        let key_len = read_u64(file, &mut offset, payload_len)?;
        if key_len == 0 || key_len > 500 {
            return Err("The backup contains an invalid storage key.");
        }
        skip(file, &mut offset, payload_len, key_len)?;
        let value_len = read_u64(file, &mut offset, payload_len)?;
        if value_len > crate::infra::storage::MAX_SNAPSHOT_RECORD_BYTES as u64 {
            return Err("The backup contains a record larger than the supported limit.");
        }
        let value_end = offset
            .checked_add(value_len)
            .ok_or("The backup payload length is invalid.")?;
        if value_end > payload_len {
            return Err("The backup payload is truncated.");
        }
        if table == Table::Meta && matches!(value_len, 4 | 8) {
            skip(file, &mut offset, payload_len, value_len)?;
        } else if table_has_index_string_value(table) {
            preflight_index_string(file, &mut offset, value_end)?;
        } else {
            preflight_record(file, &mut offset, value_end)?;
        }
    }
    if offset != payload_len {
        return Err("The backup payload contains trailing data.");
    }
    Ok(())
}
