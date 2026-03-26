#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl};

mod storage;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
}

#[contract]
pub struct CheesePay;

#[contractimpl]
impl CheesePay {}

#[cfg(test)]
mod tests {
    use super::storage::{get_instance, get_persistent, DataKey};
    use super::Error;
    use soroban_sdk::{testutils::Address as _, Address, Env, String};

    // ── helpers ──────────────────────────────────────────────────────────────

    fn env() -> Env {
        Env::default()
    }

    fn fake_addr(e: &Env) -> Address {
        Address::generate(e)
    }

    // ── instance round-trips ─────────────────────────────────────────────────

    #[test]
    fn instance_missing_returns_not_initialized() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        e.as_contract(&contract_id, || {
            let result: Result<Address, Error> = get_instance(&e, &DataKey::Admin);
            assert_eq!(result, Err(Error::NotInitialized));
        });
    }

    #[test]
    fn instance_admin_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        let addr = fake_addr(&e);
        e.as_contract(&contract_id, || {
            e.storage().instance().set(&DataKey::Admin, &addr);
            let got: Address = get_instance(&e, &DataKey::Admin).unwrap();
            assert_eq!(got, addr);
        });
    }

    #[test]
    fn instance_usdc_token_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        let addr = fake_addr(&e);
        e.as_contract(&contract_id, || {
            e.storage().instance().set(&DataKey::UsdcToken, &addr);
            let got: Address = get_instance(&e, &DataKey::UsdcToken).unwrap();
            assert_eq!(got, addr);
        });
    }

    #[test]
    fn instance_fee_rate_bps_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        e.as_contract(&contract_id, || {
            e.storage().instance().set(&DataKey::FeeRateBps, &30_u32);
            let got: u32 = get_instance(&e, &DataKey::FeeRateBps).unwrap();
            assert_eq!(got, 30);
        });
    }

    #[test]
    fn instance_fee_treasury_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        let addr = fake_addr(&e);
        e.as_contract(&contract_id, || {
            e.storage().instance().set(&DataKey::FeeTreasury, &addr);
            let got: Address = get_instance(&e, &DataKey::FeeTreasury).unwrap();
            assert_eq!(got, addr);
        });
    }

    #[test]
    fn instance_paused_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        e.as_contract(&contract_id, || {
            e.storage().instance().set(&DataKey::Paused, &true);
            let got: bool = get_instance(&e, &DataKey::Paused).unwrap();
            assert!(got);
        });
    }

    // ── persistent round-trips ───────────────────────────────────────────────

    #[test]
    fn persistent_missing_returns_none() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        let key = DataKey::Balance(String::from_str(&e, "ghost"));
        e.as_contract(&contract_id, || {
            let got: Option<i128> = get_persistent(&e, &key);
            assert!(got.is_none());
        });
    }

    #[test]
    fn persistent_balance_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        let key = DataKey::Balance(String::from_str(&e, "alice"));
        e.as_contract(&contract_id, || {
            e.storage().persistent().set(&key, &500_i128);
            let got: i128 = get_persistent(&e, &key).unwrap();
            assert_eq!(got, 500);
        });
    }

    #[test]
    fn persistent_stake_balance_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        let key = DataKey::StakeBalance(String::from_str(&e, "bob"));
        e.as_contract(&contract_id, || {
            e.storage().persistent().set(&key, &1_000_i128);
            let got: i128 = get_persistent(&e, &key).unwrap();
            assert_eq!(got, 1_000);
        });
    }

    #[test]
    fn persistent_username_to_addr_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        let addr = fake_addr(&e);
        let key = DataKey::UsernameToAddr(String::from_str(&e, "carol"));
        e.as_contract(&contract_id, || {
            e.storage().persistent().set(&key, &addr);
            let got: Address = get_persistent(&e, &key).unwrap();
            assert_eq!(got, addr);
        });
    }

    #[test]
    fn persistent_addr_to_username_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        let addr = fake_addr(&e);
        let username = String::from_str(&e, "dave");
        let key = DataKey::AddrToUsername(addr);
        e.as_contract(&contract_id, || {
            e.storage().persistent().set(&key, &username);
            let got: String = get_persistent(&e, &key).unwrap();
            assert_eq!(got, username);
        });
    }

    #[test]
    fn persistent_paylink_round_trip() {
        let e = env();
        let contract_id = e.register(super::CheesePay, ());
        let key = DataKey::PayLink(String::from_str(&e, "tok-abc"));
        e.as_contract(&contract_id, || {
            e.storage().persistent().set(&key, &999_i128);
            let got: i128 = get_persistent(&e, &key).unwrap();
            assert_eq!(got, 999);
        });
    }
}
