#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, Address, Env, String, Symbol,
};

/// Extra ledgers beyond `ttl_ledgers` so persistent PayLink data remains readable until
/// after the logical expiry ledger (archival buffer).
const PAYLINK_TTL_BUFFER_LEDGERS: u32 = 16_384;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Creator(String),
    PayLink(String),
    Admin,
    Balance(String),
    StakeBalance(String),
    Paused,
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
    pub cancelled: bool,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    PayLinkAlreadyExists = 1,
    InvalidAmount = 2,
    CreatorNotFound = 3,
    LedgerOverflow = 4,
    Unauthorized = 5,
    UserNotFound = 6,
    ContractPaused = 7,
    InsufficientBalance = 8,
    PayLinkNotFound = 9,
    NotPayLinkCreator = 10,
    PayLinkAlreadyPaid = 11,
}

#[contract]
pub struct PayLinkContract;

#[contractimpl]
impl PayLinkContract {
    /// One-time admin initialisation. Panics if already set.
    pub fn set_admin(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("admin already set");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Marks `username` as an existing creator so `create_paylink` may succeed.
    /// Intended to be invoked from the same onboarding flow that provisions profiles on-chain.
    pub fn register_creator(env: Env, username: String) {
        env.storage()
            .persistent()
            .set(&DataKey::Creator(username), &true);
    }

    pub fn stake(env: Env, username: String, amount: i128) -> Result<(), Error> {
        Self::require_admin(&env)?;
        Self::require_not_paused(&env)?;

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        Self::require_existing_user(&env, &username)?;

        let balance_key = DataKey::Balance(username.clone());
        let current_balance: i128 = env.storage().persistent().get(&balance_key).unwrap_or(0);
        if current_balance < amount {
            return Err(Error::InsufficientBalance);
        }

        let stake_key = DataKey::StakeBalance(username.clone());
        let current_stake_balance: i128 = env.storage().persistent().get(&stake_key).unwrap_or(0);
        let new_balance = current_balance - amount;
        let new_stake_balance = current_stake_balance + amount;

        env.storage().persistent().set(&balance_key, &new_balance);
        env.storage()
            .persistent()
            .set(&stake_key, &new_stake_balance);
        Self::bump_persistent_ttl(&env, &balance_key);
        Self::bump_persistent_ttl(&env, &stake_key);

        env.events().publish(
            (Symbol::new(&env, "staked"),),
            (username, amount, new_stake_balance, env.ledger().sequence()),
        );

        Ok(())
    }

    /// Credits yield to a staker's balance. Admin-only; does NOT check the paused flag.
    pub fn credit_yield(env: Env, username: String, amount: i128) -> Result<(), Error> {
        Self::require_admin(&env)?;

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        Self::require_existing_user(&env, &username)?;

        let stake_key = DataKey::StakeBalance(username.clone());
        let current: i128 = env.storage().persistent().get(&stake_key).unwrap_or(0);
        let new_balance = current + amount;
        env.storage().persistent().set(&stake_key, &new_balance);
        Self::bump_persistent_ttl(&env, &stake_key);

        env.events().publish(
            (Symbol::new(&env, "yield_credited"),),
            (username, amount, new_balance, env.ledger().sequence()),
        );

        Ok(())
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
            cancelled: false,
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

    pub fn cancel_paylink(
        env: Env,
        requester_username: String,
        token_id: String,
    ) -> Result<(), Error> {
        env.current_contract_address().require_auth();

        let paylink_key = DataKey::PayLink(token_id.clone());
        let mut paylink = env
            .storage()
            .persistent()
            .get::<_, PayLinkData>(&paylink_key)
            .ok_or(Error::PayLinkNotFound)?;

        if requester_username != paylink.creator_username {
            return Err(Error::NotPayLinkCreator);
        }

        if paylink.paid {
            return Err(Error::PayLinkAlreadyPaid);
        }

        paylink.cancelled = true;
        env.storage().persistent().set(&paylink_key, &paylink);

        env.events().publish(
            (Symbol::new(&env, "paylink_cancelled"),),
            (requester_username, token_id),
        );

        Ok(())
    }

    pub fn get_paylink(env: Env, token_id: String) -> Option<PayLinkData> {
        env.storage().persistent().get(&DataKey::PayLink(token_id))
    }

    fn require_admin(env: &Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::Unauthorized)?;
        admin.require_auth();
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), Error> {
        let paused = env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false);
        if paused {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    fn require_existing_user(env: &Env, username: &String) -> Result<(), Error> {
        if !env
            .storage()
            .persistent()
            .has(&DataKey::Creator(username.clone()))
        {
            return Err(Error::UserNotFound);
        }
        Ok(())
    }

    fn bump_persistent_ttl(env: &Env, key: &DataKey) {
        env.storage().persistent().extend_ttl(
            key,
            PAYLINK_TTL_BUFFER_LEDGERS,
            PAYLINK_TTL_BUFFER_LEDGERS,
        );
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Ledger};

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

        let stored = client
            .get_paylink(&token_id)
            .expect("expected PayLink in storage");
        assert_eq!(stored.creator_username, creator);
        assert_eq!(stored.amount, 100);
        assert_eq!(stored.note, note);
        assert_eq!(stored.expiration_ledger, 150);
        assert!(!stored.paid);
        assert!(!stored.cancelled);
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
            Err(Ok(Error::PayLinkAlreadyExists))
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
            Err(Ok(Error::InvalidAmount))
        );
    }

