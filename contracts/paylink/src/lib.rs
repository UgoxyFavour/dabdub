#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Env, String, Symbol};

/// Extra ledgers beyond `ttl_ledgers` so persistent PayLink data remains readable until
/// after the logical expiry ledger (archival buffer).
const PAYLINK_TTL_BUFFER_LEDGERS: u32 = 16_384;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Creator(String),
    PayLink(String),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayLinkData {
    pub creator_username: String,
    pub amount: i128,
    pub note: String,
    pub expiration_ledger: u32,
    /// Reserved for single-payment enforcement when claiming or settling a PayLink.
    pub paid: bool,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    PayLinkAlreadyExists = 1,
    InvalidAmount = 2,
    CreatorNotFound = 3,
    LedgerOverflow = 4,
}

#[contract]
pub struct PayLinkContract;

#[contractimpl]
impl PayLinkContract {
    /// Marks `username` as an existing creator so `create_paylink` may succeed.
    /// Intended to be invoked from the same onboarding flow that provisions profiles on-chain.
    pub fn register_creator(env: Env, username: String) {
        env.storage().persistent().set(&DataKey::Creator(username), &true);
    }

    pub fn create_paylink(
        env: Env,
        creator_username: String,
        token_id: String,
        amount: i128,
        note: String,
        ttl_ledgers: u32,
    ) -> Result<(), Error> {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Creator(creator_username.clone()))
        {
            return Err(Error::CreatorNotFound);
        }

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let paylink_key = DataKey::PayLink(token_id.clone());
        if env.storage().persistent().has(&paylink_key) {
            return Err(Error::PayLinkAlreadyExists);
        }

        let current = env.ledger().sequence();
        let expiration_ledger = current
            .checked_add(ttl_ledgers)
            .ok_or(Error::LedgerOverflow)?;

        let data = PayLinkData {
            creator_username: creator_username.clone(),
            amount,
            note,
            expiration_ledger,
            paid: false,
        };

        env.storage().persistent().set(&paylink_key, &data);

        let min_ttl = ttl_ledgers
            .checked_add(PAYLINK_TTL_BUFFER_LEDGERS)
            .ok_or(Error::LedgerOverflow)?;
        env.storage()
            .persistent()
            .extend_ttl(&paylink_key, min_ttl, min_ttl);

        env.events().publish(
            (Symbol::new(&env, "paylink_created"),),
            (creator_username, token_id, amount, expiration_ledger),
        );

        Ok(())
    }

    pub fn get_paylink(env: Env, token_id: String) -> Option<PayLinkData> {
        env.storage()
            .persistent()
            .get(&DataKey::PayLink(token_id))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Ledger;

    #[test]
    fn create_paylink_persists_paylink_data() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PayLinkContract);
        let client = PayLinkContractClient::new(&env, &contract_id);

        let creator = String::from_str(&env, "alice");
        let token_id = String::from_str(&env, "tok-1");
        let note = String::from_str(&env, "coffee");

        client.register_creator(&creator);
        env.ledger().set_sequence_number(100);

        client.create_paylink(&creator, &token_id, &100_i128, &note, &50);

        let stored = client.get_paylink(&token_id).expect("expected PayLink in storage");
        assert_eq!(stored.creator_username, creator);
        assert_eq!(stored.amount, 100);
        assert_eq!(stored.note, note);
        assert_eq!(stored.expiration_ledger, 150);
        assert!(!stored.paid);
    }

    #[test]
    fn duplicate_token_id_returns_paylink_already_exists() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PayLinkContract);
        let client = PayLinkContractClient::new(&env, &contract_id);

        let creator = String::from_str(&env, "bob");
        let token_id = String::from_str(&env, "dup");
        let note = String::from_str(&env, "n");

        client.register_creator(&creator);

        client.create_paylink(&creator, &token_id, &1_i128, &note, &10);
        assert_eq!(
            client.try_create_paylink(&creator, &token_id, &2_i128, &note, &10),
            Ok(Err(Error::PayLinkAlreadyExists))
        );
    }

    #[test]
    fn zero_amount_returns_invalid_amount() {
        let env = Env::default();
        let contract_id = env.register_contract(None, PayLinkContract);
        let client = PayLinkContractClient::new(&env, &contract_id);

        let creator = String::from_str(&env, "carol");
        let token_id = String::from_str(&env, "z");
        let note = String::from_str(&env, "n");

        client.register_creator(&creator);

        assert_eq!(
            client.try_create_paylink(&creator, &token_id, &0_i128, &note, &10),
            Ok(Err(Error::InvalidAmount))
        );
    }
}
