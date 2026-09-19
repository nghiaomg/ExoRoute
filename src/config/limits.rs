use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GatewayResourceLimits {
    pub gateway_body_limit_bytes: usize,
    pub gateway_body_processing_concurrency: usize,
    pub sse_frame_limit_bytes: usize,
    pub sse_buffer_limit_bytes: usize,
    pub provider_max_concurrency: usize,
    pub stream_continuity_enabled: bool,
    pub stream_continuity_max_concurrency: usize,
}

impl GatewayResourceLimits {
    pub const DEFAULT_GATEWAY_BODY_LIMIT_BYTES: usize = 16 * MIB;
    pub const MIN_GATEWAY_BODY_LIMIT_BYTES: usize = MIB;
    pub const MIN_GATEWAY_BODY_PROCESSING_CONCURRENCY: usize = 1;
    /// Zero disables the body/SSE processing concurrency gate.
    pub const UNLIMITED_GATEWAY_BODY_PROCESSING_CONCURRENCY: usize = 0;
    pub const DEFAULT_GATEWAY_BODY_PROCESSING_CONCURRENCY: usize = 4;
    pub const MAX_GATEWAY_BODY_PROCESSING_CONCURRENCY: usize = 8;
    pub const DEFAULT_SSE_FRAME_LIMIT_BYTES: usize = MIB;
    pub const MIN_SSE_FRAME_LIMIT_BYTES: usize = 64 * KIB;
    pub const DEFAULT_SSE_BUFFER_LIMIT_BYTES: usize = 2 * MIB;
    /// Zero disables the per-event SSE size limit.
    pub const UNLIMITED_SSE_FRAME_LIMIT_BYTES: usize = 0;
    /// Zero disables the buffered SSE data limit.
    pub const UNLIMITED_SSE_BUFFER_LIMIT_BYTES: usize = 0;
    pub const MIN_PROVIDER_MAX_CONCURRENCY: usize = 1;
    /// Zero disables the per-provider concurrency gate.
    pub const UNLIMITED_PROVIDER_MAX_CONCURRENCY: usize = 0;
    pub const DEFAULT_PROVIDER_MAX_CONCURRENCY: usize = 32;
    pub const MIN_STREAM_CONTINUITY_MAX_CONCURRENCY: usize = 1;
    pub const DEFAULT_STREAM_CONTINUITY_MAX_CONCURRENCY: usize = 16;
    pub const MAX_STREAM_CONTINUITY_MAX_CONCURRENCY: usize = 64;

    pub fn validate(self) -> Result<(), &'static str> {
        if !(Self::MIN_GATEWAY_BODY_LIMIT_BYTES..=Self::DEFAULT_GATEWAY_BODY_LIMIT_BYTES)
            .contains(&self.gateway_body_limit_bytes)
        {
            return Err("gateway body limit must be between 1 and 16 MiB");
        }
        if self.gateway_body_processing_concurrency
            != Self::UNLIMITED_GATEWAY_BODY_PROCESSING_CONCURRENCY
            && !(Self::MIN_GATEWAY_BODY_PROCESSING_CONCURRENCY
                ..=Self::MAX_GATEWAY_BODY_PROCESSING_CONCURRENCY)
                .contains(&self.gateway_body_processing_concurrency)
        {
            return Err("gateway body processing concurrency must be unlimited or between 1 and 8");
        }
        if self.sse_frame_limit_bytes != Self::UNLIMITED_SSE_FRAME_LIMIT_BYTES
            && self.sse_frame_limit_bytes < Self::MIN_SSE_FRAME_LIMIT_BYTES
        {
            return Err("SSE frame limit must be 0 (unlimited) or at least 64 KiB");
        }
        if self.sse_frame_limit_bytes == Self::UNLIMITED_SSE_FRAME_LIMIT_BYTES
            && self.sse_buffer_limit_bytes != Self::UNLIMITED_SSE_BUFFER_LIMIT_BYTES
        {
            return Err("SSE buffer limit must be 0 (unlimited) when the frame limit is unlimited");
        }
        if self.sse_buffer_limit_bytes != Self::UNLIMITED_SSE_BUFFER_LIMIT_BYTES
            && self.sse_frame_limit_bytes != Self::UNLIMITED_SSE_FRAME_LIMIT_BYTES
            && self.sse_buffer_limit_bytes < self.sse_frame_limit_bytes
        {
            return Err(
                "SSE buffer limit must be 0 (unlimited) or at least the finite frame limit",
            );
        }
        if self.provider_max_concurrency != Self::UNLIMITED_PROVIDER_MAX_CONCURRENCY
            && !(Self::MIN_PROVIDER_MAX_CONCURRENCY..=Self::DEFAULT_PROVIDER_MAX_CONCURRENCY)
                .contains(&self.provider_max_concurrency)
        {
            return Err("provider concurrency must be unlimited or between 1 and 32");
        }
        if !(Self::MIN_STREAM_CONTINUITY_MAX_CONCURRENCY
            ..=Self::MAX_STREAM_CONTINUITY_MAX_CONCURRENCY)
            .contains(&self.stream_continuity_max_concurrency)
        {
            return Err("stream continuity concurrency must be between 1 and 64");
        }
        Ok(())
    }
}

