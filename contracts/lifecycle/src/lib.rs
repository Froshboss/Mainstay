#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, contracterror, panic_with_error, symbol_short, Address, Env, String, Symbol, Vec};

#[contracttype]
#[derive(Clone)]
pub struct MaintenanceRecord {
    pub asset_id: u64,
    pub task_type: Symbol,
    pub notes: String,
    pub engineer: Address,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct Config {
    pub admin: Address,
    pub collateral_threshold: u32,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Error {
    NotInitialized = 1,
    AlreadyInitialized = 2,
    Unauthorized = 3,
}

const CONFIG: Symbol = symbol_short!("CONFIG");

fn history_key(asset_id: u64) -> (Symbol, u64) {
    (symbol_short!("HIST"), asset_id)
}

fn score_key(asset_id: u64) -> (Symbol, u64) {
    (symbol_short!("SCORE"), asset_id)
}

#[contract]
pub struct Lifecycle;

#[contractimpl]
impl Lifecycle {
    pub fn initialize(env: Env, admin: Address, collateral_threshold: u32) {
        if env.storage().instance().has(&CONFIG) {
            panic_with_error!(&env, Error::AlreadyInitialized);
        }
        env.storage().instance().set(&CONFIG, &Config { admin, collateral_threshold });
    }

    pub fn set_threshold(env: Env, caller: Address, new_threshold: u32) {
        caller.require_auth();
        let mut config: Config = env
            .storage()
            .instance()
            .get(&CONFIG)
            .unwrap_or_else(|| panic_with_error!(&env, Error::NotInitialized));
        if config.admin != caller {
            panic_with_error!(&env, Error::Unauthorized);
        }
        config.collateral_threshold = new_threshold;
        env.storage().instance().set(&CONFIG, &config);
    }

    pub fn submit_maintenance(
        env: Env,
        asset_id: u64,
        task_type: Symbol,
        notes: String,
        engineer: Address,
    ) {
        engineer.require_auth();
        let record = MaintenanceRecord {
            asset_id,
            task_type,
            notes,
            engineer,
            timestamp: env.ledger().timestamp(),
        };

        let mut history: Vec<MaintenanceRecord> = env
            .storage()
            .persistent()
            .get(&history_key(asset_id))
            .unwrap_or(Vec::new(&env));
        history.push_back(record);
        env.storage().persistent().set(&history_key(asset_id), &history);

        // increment score (capped at 100)
        let score: u32 = env
            .storage()
            .persistent()
            .get(&score_key(asset_id))
            .unwrap_or(0u32);
        let new_score = (score + 5).min(100);
        env.storage().persistent().set(&score_key(asset_id), &new_score);
    }

    pub fn get_maintenance_history(env: Env, asset_id: u64) -> Vec<MaintenanceRecord> {
        env.storage()
            .persistent()
            .get(&history_key(asset_id))
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_last_service(env: Env, asset_id: u64) -> MaintenanceRecord {
        let history: Vec<MaintenanceRecord> = env
            .storage()
            .persistent()
            .get(&history_key(asset_id))
            .expect("no maintenance history");
        history.last().expect("no records")
    }

    pub fn get_collateral_score(env: Env, asset_id: u64) -> u32 {
        env.storage()
            .persistent()
            .get(&score_key(asset_id))
            .unwrap_or(0)
    }

    pub fn is_collateral_eligible(env: Env, asset_id: u64) -> bool {
        let threshold = env
            .storage()
            .instance()
            .get(&CONFIG)
            .map(|c: Config| c.collateral_threshold)
            .unwrap_or(50);
        Self::get_collateral_score(env, asset_id) >= threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{symbol_short, testutils::Address as _, Env, String};

    fn setup() -> (Env, soroban_sdk::Address, LifecycleClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(Lifecycle, ());
        let client = LifecycleClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin, &50);
        (env, admin, client)
    }

    #[test]
    fn test_submit_and_score() {
        let (env, _, client) = setup();
        let engineer = Address::generate(&env);

        for _ in 0..10 {
            client.submit_maintenance(
                &1u64,
                &symbol_short!("OIL_CHG"),
                &String::from_str(&env, "Routine oil change"),
                &engineer,
            );
        }

        assert_eq!(client.get_collateral_score(&1u64), 50);
        assert!(client.is_collateral_eligible(&1u64));
        assert_eq!(client.get_maintenance_history(&1u64).len(), 10);
    }

    #[test]
    fn test_set_threshold_updates_eligibility() {
        let (env, admin, client) = setup();
        let engineer = Address::generate(&env);

        // 10 records → score 50, eligible at threshold 50
        for _ in 0..10 {
            client.submit_maintenance(
                &2u64,
                &symbol_short!("OIL_CHG"),
                &String::from_str(&env, "Routine oil change"),
                &engineer,
            );
        }
        assert!(client.is_collateral_eligible(&2u64));

        // raise threshold to 75 — same score now ineligible
        client.set_threshold(&admin, &75);
        assert!(!client.is_collateral_eligible(&2u64));

        // lower threshold to 25 — eligible again
        client.set_threshold(&admin, &25);
        assert!(client.is_collateral_eligible(&2u64));
    }

    #[test]
    #[should_panic]
    fn test_set_threshold_rejects_non_admin() {
        let (env, _, client) = setup();
        let non_admin = Address::generate(&env);
        client.set_threshold(&non_admin, &10);
    }

    #[test]
    #[should_panic]
    fn test_initialize_twice_panics() {
        let (_env, admin, client) = setup();
        client.initialize(&admin, &60);
    }
}
