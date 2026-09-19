use crate::provider_adapters;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub(crate) struct ProviderKeyTest {
    pub(crate) test_passed: bool,
    pub(crate) status: Option<u16>,
    pub(crate) message: Option<String>,
    pub(crate) warning: Option<String>,
}

pub(crate) async fn test_provider_credential(
    adapter_id: &str,
    request: provider_adapters::AdapterApiKeyRequest<'_>,
) -> ProviderKeyTest {
    if matches!(
        adapter_id,
        provider_adapters::CLINE_ADAPTER_ID | provider_adapters::CLINEPASS_ADAPTER_ID
    ) {
        let message = "Cline API keys are saved without an automatic inference test; use Test model explicitly.";
        return ProviderKeyTest {
            test_passed: false,
            status: None,
            message: Some(message.to_owned()),
            warning: Some(message.to_owned()),
        };
    }
    let _probe_permit = match request
        .state
        .admin
        .admin_provider_probe_in_flight
        .clone()
        .try_acquire_owned()
    {
        Ok(permit) => permit,
        Err(_) => {
            let message =
                "Provider probe capacity is busy; the key was saved without a completed test.";
            return ProviderKeyTest {
                test_passed: false,
                status: None,
                message: Some(message.to_owned()),
                warning: Some(message.to_owned()),
            };
        }
    };
    let outcome = provider_adapters::test_api_key_credential(adapter_id, request).await;
    ProviderKeyTest {
        test_passed: outcome.test_passed,
        status: outcome.status,
        message: Some(outcome.message.clone()),
        warning: (!outcome.test_passed).then(|| {
            format!(
                "The key was saved, but its test failed: {}",
                outcome.message
            )
        }),
    }
}
