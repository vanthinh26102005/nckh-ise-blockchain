use crate::types::L1Mode;
use anyhow::Result;

pub trait L1Adapter: Send + Sync {
    fn mode(&self) -> L1Mode;

    /// Simulates or executes L1 tx submission and confirmation.
    /// Returns `(confirmed_at_ms, note)`.
    fn submit_and_confirm(
        &self,
        epoch_id: usize,
        lot_id: usize,
        submit_tx_at_ms: u64,
    ) -> Result<(u64, String)>;
}

/// Mock L1 adapter simulating Ethereum 12s block confirmation.
pub struct MockL1Adapter {
    pub confirmation_delay_ms: u64,
    pub mode: L1Mode,
}

impl MockL1Adapter {
    pub fn new(mode: L1Mode) -> Self {
        let confirmation_delay_ms = match mode {
            L1Mode::Mock => 12_000,   // Standard ~12s Ethereum block time
            L1Mode::Local => 1_000,   // Fast local dev node (~1s block)
            L1Mode::Anvil => 1_000,   // Anvil local instant/1s block
            L1Mode::Sepolia => 15_000,// Sepolia testnet average ~15s
        };
        Self {
            confirmation_delay_ms,
            mode,
        }
    }
}

impl L1Adapter for MockL1Adapter {
    fn mode(&self) -> L1Mode {
        self.mode
    }

    fn submit_and_confirm(
        &self,
        _epoch_id: usize,
        _lot_id: usize,
        submit_tx_at_ms: u64,
    ) -> Result<(u64, String)> {
        let confirmed_at_ms = submit_tx_at_ms + self.confirmation_delay_ms;
        let note = match self.mode {
            L1Mode::Mock => "mock_l1_adapter_12s_delay".to_string(),
            L1Mode::Local => "local_l1_adapter_1s_delay".to_string(),
            L1Mode::Anvil => "anvil_l1_adapter_simulated_1s_delay".to_string(),
            L1Mode::Sepolia => "sepolia_l1_adapter_simulated_15s_delay".to_string(),
        };
        Ok((confirmed_at_ms, note))
    }
}

pub fn create_l1_adapter(mode: L1Mode) -> Box<dyn L1Adapter> {
    Box::new(MockL1Adapter::new(mode))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_l1_adapter() {
        let adapter = create_l1_adapter(L1Mode::Mock);
        let submit_time = 100_000u64;
        let (confirmed_at, note) = adapter.submit_and_confirm(1, 1, submit_time).unwrap();

        assert_eq!(adapter.mode(), L1Mode::Mock);
        assert_eq!(confirmed_at, submit_time + 12_000);
        assert!(confirmed_at > submit_time);
        assert!(note.contains("mock_l1_adapter"));
    }
}