    #[test]
    fn cancel_paylink_marks_link_cancelled() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, PayLinkContract);
        let client = PayLinkContractClient::new(&env, &contract_id);

        let creator = String::from_str(&env, "dave");
        let token_id = String::from_str(&env, "tok-cancel");
        let note = String::from_str(&env, "lunch");

        client.register_creator(&creator);
        client.create_paylink(&creator, &token_id, &25_i128, &note, &20);

        client.cancel_paylink(&creator, &token_id);

        let stored = client
            .get_paylink(&token_id)
            .expect("expected PayLink in storage");
        assert!(stored.cancelled);
        assert_eq!(stored.creator_username, creator);
        assert!(!stored.paid);
    }

    #[test]
    fn cancel_paylink_by_non_creator_returns_not_paylink_creator() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, PayLinkContract);
        let client = PayLinkContractClient::new(&env, &contract_id);

        let creator = String::from_str(&env, "erin");
        let other_user = String::from_str(&env, "frank");
        let token_id = String::from_str(&env, "tok-wrong-user");
        let note = String::from_str(&env, "gift");

        client.register_creator(&creator);
        client.create_paylink(&creator, &token_id, &40_i128, &note, &20);

        assert_eq!(
            client.try_cancel_paylink(&other_user, &token_id),
            Err(Ok(Error::NotPayLinkCreator))
        );
    }

    #[test]
    fn cancel_paid_paylink_returns_paylink_already_paid() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, PayLinkContract);
        let client = PayLinkContractClient::new(&env, &contract_id);

        let creator = String::from_str(&env, "grace");
        let token_id = String::from_str(&env, "tok-paid");
        let note = String::from_str(&env, "rent");

        client.register_creator(&creator);
        client.create_paylink(&creator, &token_id, &75_i128, &note, &20);
        set_paylink_paid(&env, &contract_id, &token_id);

        assert_eq!(
            client.try_cancel_paylink(&creator, &token_id),
            Err(Ok(Error::PayLinkAlreadyPaid))
        );
    }

    fn setup_with_admin(env: &Env) -> (Address, PayLinkContractClient<'_>, Address) {
        let contract_id = env.register_contract(None, PayLinkContract);
        let client = PayLinkContractClient::new(env, &contract_id);
        let admin = Address::generate(env);
        client.set_admin(&admin);
        (contract_id, client, admin)
    }

    fn set_balance(env: &Env, contract_id: &Address, username: &String, amount: i128) {
        env.as_contract(contract_id, || {
            env.storage()
                .persistent()
                .set(&DataKey::Balance(username.clone()), &amount);
        });
    }

    fn get_balance(env: &Env, contract_id: &Address, username: &String) -> i128 {
        env.as_contract(contract_id, || {
            env.storage()
                .persistent()
                .get(&DataKey::Balance(username.clone()))
                .unwrap_or(0)
        })
    }

    fn get_stake_balance(env: &Env, contract_id: &Address, username: &String) -> i128 {
        env.as_contract(contract_id, || {
            env.storage()
                .persistent()
                .get(&DataKey::StakeBalance(username.clone()))
                .unwrap_or(0)
        })
    }

    fn set_paylink_paid(env: &Env, contract_id: &Address, token_id: &String) {
        env.as_contract(contract_id, || {
            let mut stored = env
                .storage()
                .persistent()
                .get::<_, PayLinkData>(&DataKey::PayLink(token_id.clone()))
                .expect("expected PayLink in storage");
            stored.paid = true;
            env.storage()
                .persistent()
                .set(&DataKey::PayLink(token_id.clone()), &stored);
        });
    }

    #[test]
    fn stake_partial_balance_moves_amount_to_stake_balance() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, client, _admin) = setup_with_admin(&env);

        let username = String::from_str(&env, "staker-partial");
        client.register_creator(&username);
        set_balance(&env, &contract_id, &username, 100);

        client.stake(&username, &40_i128);

        assert_eq!(get_balance(&env, &contract_id, &username), 60);
        assert_eq!(get_stake_balance(&env, &contract_id, &username), 40);
    }

    #[test]
    fn stake_entire_balance_moves_all_liquid_funds_to_stake_balance() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, client, _admin) = setup_with_admin(&env);

        let username = String::from_str(&env, "staker-full");
        client.register_creator(&username);
        set_balance(&env, &contract_id, &username, 55);

        client.stake(&username, &55_i128);

        assert_eq!(get_balance(&env, &contract_id, &username), 0);
        assert_eq!(get_stake_balance(&env, &contract_id, &username), 55);
    }

    #[test]
    fn over_stake_returns_insufficient_balance() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, client, _admin) = setup_with_admin(&env);

        let username = String::from_str(&env, "staker-over");
        client.register_creator(&username);
        set_balance(&env, &contract_id, &username, 25);

        assert_eq!(
            client.try_stake(&username, &30_i128),
            Err(Ok(Error::InsufficientBalance))
        );
        assert_eq!(get_balance(&env, &contract_id, &username), 25);
        assert_eq!(get_stake_balance(&env, &contract_id, &username), 0);
    }
}