impl Default for GatewayResourceLimits {
    fn default() -> Self {
        Self {
            gateway_body_limit_bytes: Self::DEFAULT_GATEWAY_BODY_LIMIT_BYTES,
            gateway_body_processing_concurrency: Self::DEFAULT_GATEWAY_BODY_PROCESSING_CONCURRENCY,
            sse_frame_limit_bytes: Self::DEFAULT_SSE_FRAME_LIMIT_BYTES,
            sse_buffer_limit_bytes: Self::DEFAULT_SSE_BUFFER_LIMIT_BYTES,
            provider_max_concurrency: Self::DEFAULT_PROVIDER_MAX_CONCURRENCY,
            stream_continuity_enabled: false,
            stream_continuity_max_concurrency: Self::DEFAULT_STREAM_CONTINUITY_MAX_CONCURRENCY,
        }
    }
}

#[cfg(test)]
mod gateway_resource_limits_tests {
    use super::GatewayResourceLimits;

    #[test]
    fn defaults_match_existing_gateway_resource_bounds() {
        let limits = GatewayResourceLimits::default();
        assert_eq!(limits.gateway_body_limit_bytes, 16 * 1024 * 1024);
        assert_eq!(limits.gateway_body_processing_concurrency, 4);
        assert_eq!(limits.sse_frame_limit_bytes, 1024 * 1024);
        assert_eq!(limits.sse_buffer_limit_bytes, 2 * 1024 * 1024);
        assert_eq!(limits.provider_max_concurrency, 32);
        assert_eq!(limits.stream_continuity_max_concurrency, 16);
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn rejects_sse_buffer_smaller_than_frame() {
        let limits = GatewayResourceLimits {
            sse_frame_limit_bytes: 512 * 1024,
            sse_buffer_limit_bytes: 256 * 1024,
            ..GatewayResourceLimits::default()
        };
        assert!(limits.validate().is_err());
    }

    #[test]
    fn allows_unlimited_sse_limits_and_large_finite_values() {
        let unlimited = GatewayResourceLimits {
            sse_frame_limit_bytes: GatewayResourceLimits::UNLIMITED_SSE_FRAME_LIMIT_BYTES,
            sse_buffer_limit_bytes: GatewayResourceLimits::UNLIMITED_SSE_BUFFER_LIMIT_BYTES,
            ..GatewayResourceLimits::default()
        };
        assert!(unlimited.validate().is_ok());

        let large = GatewayResourceLimits {
            sse_frame_limit_bytes: 8 * 1024 * 1024,
            sse_buffer_limit_bytes: 16 * 1024 * 1024,
            ..GatewayResourceLimits::default()
        };
        assert!(large.validate().is_ok());
    }

    #[test]
    fn requires_unlimited_sse_buffer_when_frame_limit_is_unlimited() {
        let limits = GatewayResourceLimits {
            sse_frame_limit_bytes: GatewayResourceLimits::UNLIMITED_SSE_FRAME_LIMIT_BYTES,
            sse_buffer_limit_bytes: 2 * 1024 * 1024,
            ..GatewayResourceLimits::default()
        };
        assert!(limits.validate().is_err());
    }

    #[test]
    fn rejects_limits_above_current_hard_ceilings() {
        let limits = GatewayResourceLimits {
            gateway_body_limit_bytes: 17 * 1024 * 1024,
            ..GatewayResourceLimits::default()
        };
        assert!(limits.validate().is_err());
    }

    #[test]
    fn allows_unlimited_body_processing_concurrency() {
        let limits = GatewayResourceLimits {
            gateway_body_processing_concurrency:
                GatewayResourceLimits::UNLIMITED_GATEWAY_BODY_PROCESSING_CONCURRENCY,
            ..GatewayResourceLimits::default()
        };
        assert!(limits.validate().is_ok());

        let above_finite_limit = GatewayResourceLimits {
            gateway_body_processing_concurrency: 9,
            ..GatewayResourceLimits::default()
        };
        assert!(above_finite_limit.validate().is_err());
    }

    #[test]
    fn allows_unlimited_provider_concurrency_but_rejects_values_above_the_finite_limit() {
        let unlimited = GatewayResourceLimits {
            provider_max_concurrency: GatewayResourceLimits::UNLIMITED_PROVIDER_MAX_CONCURRENCY,
            ..GatewayResourceLimits::default()
        };
        assert!(unlimited.validate().is_ok());

        let above_finite_limit = GatewayResourceLimits {
            provider_max_concurrency: GatewayResourceLimits::DEFAULT_PROVIDER_MAX_CONCURRENCY + 1,
            ..GatewayResourceLimits::default()
        };
        assert!(above_finite_limit.validate().is_err());
    }

    #[test]
    fn requires_a_bounded_stream_continuity_concurrency() {
        let zero = GatewayResourceLimits {
            stream_continuity_max_concurrency: 0,
            ..GatewayResourceLimits::default()
        };
        assert!(zero.validate().is_err());

        let maximum = GatewayResourceLimits {
            stream_continuity_max_concurrency:
                GatewayResourceLimits::MAX_STREAM_CONTINUITY_MAX_CONCURRENCY,
            ..GatewayResourceLimits::default()
        };
        assert!(maximum.validate().is_ok());

        let above_maximum = GatewayResourceLimits {
            stream_continuity_max_concurrency:
                GatewayResourceLimits::MAX_STREAM_CONTINUITY_MAX_CONCURRENCY + 1,
            ..GatewayResourceLimits::default()
        };
        assert!(above_maximum.validate().is_err());
    }
}
